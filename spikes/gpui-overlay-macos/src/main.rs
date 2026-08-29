#[cfg(target_os = "macos")]
mod macos_overlay {
    use metal::{
        Buffer, CommandQueue, CompileOptions, Device, MTLBlendFactor, MTLClearColor,
        MTLCommandBufferStatus, MTLLoadAction, MTLOrigin, MTLPixelFormat, MTLPrimitiveType,
        MTLRegion, MTLResourceOptions, MTLSize, MTLStoreAction, MetalLayer, RenderPassDescriptor,
        RenderPipelineDescriptor, RenderPipelineState,
    };
    use objc2::{
        MainThreadMarker, MainThreadOnly,
        rc::{Retained, autoreleasepool},
    };
    use objc2_app_kit::{
        NSApplication, NSBackingStoreType, NSColor, NSFloatingWindowLevel, NSPanel, NSView,
        NSWindowAnimationBehavior, NSWindowCollectionBehavior, NSWindowStyleMask,
    };
    use objc2_foundation::{NSPoint, NSRect, NSSize};
    use std::{
        mem,
        sync::atomic::{AtomicUsize, Ordering},
    };

    static LIVE_OVERLAY_OWNERS: AtomicUsize = AtomicUsize::new(0);

    const OVERLAY_WIDTH: usize = 320;
    const OVERLAY_HEIGHT: usize = 240;
    const SHADER_SOURCE: &str = r#"
        #include <metal_stdlib>
        using namespace metal;

        struct Vertex {
            float2 position;
            float4 color;
        };

        struct RasterVertex {
            float4 position [[position]];
            float4 color;
        };

        vertex RasterVertex overlay_vertex(
            const device Vertex* vertices [[buffer(0)]],
            uint vertex_id [[vertex_id]]
        ) {
            RasterVertex output;
            output.position = float4(vertices[vertex_id].position, 0.0, 1.0);
            output.color = vertices[vertex_id].color;
            return output;
        }

        fragment float4 overlay_fragment(RasterVertex input [[stage_in]]) {
            return input.color;
        }
    "#;

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct OverlayVertex {
        position: [f32; 2],
        _metal_float4_alignment: [f32; 2],
        premultiplied_color: [f32; 4],
    }

    const OVERLAY_VERTICES: [OverlayVertex; 3] = [
        OverlayVertex {
            position: [0.0, 0.72],
            _metal_float4_alignment: [0.0; 2],
            premultiplied_color: [0.72, 0.25, 0.06, 0.78],
        },
        OverlayVertex {
            position: [-0.68, -0.62],
            _metal_float4_alignment: [0.0; 2],
            premultiplied_color: [0.16, 0.58, 0.68, 0.78],
        },
        OverlayVertex {
            position: [0.68, -0.62],
            _metal_float4_alignment: [0.0; 2],
            premultiplied_color: [0.48, 0.16, 0.64, 0.78],
        },
    ];

    #[derive(Debug)]
    pub struct CycleReport {
        pub cycles: u32,
        pub windows_before: usize,
        pub windows_after: usize,
        pub owners_before: usize,
        pub owners_after: usize,
    }

    #[derive(Clone, Copy)]
    pub struct ResourceCounts {
        windows: usize,
        owners: usize,
    }

    #[derive(Clone, Copy)]
    enum PresentBehavior {
        Submit,
        SubmitAndWait,
        SimulateDrawableUnavailable,
    }

    pub struct NativeOverlay {
        panel: mem::ManuallyDrop<Retained<NSPanel>>,
        layer: Option<MetalLayer>,
        queue: Option<CommandQueue>,
        pipeline: Option<RenderPipelineState>,
        vertex_buffer: Option<Buffer>,
        log_lifecycle: bool,
    }

    impl NativeOverlay {
        pub fn create(mtm: MainThreadMarker) -> Result<Self, String> {
            Self::create_with_logging(mtm, true)
        }

