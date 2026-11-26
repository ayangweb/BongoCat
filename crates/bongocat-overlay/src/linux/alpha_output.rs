//! Independent offscreen renderer for the optional V4L2 Alpha camera.

use super::renderer::Renderer;
use crate::OverlayWindowBounds;
use bongocat_platform::{
    ALPHA_FRAME_HEIGHT, ALPHA_FRAME_WIDTH, AlphaCameraError, LinuxAlphaCamera,
};
use bongocat_render::{RenderFrame, RenderSubscription};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const FRAME_INTERVAL: Duration = Duration::from_nanos(1_000_000_000 / 30);
const READBACK_TIMEOUT: Duration = Duration::from_millis(250);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AlphaOutputStatus {
    Disabled,
    Starting,
    Running,
    Stopping,
    CameraFailed(AlphaCameraError),
    RenderFailed,
    WorkerFailed,
}

#[derive(Clone)]
pub(super) struct AlphaSource {
    pub(super) initial: RenderFrame,
    pub(super) subscription: Arc<RenderSubscription>,
    pub(super) bounds: OverlayWindowBounds,
    pub(super) visible: bool,
}

struct RenderLayer {
    source: Arc<RenderSubscription>,
    renderer: Renderer,
    frame: RenderFrame,
}

type Scene = Arc<Mutex<Vec<AlphaSource>>>;

pub(super) struct AlphaOutput {
    events: mpsc::Receiver<AlphaOutputStatus>,
    scene: Scene,
    primary: Arc<RenderSubscription>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl AlphaOutput {
    pub(super) fn start(
        initial: RenderFrame,
        environment: bongocat_config::BuildEnvironment,
        instance: wgpu::Instance,
        subscription: RenderSubscription,
    ) -> std::io::Result<Self> {
        let primary = AlphaSource {
            initial,
            subscription: Arc::new(subscription),
            bounds: OverlayWindowBounds::new(0, 0, ALPHA_FRAME_WIDTH, ALPHA_FRAME_HEIGHT),
            visible: true,
        };
        let primary_subscription = Arc::clone(&primary.subscription);
        let scene = Arc::new(Mutex::new(vec![primary]));
        let worker_scene = Arc::clone(&scene);
        let (event_tx, events) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop);
        let thread = thread::Builder::new()
            .name("bongocat-alpha-output".into())
            .spawn(move || {
                let result = run(
                    environment,
                    instance,
                    &worker_scene,
                    &thread_stop,
                    &event_tx,
                );
                if let Err(status) = result {
                    let _ = event_tx.send(status);
                }
            })?;
        Ok(Self {
            events,
            scene,
            primary: primary_subscription,
            stop,
            thread: Some(thread),
        })
    }

    pub(super) fn primary_source(&self) -> Arc<RenderSubscription> {
        Arc::clone(&self.primary)
    }

    pub(super) fn set_scene(&self, sources: Vec<AlphaSource>) {
        if let Ok(mut scene) = self.scene.try_lock() {
            *scene = sources;
        }
    }

    pub(super) fn latest_status(&self) -> Option<AlphaOutputStatus> {
        self.events.try_iter().last()
    }

    pub(super) fn request_stop(&self) {
        self.stop.store(true, Ordering::Release);
    }

    pub(super) fn is_stopping(&self) -> bool {
        self.stop.load(Ordering::Acquire)
    }

    pub(super) fn is_finished(&self) -> bool {
        self.thread.as_ref().is_none_or(JoinHandle::is_finished)
    }
}

