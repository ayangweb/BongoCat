#[cfg(target_os = "macos")]
mod macos_overlay {
    use metal::{
        CommandQueue, Device, MTLClearColor, MTLCommandBufferStatus, MTLLoadAction, MTLPixelFormat,
        MTLStoreAction, MetalLayer, RenderPassDescriptor,
    };
    use objc2::{
        MainThreadMarker, MainThreadOnly,
        rc::{Retained, autoreleasepool},
    };
    use objc2_app_kit::{
        NSApplication, NSBackingStoreType, NSColor, NSFloatingWindowLevel, NSPanel, NSView,
        NSWindowCollectionBehavior, NSWindowStyleMask,
    };
    use objc2_foundation::{NSPoint, NSRect, NSSize};
    use std::{
        mem,
        sync::atomic::{AtomicUsize, Ordering},
    };

    static LIVE_OVERLAY_OWNERS: AtomicUsize = AtomicUsize::new(0);

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
            layer.set_drawable_size(core_graphics_types::geometry::CGSize::new(320.0, 240.0));
            // SAFETY: metal::MetalLayerRef and objc2_quartz_core::CALayer are
            // the same Objective-C CAMetalLayer object; the view retains it.
            let layer_ref = unsafe {
                mem::transmute::<&metal::MetalLayerRef, &objc2_quartz_core::CALayer>(layer.as_ref())
            };
            view.setLayer(Some(layer_ref));
            panel.setContentView(Some(&view));

            let overlay = Self {
                panel: mem::ManuallyDrop::new(panel),
                layer: Some(layer),
                queue: Some(device.new_command_queue()),
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
            self.clear_present_with_behavior(PresentBehavior::Submit)
        }

        pub fn clear_present_simulating_unavailable(&self) -> Result<(), String> {
            self.clear_present_with_behavior(PresentBehavior::SimulateDrawableUnavailable)
        }

        fn clear_present_and_wait(&self) -> Result<(), String> {
            self.clear_present_with_behavior(PresentBehavior::SubmitAndWait)
        }

        fn clear_present_with_behavior(&self, behavior: PresentBehavior) -> Result<(), String> {
            if matches!(behavior, PresentBehavior::SimulateDrawableUnavailable) {
                return Err("simulated CAMetalLayer drawable unavailable".into());
            }
            let drawable = self
                .layer
                .as_ref()
                .expect("live overlay must own a Metal layer")
                .next_drawable()
                .ok_or("CAMetalLayer returned no drawable")?;
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
            }
            if self.log_lifecycle {
                println!("gpui-overlay-macos-spike: transparent clear/present submitted");
            }
            Ok(())
        }
    }

    impl Drop for NativeOverlay {
        fn drop(&mut self) {
            self.panel.setContentView(None);
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
        use super::{ResourceCounts, validate_creation_cycles};

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
            "gpui-overlay-spike: Windows cycles={} handles_before={} handles_after={} clean_shutdown=true",
            report.cycles, report.handles_before, report.handles_after
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
                            "gpui-overlay-spike: macOS cycles={} windows_before={} windows_after={} owners_before={} owners_after={} clean_shutdown=true",
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
                        Err(error) if simulate_macos_drawable_unavailable => {
                            overlay_status = format!("Overlay unavailable: {error}");
                            println!("gpui-overlay-spike: overlay degraded error={error}");
                        }
                        Err(error) => panic!("show and clear/present native overlay: {error}"),
                    }
                    cx.set_global(OverlayGlobal {
                        overlay: Some(overlay),
                    });
                }
                Err(error) if simulate_failure => {
                    overlay_status = format!("Overlay unavailable: {error}");
                    cx.set_global(OverlayGlobal { overlay: None });
                    println!("gpui-overlay-spike: overlay degraded error={error}");
                }
                Err(error) => panic!("create native overlay: {error}"),
            }
        }

        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        cx.set_global(OverlayGlobal {});

        cx.open_window(
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
                        let overlay = cx.global_mut::<OverlayGlobal>().overlay.take();
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