        fn create_with_logging(mtm: MainThreadMarker, log_lifecycle: bool) -> Result<Self, String> {
            let frame = NSRect::new(NSPoint::new(80.0, 80.0), NSSize::new(320.0, 240.0));
            let style = NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel;
            // SAFETY: creation is performed on the AppKit main thread and the
            // selected style/backing values are valid NSPanel initializer inputs.
            let panel = NSPanel::initWithContentRect_styleMask_backing_defer(
                NSPanel::alloc(mtm),
                frame,
                style,
                NSBackingStoreType::Buffered,
                false,
            );
            panel.setOpaque(false);
            panel.setHasShadow(false);
            panel.setAnimationBehavior(NSWindowAnimationBehavior::None);
            panel.setBackgroundColor(Some(&NSColor::clearColor()));
            panel.setLevel(NSFloatingWindowLevel);
            panel.setCollectionBehavior(
                NSWindowCollectionBehavior::CanJoinAllSpaces
                    | NSWindowCollectionBehavior::FullScreenAuxiliary,
            );
            panel.setIgnoresMouseEvents(true);

            let view = NSView::new(mtm);
            view.setWantsLayer(true);
            let device = Device::system_default().ok_or("Metal device unavailable")?;
            let layer = MetalLayer::new();
            layer.set_device(&device);
            layer.set_pixel_format(MTLPixelFormat::BGRA8Unorm);
            layer.set_opaque(false);
            layer.set_presents_with_transaction(false);
            layer.set_framebuffer_only(false);
            configure_drawable_size(&panel, &layer, OVERLAY_WIDTH as f64, OVERLAY_HEIGHT as f64)?;
            // SAFETY: metal::MetalLayerRef and objc2_quartz_core::CALayer are
            // the same Objective-C CAMetalLayer object; the view retains it.
            let layer_ref = unsafe {
                mem::transmute::<&metal::MetalLayerRef, &objc2_quartz_core::CALayer>(layer.as_ref())
            };
            view.setLayer(Some(layer_ref));
            panel.setContentView(Some(&view));

            let pipeline = create_render_pipeline(&device)?;
            let vertex_buffer = device.new_buffer_with_data(
                OVERLAY_VERTICES.as_ptr().cast(),
                size_of_val(&OVERLAY_VERTICES) as u64,
                MTLResourceOptions::StorageModeShared,
            );
            let overlay = Self {
                panel: mem::ManuallyDrop::new(panel),
                layer: Some(layer),
                queue: Some(device.new_command_queue()),
                pipeline: Some(pipeline),
                vertex_buffer: Some(vertex_buffer),
                log_lifecycle,
            };
            LIVE_OVERLAY_OWNERS.fetch_add(1, Ordering::AcqRel);
            Ok(overlay)
        }

        pub fn show(&self) {
            self.panel.orderFrontRegardless();
            if self.log_lifecycle {
                println!("gpui-overlay-macos-spike: overlay shown");
            }
        }

        pub fn hide(&self) {
            self.panel.orderOut(None);
            if self.log_lifecycle {
                println!("gpui-overlay-macos-spike: overlay hidden");
            }
        }

        pub fn clear_present(&self) -> Result<(), String> {
            self.clear_present_with_behavior(PresentBehavior::Submit, true)
        }

        pub fn render_scheduled_frame(&self) -> Result<(), String> {
            self.clear_present_with_behavior(PresentBehavior::Submit, false)
        }

        pub fn resize(&mut self, width: u32, height: u32) -> Result<(), String> {
            if width == 0 || height == 0 {
                return Err("overlay dimensions must be non-zero".into());
            }
            let logical_width = width as f64;
            let logical_height = height as f64;
            self.panel
                .setContentSize(NSSize::new(logical_width, logical_height));
            configure_drawable_size(
                &self.panel,
                self.layer
                    .as_ref()
                    .expect("live overlay must own a Metal layer"),
                logical_width,
                logical_height,
            )?;
            if self.log_lifecycle {
                println!(
                    "gpui-overlay-spike: macOS overlay resized width={width} height={height} scale={}",
                    self.panel.backingScaleFactor()
                );
            }
            Ok(())
        }

        pub fn clear_present_simulating_unavailable(&self) -> Result<(), String> {
            self.clear_present_with_behavior(PresentBehavior::SimulateDrawableUnavailable, true)
        }

        fn clear_present_and_wait(&self) -> Result<(), String> {
            self.clear_present_with_behavior(PresentBehavior::SubmitAndWait, true)
        }