impl Drop for AlphaOutput {
    fn drop(&mut self) {
        self.request_stop();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn run(
    environment: bongocat_config::BuildEnvironment,
    instance: wgpu::Instance,
    scene: &Scene,
    stop: &AtomicBool,
    events: &mpsc::Sender<AlphaOutputStatus>,
) -> Result<(), AlphaOutputStatus> {
    if stop.load(Ordering::Acquire) {
        return Ok(());
    }
    let mut camera =
        LinuxAlphaCamera::discover(environment).map_err(AlphaOutputStatus::CameraFailed)?;
    let mut layers: Vec<RenderLayer> = Vec::new();
    let mut pixels = vec![0; ALPHA_FRAME_WIDTH as usize * ALPHA_FRAME_HEIGHT as usize * 4];
    let mut deadline = Instant::now();
    let mut announced = false;
    while !stop.load(Ordering::Acquire) {
        let sources = scene
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        let layout = fit_scene(
            &sources
                .iter()
                .map(|source| source.bounds)
                .collect::<Vec<_>>(),
        );
        layers.retain(|layer| {
            sources
                .iter()
                .any(|source| Arc::ptr_eq(&layer.source, &source.subscription))
        });
        pixels.fill(0);
        for (source, rect) in sources.iter().zip(layout) {
            if stop.load(Ordering::Acquire) {
                return Ok(());
            }
            if !source.visible {
                continue;
            }
            let latest = source.subscription.take_latest();
            let index = if let Some(index) = layers
                .iter()
                .position(|layer| Arc::ptr_eq(&layer.source, &source.subscription))
            {
                index
            } else {
                let frame = latest.as_ref().unwrap_or(&source.initial);
                let renderer =
                    Renderer::with_instance(instance.clone(), None, frame, rect.width, rect.height)
                        .map_err(|_| AlphaOutputStatus::RenderFailed)?;
                layers.push(RenderLayer {
                    source: Arc::clone(&source.subscription),
                    renderer,
                    frame: frame.clone(),
                });
                layers.len() - 1
            };
            let layer = &mut layers[index];
            if let Some(frame) = latest {
                if layer.frame.model_generation != frame.model_generation {
                    layer
                        .renderer
                        .prepare(&frame)
                        .map_err(|_| AlphaOutputStatus::RenderFailed)?;
                }
                layer.frame = frame;
            }
            layer.renderer.resize(rect.width, rect.height);
            layer
                .renderer
                .draw_alpha(&layer.frame)
                .map_err(|_| AlphaOutputStatus::RenderFailed)?;
            let captured = layer
                .renderer
                .readback_with_timeout(READBACK_TIMEOUT)
                .map_err(|_| AlphaOutputStatus::RenderFailed)?;
            composite_bgra(&mut pixels, ALPHA_FRAME_WIDTH, rect, captured.pixels());
        }
        let written = camera
            .write_frame(&pixels)
            .map_err(AlphaOutputStatus::CameraFailed)?;
        if !announced && written == bongocat_platform::AlphaFrameWrite::Written {
            announced = true;
            let _ = events.send(AlphaOutputStatus::Running);
        }
        deadline += FRAME_INTERVAL;
        let now = Instant::now();
        if let Some(delay) = deadline.checked_duration_since(now) {
            thread::sleep(delay);
        } else {
            deadline = now;
        }
    }
    Ok(())
}

/// Fit all selected windows together, including offline slots, into one transparent frame.
fn fit_scene(bounds: &[OverlayWindowBounds]) -> Vec<OverlayWindowBounds> {
    match bounds {
        [] => return Vec::new(),
        [_] => {
            return vec![OverlayWindowBounds::new(
                0,
                0,
                ALPHA_FRAME_WIDTH,
                ALPHA_FRAME_HEIGHT,
            )];
        }
        _ => {}
    }
    let left = bounds
        .iter()
        .map(|r| f64::from(r.x))
        .fold(f64::INFINITY, f64::min);
    let top = bounds
        .iter()
        .map(|r| f64::from(r.y))
        .fold(f64::INFINITY, f64::min);
    let right = bounds
        .iter()
        .map(|r| f64::from(r.x) + f64::from(r.width))
        .fold(f64::NEG_INFINITY, f64::max);
    let bottom = bounds
        .iter()
        .map(|r| f64::from(r.y) + f64::from(r.height))
        .fold(f64::NEG_INFINITY, f64::max);
    let width = (right - left).max(1.0);
    let height = (bottom - top).max(1.0);
    let scale = (f64::from(ALPHA_FRAME_WIDTH) / width).min(f64::from(ALPHA_FRAME_HEIGHT) / height);
    let offset_x = (f64::from(ALPHA_FRAME_WIDTH) - width * scale) / 2.0;
    let offset_y = (f64::from(ALPHA_FRAME_HEIGHT) - height * scale) / 2.0;
    bounds
        .iter()
        .map(|r| {
            let x = (offset_x + (f64::from(r.x) - left) * scale).round() as u32;
            let y = (offset_y + (f64::from(r.y) - top) * scale).round() as u32;
            let x = x.min(ALPHA_FRAME_WIDTH - 1);
            let y = y.min(ALPHA_FRAME_HEIGHT - 1);
            let w = ((f64::from(r.width) * scale).round() as u32).clamp(1, ALPHA_FRAME_WIDTH - x);
            let h = ((f64::from(r.height) * scale).round() as u32).clamp(1, ALPHA_FRAME_HEIGHT - y);
            OverlayWindowBounds::new(x as i32, y as i32, w, h)
        })
        .collect()
}

/// Composite straight-alpha RGBA readback into the camera's straight-alpha BGRA frame.
fn composite_bgra(output: &mut [u8], stride: u32, rect: OverlayWindowBounds, source: &[u8]) {
    let row_bytes = rect.width as usize * 4;
    for (row, src) in source.chunks_exact(row_bytes).enumerate() {
        let offset = ((rect.y as usize + row) * stride as usize + rect.x as usize) * 4;
        for (src, dst) in src
            .chunks_exact(4)
            .zip(output[offset..offset + row_bytes].chunks_exact_mut(4))
        {
            let foreground = u32::from(src[3]);
            if foreground == 0 {
                continue;
            }
            if foreground == 255 || dst[3] == 0 {
                dst.copy_from_slice(&[src[2], src[1], src[0], src[3]]);
                continue;
            }
            let background = u32::from(dst[3]) * (255 - foreground);
            let alpha = foreground * 255 + background;
            for (channel, original) in [2, 1, 0].into_iter().zip(&mut dst[..3]) {
                let color =
                    u32::from(src[channel]) * foreground * 255 + u32::from(*original) * background;
                *original = ((color + alpha / 2) / alpha) as u8;
            }
            dst[3] = ((alpha + 127) / 255) as u8;
        }
    }
}

#[cfg(test)]
fn rgba_to_bgra(rgba: &[u8]) -> Vec<u8> {
    let mut bgra = rgba.to_vec();
    for pixel in bgra.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    bgra
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scene_fits_negative_origins_and_preserves_relative_gaps() {
        let layout = fit_scene(&[
            OverlayWindowBounds::new(-400, 50, 200, 100),
            OverlayWindowBounds::new(0, 50, 200, 100),
            OverlayWindowBounds::new(400, 150, 200, 100),
        ]);
        assert_eq!(layout[0].x, 0);
        assert_eq!(layout[0].width, 256);
        assert_eq!(layout[1].x - layout[0].x, 512);
        assert_eq!(layout[2].x - layout[1].x, 512);
        assert_eq!(layout[2].y - layout[1].y, 128);
    }

    #[test]
    fn group_composite_preserves_holes_and_blends_straight_alpha() {
        let mut output = [0; 12];
        let rect = OverlayWindowBounds::new(1, 0, 1, 1);
        composite_bgra(&mut output, 3, rect, &[255, 0, 0, 128]);
        composite_bgra(&mut output, 3, rect, &[0, 0, 255, 128]);
        assert_eq!(output, [0, 0, 0, 0, 170, 0, 85, 192, 0, 0, 0, 0]);
    }

    #[test]
    #[ignore = "requires a Vulkan GPU and the standard preset model"]
    fn alpha_render_includes_model_background_and_remains_transparent() {
        use bongocat_model::{ModelId, ModelPackageLimits, PresetModelCatalog};
        use bongocat_runtime::{RuntimeCommand, RuntimeOwner};

        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .expect("repository root");
        let model =
            PresetModelCatalog::open(root.join("resources/models"), ModelPackageLimits::default())
                .expect("preset catalog")
                .load(&ModelId::parse("standard").expect("model id"))
                .expect("standard model");
        let (runtime, consumer) = RuntimeOwner::start_with_rendering(false, 64);
        let client = runtime.client();
        let timeout = Duration::from_secs(3);
        client.wait_for_revision(1, timeout).expect("runtime ready");
        let command = client
            .send(RuntimeCommand::ActivateModel(Arc::new(model)))
            .expect("activate model");
        client
            .wait_for_model_preparation(command, timeout)
            .expect("model prepared");
        let frame = consumer.take_latest().expect("model frame");
        assert!(
            frame.resources.background.is_some(),
            "fixture includes the keyboard and mat"
        );
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::VULKAN,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let mut renderer =
            Renderer::with_instance(instance, None, &frame, 1280, 720).expect("offscreen renderer");
        renderer
            .draw_alpha(&frame)
            .expect("draw alpha with background");
        let with = renderer.readback().expect("read alpha frame");
        let mut without_frame = frame.clone();
        Arc::make_mut(&mut without_frame.resources).background = None;
        renderer
            .prepare(&without_frame)
            .expect("remove background from fixture");
        renderer
            .draw_alpha(&without_frame)
            .expect("draw without background");
        let without = renderer.readback().expect("read frame without background");
        assert!(
            with.pixels().chunks_exact(4).any(|pixel| pixel[3] == 0),
            "background asset must not make the entire frame opaque"
        );
        assert!(
            with.pixels()
                .chunks_exact(4)
                .zip(without.pixels().chunks_exact(4))
                .any(|(a, b)| a[3] > b[3]),
            "the model background must contribute visible pixels"
        );
        runtime
            .shutdown(Duration::from_secs(2))
            .expect("runtime shutdown");
    }

    #[test]
    #[ignore = "requires a disposable BongoCat Alpha (Development) v4l2loopback and Vulkan GPU"]
    fn gpu_loopback_smoke_preserves_model_alpha_and_stops() {
        use bongocat_model::{ModelId, ModelPackageLimits, PresetModelCatalog};
        use bongocat_runtime::{RuntimeCommand, RuntimeOwner};
        use std::process::{Command, Stdio};

        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .expect("repository root");
        let model =
            PresetModelCatalog::open(root.join("resources/models"), ModelPackageLimits::default())
                .expect("preset catalog")
                .load(&ModelId::parse("standard").expect("model id"))
                .expect("standard model");
        let (runtime, consumer) = RuntimeOwner::start_with_rendering(false, 64);
        let client = runtime.client();
        let timeout = Duration::from_secs(3);
        client.wait_for_revision(1, timeout).expect("runtime ready");
        let output_command = client
            .send(RuntimeCommand::SetVideoOutputStatus(
                bongocat_runtime::VideoOutputStatus::Running,
            ))
            .expect("enable video output");
        let snapshot = client
            .wait_for_command(output_command, timeout)
            .expect("video output active");
        assert!(!snapshot.overlay_visible);
        assert!(snapshot.video_output_active);
        let command = client
            .send(RuntimeCommand::ActivateModel(Arc::new(model)))
            .expect("activate model");
        client
            .wait_for_model_preparation(command, timeout)
            .expect("model prepared");
        let frame = consumer.take_latest().expect("prepared model frame");
        let subscription = consumer.subscribe_latest();
        consumer
            .report_model_commit(bongocat_render::ModelCommitFeedback {
                token: frame.model_commit.expect("model commit"),
                outcome: bongocat_render::ModelCommitOutcome::Prepared,
            })
            .expect("commit model");
        client
            .wait_for_command(command, timeout)
            .expect("model committed");
        let environment = if std::env::var_os("BONGOCAT_ALPHA_TEST_PRODUCTION").is_some() {
            bongocat_config::BuildEnvironment::Production
        } else {
            bongocat_config::BuildEnvironment::Development
        };
        let output = AlphaOutput::start(
            frame.clone(),
            environment,
            wgpu::Instance::new(wgpu::InstanceDescriptor {
                backends: wgpu::Backends::VULKAN,
                ..wgpu::InstanceDescriptor::new_without_display_handle()
            }),
            subscription,
        )
        .expect("start alpha worker");
        let start = Instant::now();
        let mut running = false;
        while start.elapsed() < Duration::from_secs(10) {
            match output.latest_status() {
                Some(AlphaOutputStatus::Running) => {
                    running = true;
                    break;
                }
                Some(status) => panic!("alpha output failed: {status:?}"),
                None => thread::sleep(Duration::from_millis(20)),
            }
        }
        assert!(running, "alpha output did not start");
        let file_path =
            std::env::temp_dir().join(format!("bongocat-alpha-smoke-{}.bgra", std::process::id()));
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&file_path)
            .expect("temporary capture");
        let device = std::env::var("BONGOCAT_ALPHA_TEST_DEVICE").expect("test camera path");
        let mut capture = Command::new("/usr/bin/v4l2-ctl")
            .args([
                "--device",
                &device,
                "--stream-mmap=2",
                "--stream-count=1",
                "--stream-to=-",
            ])
            .stdout(Stdio::from(file))
            .stderr(Stdio::null())
            .spawn()
            .expect("capture command");
        let start = Instant::now();
        let status = loop {
            if let Some(status) = capture.try_wait().expect("capture status") {
                break status;
            }
            if start.elapsed() >= Duration::from_secs(5) {
                let _ = capture.kill();
                let _ = capture.wait();
                panic!("camera capture timed out");
            }
            thread::sleep(Duration::from_millis(20));
        };
        assert!(status.success(), "capture command failed");
        let pixels = std::fs::read(&file_path).expect("captured frame");
        std::fs::remove_file(file_path).expect("remove temporary frame");
        assert_eq!(pixels.len(), bongocat_platform::ALPHA_FRAME_BYTES);
        assert!(
            pixels.chunks_exact(4).any(|p| p[3] == 0),
            "no transparent pixels"
        );
        assert!(
            pixels.chunks_exact(4).any(|p| p[3] == 255),
            "no opaque pixels"
        );
        assert!(
            pixels.chunks_exact(4).any(|p| (1..255).contains(&p[3])),
            "no translucent edges"
        );
        // The desktop consumer is deliberately not polled during this capture:
        // output must keep receiving runtime snapshots on its own.
        let frames = std::env::var("BONGOCAT_ALPHA_TEST_FRAMES")
            .ok()
            .and_then(|value| value.parse::<u32>().ok())
            .filter(|value| (1..=300).contains(value))
            .unwrap_or(12);
        let cadence = Command::new("/usr/bin/timeout")
            .args([
                "15s",
                "/usr/bin/v4l2-ctl",
                "--device",
                &device,
                "--stream-mmap=2",
                &format!("--stream-count={frames}"),
                "--stream-to=/dev/null",
            ])
            .stdout(Stdio::null())
            .status()
            .expect("capture independent cadence");
        assert!(
            cadence.success(),
            "output stalled without desktop frame ticks"
        );
        let stop_start = Instant::now();
        drop(output);
        assert!(
            stop_start.elapsed() < Duration::from_secs(2),
            "alpha worker shutdown timed out"
        );
        runtime
            .shutdown(Duration::from_secs(2))
            .expect("runtime shutdown");
    }

    #[test]
    fn swaps_color_channels_without_changing_alpha() {
        assert_eq!(
            rgba_to_bgra(&[1, 2, 3, 4, 11, 12, 13, 14]),
            [3, 2, 1, 4, 13, 12, 11, 14]
        );
    }
}
