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

use gpui::{
    App, Application, Bounds, Context, Global, Render, Timer, Window, WindowBounds, WindowOptions,
    div, prelude::*, px, rgb, size,
};
use std::time::Duration;

#[cfg(target_os = "macos")]
struct OverlayGlobal {
    overlay: macos_overlay::NativeOverlay,
}

#[cfg(not(target_os = "macos"))]
struct OverlayGlobal {}

impl Global for OverlayGlobal {}

struct SettingsProbe;

impl Render for SettingsProbe {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .bg(rgb(0x25282e))
            .child("GPUI settings + native overlay")
    }
}

fn main() {
    Application::new().run(|cx: &mut App| {
        #[cfg(target_os = "macos")]
        {
            let mtm = objc2::MainThreadMarker::new().expect("GPUI must run on AppKit main thread");
            let overlay = macos_overlay::NativeOverlay::create(mtm).expect("create overlay");
            overlay.show();
            overlay.clear_present().expect("clear/present overlay");
            cx.set_global(OverlayGlobal { overlay });
            println!("gpui-overlay-macos-spike: native overlay created");
        }
        #[cfg(not(target_os = "macos"))]
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
            |_, cx| cx.new(|_| SettingsProbe),
        )
        .expect("open GPUI settings window");

        #[cfg(target_os = "macos")]
        cx.spawn(async move |cx| {
            Timer::after(Duration::from_millis(300)).await;
            cx.update(|cx| {
                let global = cx.global_mut::<OverlayGlobal>();
                global.overlay.hide();
            })
            .ok();
            Timer::after(Duration::from_millis(300)).await;
            cx.update(|cx| {
                let global = cx.global_mut::<OverlayGlobal>();
                global.overlay.show();
                global.overlay.clear_present().ok();
            })
            .ok();
        })
        .detach();

        if let Some(milliseconds) = std::env::args()
            .skip(1)
            .position(|arg| arg == "--auto-quit-ms")
            .and_then(|index| std::env::args().nth(index + 2))
            .and_then(|value| value.parse::<u64>().ok())
        {
            cx.spawn(async move |cx| {
                Timer::after(Duration::from_millis(milliseconds)).await;
                cx.update(|cx| cx.quit()).ok();
            })
            .detach();
        }
    });
}