        fn clear_present_with_behavior(
            &self,
            behavior: PresentBehavior,
            log_frame: bool,
        ) -> Result<(), String> {
            if matches!(behavior, PresentBehavior::SimulateDrawableUnavailable) {
                return Err("simulated CAMetalLayer drawable unavailable".into());
            }
            let drawable = self
                .layer
                .as_ref()
                .expect("live overlay must own a Metal layer")
                .next_drawable()
                .ok_or("CAMetalLayer returned no drawable")?;
            let expected_size = self
                .layer
                .as_ref()
                .expect("live overlay must own a Metal layer")
                .drawable_size();
            if drawable.texture().width() != expected_size.width.round() as u64
                || drawable.texture().height() != expected_size.height.round() as u64
            {
                return Err(format!(
                    "Metal drawable size {}x{} does not match layer {}x{}",
                    drawable.texture().width(),
                    drawable.texture().height(),
                    expected_size.width,
                    expected_size.height
                ));
            }
            let pass = RenderPassDescriptor::new();
            let attachment = pass
                .color_attachments()
                .object_at(0)
                .ok_or("missing color attachment")?;
            attachment.set_texture(Some(drawable.texture()));
            attachment.set_load_action(MTLLoadAction::Clear);
            attachment.set_store_action(MTLStoreAction::Store);
            attachment.set_clear_color(MTLClearColor::new(0.0, 0.0, 0.0, 0.0));
            let command_buffer = self
                .queue
                .as_ref()
                .expect("live overlay must own a Metal command queue")
                .new_command_buffer();
            let encoder = command_buffer.new_render_command_encoder(pass);
            encoder.set_render_pipeline_state(
                self.pipeline
                    .as_ref()
                    .expect("live overlay must own a Metal render pipeline"),
            );
            encoder.set_vertex_buffer(
                0,
                Some(
                    self.vertex_buffer
                        .as_ref()
                        .expect("live overlay must own a Metal vertex buffer"),
                ),
                0,
            );
            encoder.draw_primitives(MTLPrimitiveType::Triangle, 0, OVERLAY_VERTICES.len() as u64);
            encoder.end_encoding();
            command_buffer.present_drawable(drawable);
            command_buffer.commit();
            if matches!(behavior, PresentBehavior::SubmitAndWait) {
                command_buffer.wait_until_completed();
                let status = command_buffer.status();
                if status != MTLCommandBufferStatus::Completed {
                    return Err(format!(
                        "Metal command buffer did not complete successfully: {status:?}"
                    ));
                }
                verify_non_empty_frame(drawable.texture())?;
            }
            if self.log_lifecycle && log_frame {
                println!(
                    "gpui-overlay-macos-spike: non-empty premultiplied-alpha draw/present submitted"
                );
            }
            Ok(())
        }
    }

    impl Drop for NativeOverlay {
        fn drop(&mut self) {
            self.panel.setContentView(None);
            drop(self.vertex_buffer.take());
            drop(self.pipeline.take());
            drop(self.queue.take());
            drop(self.layer.take());
            // SAFETY: releasedWhenClosed transfers the retain represented by
            // the ManuallyDrop field to AppKit's close path. close() consumes
            // that retain; the field must never be released or accessed again.
            // The content view is detached and Rust-owned Metal objects are
            // released before the panel is destroyed.
            unsafe { self.panel.setReleasedWhenClosed(true) };
            self.panel.close();
            let previous = LIVE_OVERLAY_OWNERS.fetch_sub(1, Ordering::AcqRel);
            assert!(previous > 0, "macOS overlay owner count underflow");
            if self.log_lifecycle {
                println!("gpui-overlay-macos-spike: overlay window/GPU owner released");
            }
        }
    }

    fn create_render_pipeline(device: &Device) -> Result<RenderPipelineState, String> {
        let library = device
            .new_library_with_source(SHADER_SOURCE, &CompileOptions::new())
            .map_err(|error| format!("compile Metal overlay shaders: {error}"))?;
        let vertex_function = library
            .get_function("overlay_vertex", None)
            .map_err(|error| format!("load Metal overlay vertex function: {error}"))?;
        let fragment_function = library
            .get_function("overlay_fragment", None)
            .map_err(|error| format!("load Metal overlay fragment function: {error}"))?;
        let descriptor = RenderPipelineDescriptor::new();
        descriptor.set_vertex_function(Some(&vertex_function));
        descriptor.set_fragment_function(Some(&fragment_function));
        let attachment = descriptor
            .color_attachments()
            .object_at(0)
            .ok_or("missing Metal pipeline color attachment")?;
        attachment.set_pixel_format(MTLPixelFormat::BGRA8Unorm);
        attachment.set_blending_enabled(true);
        attachment.set_source_rgb_blend_factor(MTLBlendFactor::One);
        attachment.set_destination_rgb_blend_factor(MTLBlendFactor::OneMinusSourceAlpha);
        attachment.set_source_alpha_blend_factor(MTLBlendFactor::One);
        attachment.set_destination_alpha_blend_factor(MTLBlendFactor::OneMinusSourceAlpha);
        device
            .new_render_pipeline_state(&descriptor)
            .map_err(|error| format!("create Metal overlay render pipeline: {error}"))
    }

    fn configure_drawable_size(
        panel: &NSPanel,
        layer: &MetalLayer,
        logical_width: f64,
        logical_height: f64,
    ) -> Result<(), String> {
        let scale = panel.backingScaleFactor();
        if !scale.is_finite() || scale <= 0.0 {
            return Err(format!("invalid AppKit backing scale factor: {scale}"));
        }
        layer.set_drawable_size(core_graphics_types::geometry::CGSize::new(
            logical_width * scale,
            logical_height * scale,
        ));
        Ok(())
    }

