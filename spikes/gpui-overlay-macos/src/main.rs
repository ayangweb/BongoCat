#[cfg(target_os = "macos")]
mod macos_overlay {
    use metal::{
        CommandQueue, Device, MTLClearColor, MTLLoadAction, MTLPixelFormat, MTLStoreAction,
        MetalLayer, RenderPassDescriptor,
    };
    use objc2::{MainThreadMarker, MainThreadOnly, rc::Retained};
    use objc2_app_kit::{
        NSBackingStoreType, NSColor, NSFloatingWindowLevel, NSPanel, NSView,
        NSWindowCollectionBehavior, NSWindowStyleMask,
    };
    use objc2_foundation::{NSPoint, NSRect, NSSize};
    use std::mem;

    pub struct NativeOverlay {
        panel: Retained<NSPanel>,
        layer: MetalLayer,
        queue: CommandQueue,
    }

    impl NativeOverlay {
        pub fn create(mtm: MainThreadMarker) -> Result<Self, String> {
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

            Ok(Self {
                panel,
                layer,
                queue: device.new_command_queue(),
            })
        }

        pub fn show(&self) {
            self.panel.orderFrontRegardless();
            println!("gpui-overlay-macos-spike: overlay shown");
        }

        pub fn hide(&self) {
            self.panel.orderOut(None);
            println!("gpui-overlay-macos-spike: overlay hidden");
        }

        pub fn clear_present(&self) -> Result<(), String> {
            let drawable = self
                .layer
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
            let command_buffer = self.queue.new_command_buffer();
            let encoder = command_buffer.new_render_command_encoder(pass);
            encoder.end_encoding();
            command_buffer.present_drawable(drawable);
            command_buffer.commit();
            println!("gpui-overlay-macos-spike: transparent clear/present submitted");
            Ok(())
        }
    }

    impl Drop for NativeOverlay {
        fn drop(&mut self) {
            self.panel.orderOut(None);
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

    let auto_quit_ms = argument_value("--auto-quit-ms").and_then(|value| value.parse::<u64>().ok());
    let simulate_failure = has_argument("--simulate-overlay-init-failure");

    Application::new().run(move |cx: &mut App| {
        let mut overlay_status = "GPUI settings + native overlay".to_string();

        #[cfg(any(target_os = "macos", target_os = "windows"))]
        {
            let overlay = if simulate_failure {
                Err("simulated overlay initialization failure".to_string())
            } else {
                create_platform_overlay()
            };
            match overlay {
                Ok(overlay) => {
                    show_and_present(&overlay).expect("show and clear/present native overlay");
                    #[cfg(target_os = "windows")]
                    println!(
                        "gpui-overlay-spike: Windows native overlay created driver={} dpi={}",
                        overlay.driver_name(),
                        overlay.dpi()
                    );
                    #[cfg(target_os = "macos")]
                    println!("gpui-overlay-spike: macOS native overlay created");
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
                    show_and_present(overlay).expect("reshow and clear/present native overlay");
                }
            })
            .ok();
        })
        .detach();

        if let Some(milliseconds) = auto_quit_ms {
            println!("gpui-overlay-spike: auto quit scheduled milliseconds={milliseconds}");
            cx.spawn(async move |cx| {
                Timer::after(Duration::from_millis(milliseconds)).await;
                cx.update(|cx| {
                    println!("gpui-overlay-spike: auto quit requested");
                    cx.quit();
                })
                .ok();
            })
            .detach();
        }
    });
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