    fn verify_non_empty_frame(texture: &metal::TextureRef) -> Result<(), String> {
        let width = texture.width();
        let height = texture.height();
        if width == 0 || height == 0 {
            return Err("Metal drawable has zero dimensions".into());
        }
        let mut pixel = [0_u8; 4];
        texture.get_bytes(
            pixel.as_mut_ptr().cast(),
            pixel.len() as u64,
            MTLRegion {
                origin: MTLOrigin {
                    x: width / 2,
                    y: height / 2,
                    z: 0,
                },
                size: MTLSize {
                    width: 1,
                    height: 1,
                    depth: 1,
                },
            },
            0,
        );
        let [blue, green, red, alpha] = pixel;
        if alpha == 0 {
            return Err("Metal readback found a transparent center pixel after draw".into());
        }
        if red > alpha || green > alpha || blue > alpha {
            return Err(format!(
                "Metal readback violated premultiplied alpha: bgra={pixel:?}"
            ));
        }
        Ok(())
    }

    pub fn validate_creation_cycles(
        cycles: u32,
        before: ResourceCounts,
        after: ResourceCounts,
    ) -> Result<CycleReport, String> {
        if cycles == 0 {
            return Err("overlay cycle count must be greater than zero".into());
        }
        if after.windows != before.windows {
            return Err(format!(
                "NSApplication window count changed from {} to {} after {cycles} cycles",
                before.windows, after.windows
            ));
        }
        if after.owners != before.owners {
            return Err(format!(
                "live overlay owner count changed from {} to {} after {cycles} cycles",
                before.owners, after.owners
            ));
        }

        Ok(CycleReport {
            cycles,
            windows_before: before.windows,
            windows_after: after.windows,
            owners_before: before.owners,
            owners_after: after.owners,
        })
    }

    pub fn resource_counts(mtm: MainThreadMarker) -> ResourceCounts {
        let application = NSApplication::sharedApplication(mtm);
        ResourceCounts {
            windows: application.windows().count(),
            owners: LIVE_OVERLAY_OWNERS.load(Ordering::Acquire),
        }
    }

    pub fn run_creation_cycle(mtm: MainThreadMarker) -> Result<(), String> {
        autoreleasepool(|_| {
            let overlay = NativeOverlay::create_with_logging(mtm, false)?;
            overlay.show();
            overlay.clear_present_and_wait()?;
            overlay.hide();
            drop(overlay);
            Ok(())
        })
    }

    #[cfg(test)]
    mod tests {
        use super::{OVERLAY_VERTICES, OverlayVertex, ResourceCounts, validate_creation_cycles};

        #[test]
        fn vertex_layout_matches_metal_float4_alignment() {
            assert_eq!(size_of::<OverlayVertex>(), 32);
            for vertex in OVERLAY_VERTICES {
                let [red, green, blue, alpha] = vertex.premultiplied_color;
                assert!(alpha > 0.0);
                assert!(red <= alpha && green <= alpha && blue <= alpha);
            }
        }

        #[test]
        fn accepts_stable_window_and_owner_counts() {
            let counts = ResourceCounts {
                windows: 1,
                owners: 0,
            };
            let report = validate_creation_cycles(100, counts, counts).unwrap();

            assert_eq!(report.cycles, 100);
            assert_eq!(report.windows_before, report.windows_after);
            assert_eq!(report.owners_before, report.owners_after);
        }

        #[test]
        fn rejects_window_or_owner_growth() {
            let before = ResourceCounts {
                windows: 1,
                owners: 0,
            };

            let window_error = validate_creation_cycles(
                100,
                before,
                ResourceCounts {
                    windows: 2,
                    owners: 0,
                },
            )
            .unwrap_err();
            assert!(window_error.contains("window count changed from 1 to 2"));

            let owner_error = validate_creation_cycles(
                100,
                before,
                ResourceCounts {
                    windows: 1,
                    owners: 1,
                },
            )
            .unwrap_err();
            assert!(owner_error.contains("owner count changed from 0 to 1"));
        }

        #[test]
        fn rejects_zero_cycles() {
            let counts = ResourceCounts {
                windows: 0,
                owners: 0,
            };
            assert_eq!(
                validate_creation_cycles(0, counts, counts).unwrap_err(),
                "overlay cycle count must be greater than zero"
            );
        }
    }
}

#[cfg(target_os = "windows")]
use bongocat_overlay_windows_spike as windows_overlay;

use gpui::{
    App, Application, Bounds, Context, Global, Render, Timer, Window, WindowBounds, WindowOptions,
    div, prelude::*, px, rgb, size,
};
use std::time::Duration;

#[cfg(target_os = "macos")]
type PlatformOverlay = macos_overlay::NativeOverlay;

#[cfg(target_os = "windows")]
type PlatformOverlay = windows_overlay::NativeOverlay;

#[cfg(any(target_os = "macos", target_os = "windows"))]
struct OverlayGlobal {
    overlay: Option<PlatformOverlay>,
    frame_source: FrameSourceState,
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
#[derive(Default)]
struct FrameSourceState {
    started: bool,
    running: bool,
    stopped: bool,
    frames: u64,
    resize_completed: bool,
    failures: u32,
    recoveries: u32,
    recovery_attempts: u8,
    retry_ticks_remaining: u8,
    recovery_disabled: bool,
    injected_failure: bool,
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FrameFailureKind {
    SurfaceUnavailable,
    DeviceLost,
    Fatal,
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
impl FrameFailureKind {
    fn label(self) -> &'static str {
        match self {
            Self::SurfaceUnavailable => "surface_unavailable",
            Self::DeviceLost => "device_lost",
            Self::Fatal => "fatal",
        }
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
struct FrameFailure {
    kind: FrameFailureKind,
    message: String,
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
struct FrameTickOutcome {
    keep_running: bool,
    status: Option<String>,
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
struct OverlayGlobal {}

impl Global for OverlayGlobal {}

struct SettingsProbe {
    overlay_status: String,
}

impl Render for SettingsProbe {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .bg(rgb(0x25282e))
            .child(self.overlay_status.clone())
    }
}

fn main() {
    #[cfg(target_os = "windows")]
    if let Some(cycles) =
        argument_value("--windows-overlay-cycles").and_then(|value| value.parse::<u32>().ok())
    {
        let report = windows_overlay::run_creation_cycles(cycles)
            .expect("Windows overlay create/destroy cycle smoke failed");
        println!(
            "gpui-overlay-spike: Windows cycles={} non_empty_frames={} handles_before={} handles_after={} clean_shutdown=true",
            report.cycles, report.non_empty_frames, report.handles_before, report.handles_after
        );
        return;
    }

    #[cfg(target_os = "macos")]
    if let Some(cycles) =
        argument_value("--macos-overlay-cycles").and_then(|value| value.parse::<u32>().ok())
    {
        run_macos_cycle_subprocess(cycles)
            .expect("macOS overlay create/destroy cycle smoke failed");
        return;
    }

    #[cfg(target_os = "macos")]
    let macos_overlay_cycles =
        argument_value("--macos-overlay-cycle-worker").and_then(|value| value.parse::<u32>().ok());

    let auto_quit_ms = argument_value("--auto-quit-ms").and_then(|value| value.parse::<u64>().ok());
    let simulate_failure = has_argument("--simulate-overlay-init-failure");
    let simulate_macos_drawable_unavailable = has_argument("--simulate-macos-drawable-unavailable");
    let simulate_renderer_loss_at_frame = argument_value("--simulate-renderer-loss-at-frame")
        .and_then(|value| value.parse::<u64>().ok());

    Application::new().run(move |cx: &mut App| {
        #[cfg(target_os = "macos")]
        if let Some(cycles) = macos_overlay_cycles {
            cx.spawn(async move |cx| {
                let result = async {
                    if cycles == 0 {
                        return Err("overlay cycle count must be greater than zero".to_string());
                    }

                    // Warm up process-global AppKit and Metal state, then let
                    // AppKit process the close before capturing the baseline.
                    cx.update(|_| {
                        let mtm = objc2::MainThreadMarker::new()
                            .expect("GPUI must run on AppKit main thread");
                        macos_overlay::run_creation_cycle(mtm)
                    })
                    .map_err(|error| format!("GPUI cycle task stopped: {error}"))??;
                    Timer::after(Duration::from_millis(10)).await;
                    let before = cx
                        .update(|_| {
                            let mtm = objc2::MainThreadMarker::new()
                                .expect("GPUI must run on AppKit main thread");
                            macos_overlay::resource_counts(mtm)
                        })
                        .map_err(|error| format!("GPUI cycle task stopped: {error}"))?;

                    for _ in 0..cycles {
                        cx.update(|_| {
                            let mtm = objc2::MainThreadMarker::new()
                                .expect("GPUI must run on AppKit main thread");
                            macos_overlay::run_creation_cycle(mtm)
                        })
                        .map_err(|error| format!("GPUI cycle task stopped: {error}"))??;
                        Timer::after(Duration::from_millis(1)).await;
                    }

                    Timer::after(Duration::from_millis(10)).await;
                    let after = cx
                        .update(|_| {
                            let mtm = objc2::MainThreadMarker::new()
                                .expect("GPUI must run on AppKit main thread");
                            macos_overlay::resource_counts(mtm)
                        })
                        .map_err(|error| format!("GPUI cycle task stopped: {error}"))?;
                    macos_overlay::validate_creation_cycles(cycles, before, after)
                }
                .await;

                match result {
                    Ok(report) => {
                        println!(
                            "gpui-overlay-spike: macOS cycles={} non_empty_frames={} windows_before={} windows_after={} owners_before={} owners_after={} clean_shutdown=true",
                            report.cycles,
                            report.cycles,
                            report.windows_before,
                            report.windows_after,
                            report.owners_before,
                            report.owners_after
                        );
                        cx.update(|cx| cx.quit()).ok();
                    }
                    Err(error) => {
                        eprintln!(
                            "gpui-overlay-spike: macOS overlay create/destroy cycle smoke failed: {error}"
                        );
                        cx.update(|cx| cx.quit()).ok();
                    }
                }
            })
            .detach();
            return;
        }

        let mut overlay_status = "GPUI settings + native overlay".to_string();

        #[cfg(any(target_os = "macos", target_os = "windows"))]
        let mut run_visibility_smoke = false;

        #[cfg(any(target_os = "macos", target_os = "windows"))]
        {
            let overlay = if simulate_failure {
                Err("simulated overlay initialization failure".to_string())
            } else {
                create_platform_overlay()
            };
            match overlay {
                Ok(overlay) => {
                    match initial_show_and_present(
                        &overlay,
                        simulate_macos_drawable_unavailable,
                    ) {
                        Ok(()) => {
                            run_visibility_smoke = true;
                            #[cfg(target_os = "windows")]
                            println!(
                                "gpui-overlay-spike: Windows native overlay created driver={} dpi={}",
                                overlay.driver_name(),
                                overlay.dpi()
                            );
                            #[cfg(target_os = "macos")]
                            println!("gpui-overlay-spike: macOS native overlay created");
                        }
                        Err(error) => {
                            overlay_status = format!("Overlay unavailable: {error}");
                            println!("gpui-overlay-spike: overlay degraded error={error}");
                        }
                    }
                    cx.set_global(OverlayGlobal {
                        overlay: Some(overlay),
                        frame_source: FrameSourceState::default(),
                    });
                }
                Err(error) => {
                    overlay_status = format!("Overlay unavailable: {error}");
                    cx.set_global(OverlayGlobal {
                        overlay: None,
                        frame_source: FrameSourceState {
                            stopped: true,
                            ..Default::default()
                        },
                    });
                    println!("gpui-overlay-spike: overlay degraded error={error}");
                }
            }
        }

        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        cx.set_global(OverlayGlobal {});

        let settings_window = cx
            .open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    size(px(520.0), px(320.0)),
                    cx,
                ))),
                titlebar: Some(gpui::TitlebarOptions::default()),
                ..Default::default()
            },
            |_, cx| {
                cx.new(|_| SettingsProbe {
                    overlay_status: overlay_status.clone(),
                })
            },
        )
            .expect("open GPUI settings window");
        println!("gpui-overlay-spike: GPUI settings window opened");
        cx.activate(true);

        #[cfg(any(target_os = "macos", target_os = "windows"))]
        if run_visibility_smoke {
            {
                let frame_source = &mut cx.global_mut::<OverlayGlobal>().frame_source;
                frame_source.started = true;
                frame_source.running = true;
            }
            println!("gpui-overlay-spike: frame source started target_hz=60");
            cx.spawn(async move |cx| {
                loop {
                    Timer::after(Duration::from_millis(16)).await;
                    let outcome = cx
                        .update(|cx| {
                            tick_frame_source(
                                cx.global_mut::<OverlayGlobal>(),
                                simulate_renderer_loss_at_frame,
                            )
                        })
                        .unwrap_or(FrameTickOutcome {
                            keep_running: false,
                            status: None,
                        });
                    if let Some(status) = outcome.status {
                        settings_window
                            .update(cx, |settings, _, cx| {
                                settings.overlay_status = status;
                                println!(
                                    "gpui-overlay-spike: settings overlay status={}",
                                    settings.overlay_status
                                );
                                cx.notify();
                            })
                            .ok();
                    }
                    if !outcome.keep_running {
                        break;
                    }
                }
            })
            .detach();

            cx.spawn(async move |cx| {
                Timer::after(Duration::from_millis(300)).await;
                cx.update(|cx| {
                    let global = cx.global_mut::<OverlayGlobal>();
                    if let Some(overlay) = &global.overlay {
                        hide_platform_overlay(overlay).expect("hide native overlay");
                    }
                })
                .ok();
                Timer::after(Duration::from_millis(300)).await;
                cx.update(|cx| {
                    let global = cx.global_mut::<OverlayGlobal>();
                    if let Some(overlay) = &global.overlay {
                        show_and_present(overlay)
                            .expect("reshow and clear/present native overlay");
                    }
                })
                .ok();
            })
            .detach();
        }

        if let Some(milliseconds) = auto_quit_ms {
            println!("gpui-overlay-spike: auto quit scheduled milliseconds={milliseconds}");
            cx.spawn(async move |cx| {
                Timer::after(Duration::from_millis(milliseconds)).await;
                cx.update(|cx| {
                    println!("gpui-overlay-spike: auto quit requested");
                    #[cfg(any(target_os = "macos", target_os = "windows"))]
                    {
                        cx.global_mut::<OverlayGlobal>().frame_source.running = false;
                    }
                })
                .ok();

                #[cfg(any(target_os = "macos", target_os = "windows"))]
                for _ in 0..20 {
                    Timer::after(Duration::from_millis(10)).await;
                    let stopped = cx
                        .update(|cx| {
                            let frame_source = &cx.global::<OverlayGlobal>().frame_source;
                            !frame_source.started || frame_source.stopped
                        })
                        .unwrap_or(true);
                    if stopped {
                        break;
                    }
                }

                cx.update(|cx| {
                    #[cfg(any(target_os = "macos", target_os = "windows"))]
                    {
                        let global = cx.global_mut::<OverlayGlobal>();
                        assert!(
                            !global.frame_source.started || global.frame_source.stopped,
                            "frame source did not stop before overlay teardown"
                        );
                        if global.frame_source.started {
                            assert!(
                                global.frame_source.frames > 0,
                                "frame source stopped without rendering"
                            );
                            assert!(
                                global.frame_source.resize_completed,
                                "frame source stopped before resize completed"
                            );
                        }
                        let overlay = global.overlay.take();
                        drop(overlay);
                    }
                    cx.quit();
                })
                .ok();
            })
            .detach();
        }
    });
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn tick_frame_source(
    global: &mut OverlayGlobal,
    simulate_renderer_loss_at_frame: Option<u64>,
) -> FrameTickOutcome {
    if !global.frame_source.running {
        global.frame_source.stopped = true;
        println!(
            "gpui-overlay-spike: frame source stopped frames={} resize_completed={} failures={} recoveries={}",
            global.frame_source.frames,
            global.frame_source.resize_completed,
            global.frame_source.failures,
            global.frame_source.recoveries
        );
        return FrameTickOutcome {
            keep_running: false,
            status: None,
        };
    }

    if global.frame_source.recovery_disabled {
        global.frame_source.running = false;
        return FrameTickOutcome {
            keep_running: true,
            status: None,
        };
    }

    if global.overlay.is_none() {
        return try_recover_overlay(global);
    }

    let should_inject = simulate_renderer_loss_at_frame
        .is_some_and(|frame| frame == global.frame_source.frames)
        && !global.frame_source.injected_failure;
    let render_result = if should_inject {
        global.frame_source.injected_failure = true;
        Err(FrameFailure {
            kind: FrameFailureKind::DeviceLost,
            message: "simulated renderer device loss".into(),
        })
    } else {
        let overlay = global
            .overlay
            .as_mut()
            .expect("checked overlay presence before frame submission");
        if global.frame_source.frames == 8 && !global.frame_source.resize_completed {
            match resize_platform_overlay(overlay, 400, 300) {
                Ok(()) => global.frame_source.resize_completed = true,
                Err(message) => {
                    return enter_frame_recovery(
                        global,
                        FrameFailure {
                            kind: FrameFailureKind::SurfaceUnavailable,
                            message,
                        },
                    );
                }
            }
        }
        render_platform_frame(overlay)
    };

    match render_result {
        Ok(()) => {
            global.frame_source.frames += 1;
            FrameTickOutcome {
                keep_running: true,
                status: None,
            }
        }
        Err(failure) => enter_frame_recovery(global, failure),
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn enter_frame_recovery(global: &mut OverlayGlobal, failure: FrameFailure) -> FrameTickOutcome {
    global.frame_source.failures += 1;
    global.frame_source.recovery_attempts = 0;
    global.frame_source.retry_ticks_remaining = 3;
    let kind = failure.kind;
    let message = failure.message;
    println!(
        "gpui-overlay-spike: frame degraded kind={} error={message}",
        kind.label()
    );
    drop(global.overlay.take());

    if kind == FrameFailureKind::Fatal {
        global.frame_source.recovery_disabled = true;
        return FrameTickOutcome {
            keep_running: true,
            status: Some(format!("Overlay unavailable: {message}")),
        };
    }

    FrameTickOutcome {
        keep_running: true,
        status: Some(format!("Overlay recovering: {message}")),
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn try_recover_overlay(global: &mut OverlayGlobal) -> FrameTickOutcome {
    if global.frame_source.retry_ticks_remaining > 0 {
        global.frame_source.retry_ticks_remaining -= 1;
        return FrameTickOutcome {
            keep_running: true,
            status: None,
        };
    }

    global.frame_source.recovery_attempts += 1;
    let attempt = global.frame_source.recovery_attempts;
    let recovered = create_platform_overlay().and_then(|mut overlay| {
        if global.frame_source.resize_completed {
            resize_platform_overlay(&mut overlay, 400, 300)?;
        }
        initial_show_and_present(&overlay, false)?;
        Ok(overlay)
    });

    match recovered {
        Ok(overlay) => {
            global.overlay = Some(overlay);
            global.frame_source.recoveries += 1;
            global.frame_source.recovery_attempts = 0;
            println!("gpui-overlay-spike: frame renderer recovered attempt={attempt}");
            FrameTickOutcome {
                keep_running: true,
                status: Some("GPUI settings + native overlay (recovered)".into()),
            }
        }
        Err(error) if attempt < 3 => {
            global.frame_source.retry_ticks_remaining = 3_u8.saturating_mul(1 << attempt);
            println!(
                "gpui-overlay-spike: frame renderer recovery deferred attempt={attempt} error={error}"
            );
            FrameTickOutcome {
                keep_running: true,
                status: Some(format!("Overlay recovering: {error}")),
            }
        }
        Err(error) => {
            global.frame_source.recovery_disabled = true;
            println!(
                "gpui-overlay-spike: frame renderer recovery exhausted attempts={attempt} error={error}"
            );
            FrameTickOutcome {
                keep_running: true,
                status: Some(format!("Overlay unavailable: {error}")),
            }
        }
    }
}

#[cfg(target_os = "macos")]
fn run_macos_cycle_subprocess(cycles: u32) -> Result<(), String> {
    let executable =
        std::env::current_exe().map_err(|error| format!("resolve current executable: {error}"))?;
    let output = std::process::Command::new(executable)
        .args(["--macos-overlay-cycle-worker", &cycles.to_string()])
        .output()
        .map_err(|error| format!("start macOS overlay cycle worker: {error}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    print!("{stdout}");
    eprint!("{stderr}");

    if !output.status.success() {
        return Err(format!(
            "macOS overlay cycle worker exited with {}",
            output.status
        ));
    }
    let success = format!("macOS cycles={cycles}");
    if !stdout.contains(&success) || !stdout.contains("clean_shutdown=true") {
        return Err("macOS overlay cycle worker did not report clean shutdown".into());
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn create_platform_overlay() -> Result<PlatformOverlay, String> {
    let mtm = objc2::MainThreadMarker::new().expect("GPUI must run on AppKit main thread");
    macos_overlay::NativeOverlay::create(mtm)
}

#[cfg(target_os = "windows")]
fn create_platform_overlay() -> Result<PlatformOverlay, String> {
    windows_overlay::NativeOverlay::create()
}

#[cfg(target_os = "macos")]
fn initial_show_and_present(
    overlay: &PlatformOverlay,
    simulate_drawable_unavailable: bool,
) -> Result<(), String> {
    overlay.show();
    if simulate_drawable_unavailable {
        overlay.clear_present_simulating_unavailable()
    } else {
        overlay.clear_present()
    }
}

#[cfg(target_os = "windows")]
fn initial_show_and_present(
    overlay: &PlatformOverlay,
    _simulate_drawable_unavailable: bool,
) -> Result<(), String> {
    show_and_present(overlay)
}

#[cfg(target_os = "macos")]
fn show_and_present(overlay: &PlatformOverlay) -> Result<(), String> {
    overlay.show();
    overlay.clear_present()
}

#[cfg(target_os = "windows")]
fn show_and_present(overlay: &PlatformOverlay) -> Result<(), String> {
    overlay.show()?;
    overlay.clear_present()
}

#[cfg(target_os = "macos")]
fn hide_platform_overlay(overlay: &PlatformOverlay) -> Result<(), String> {
    overlay.hide();
    Ok(())
}

#[cfg(target_os = "windows")]
fn hide_platform_overlay(overlay: &PlatformOverlay) -> Result<(), String> {
    overlay.hide()
}

#[cfg(target_os = "macos")]
fn resize_platform_overlay(
    overlay: &mut PlatformOverlay,
    width: u32,
    height: u32,
) -> Result<(), String> {
    overlay.resize(width, height)
}

#[cfg(target_os = "windows")]
fn resize_platform_overlay(
    overlay: &mut PlatformOverlay,
    width: u32,
    height: u32,
) -> Result<(), String> {
    overlay.resize(width, height)
}

#[cfg(target_os = "macos")]
fn render_platform_frame(overlay: &PlatformOverlay) -> Result<(), FrameFailure> {
    overlay
        .render_scheduled_frame()
        .map_err(|message| FrameFailure {
            kind: FrameFailureKind::SurfaceUnavailable,
            message,
        })
}

#[cfg(target_os = "windows")]
fn render_platform_frame(overlay: &PlatformOverlay) -> Result<(), FrameFailure> {
    overlay.render_scheduled_frame().map_err(|error| {
        let kind = match error.kind() {
            windows_overlay::RenderFailureKind::SurfaceUnavailable => {
                FrameFailureKind::SurfaceUnavailable
            }
            windows_overlay::RenderFailureKind::DeviceLost => FrameFailureKind::DeviceLost,
            windows_overlay::RenderFailureKind::Fatal => FrameFailureKind::Fatal,
        };
        FrameFailure {
            kind,
            message: error.to_string(),
        }
    })
}

fn argument_value(name: &str) -> Option<String> {
    let mut arguments = std::env::args().skip(1);
    while let Some(argument) = arguments.next() {
        if argument == name {
            return arguments.next();
        }
    }
    None
}

fn has_argument(name: &str) -> bool {
    std::env::args().skip(1).any(|argument| argument == name)
}
