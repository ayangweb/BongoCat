use super::*;
use windows::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, GetPixel,
    ReleaseDC, SRCCOPY, SelectObject,
};
use windows::Win32::UI::WindowsAndMessaging::{
    IsWindow, SW_MINIMIZE, SW_RESTORE, SendMessageW, WMSZ_BOTTOMRIGHT, WMSZ_RIGHT,
};

#[test]
fn window_mode_captures_opaque_picture_and_keeps_hwnd_on_model_switch() {
    let catalog = PresetModelCatalog::open(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../resources/models")
            .as_path(),
        ModelPackageLimits::default(),
    )
    .expect("preset catalog");
    let (runtime, consumer) = RuntimeOwner::start_with_rendering(true, 64);
    let client = runtime.client();
    client
        .wait_for_revision(1, RUNTIME_TIMEOUT)
        .expect("runtime ready");
    let model = catalog.load(&ModelId::parse("standard").unwrap()).unwrap();
    let (token, frame) = prepare_switch_frame(
        &client,
        &consumer,
        Arc::new(model),
        Arc::new(preview_input_bindings("standard")),
    )
    .unwrap();
    let apartment = ComApartment::initialize().unwrap();
    let options = OverlaySessionOptions {
        window_mode: true,
        window_background_color: [0, 255, 0],
        click_through: true,
        hide_on_pointer_hover: true,
        hide_on_idle: true,
        opacity_percent: 20,
        corner_radius_percent: 50,
        ..OverlaySessionOptions::default()
    };
    // The saved client geometry from the reported window-mode regression.
    // Its height is stale; it must follow this model's aspect before the first
    // swap chain is created, rather than introducing side margins on restore.
    let restored = OverlayWindowBounds::new(100, 100, 720, 308);
    let mut overlay = NativeOverlay::create(&frame, options, Some(restored), None, None)
        .expect("window renderer");
    let hwnd = overlay.window.hwnd;
    assert!(overlay.renderer.composition.is_none());
    assert_eq!(overlay.renderer.corner_radius_percent, 0);
    assert!(overlay.window.taskbar_icon_is_visible());
    overlay.window.set_taskbar_icon_visible(false).unwrap();
    overlay.window.set_click_through(true).unwrap();
    // SAFETY: every HWND operation stays on this test's owning thread.
    let (style, extended) = unsafe {
        (
            GetWindowLongPtrW(hwnd, GWL_STYLE),
            GetWindowLongPtrW(hwnd, GWL_EXSTYLE),
        )
    };
    assert_eq!(
        style as u32 & WS_OVERLAPPEDWINDOW.0,
        (WS_OVERLAPPEDWINDOW & !WS_MAXIMIZEBOX).0
    );
    assert_eq!(
        extended as u32
            & (WS_EX_LAYERED
                | WS_EX_TRANSPARENT
                | WS_EX_NOACTIVATE
                | WS_EX_NOREDIRECTIONBITMAP
                | WS_EX_TOOLWINDOW)
                .0,
        0
    );
    let bounds = overlay.window.bounds().unwrap();
    assert_eq!(bounds.width, restored.width);
    assert_eq!(
        bounds.height,
        crate::model_window_height_for_width(frame.snapshot.canvas, bounds.width)
    );
    assert_eq!(
        (bounds.width, bounds.height),
        (overlay.renderer.width, overlay.renderer.height)
    );
    verify_window_frame_and_resize(hwnd, overlay.window._state.sizing);
    overlay.draw_capturing(true).expect("opaque model readback");
    report_model_commit(&client, &consumer, token, ModelCommitOutcome::Prepared).unwrap();
    overlay.draw(false).unwrap();
    overlay.set_visible(true).unwrap();
    for _ in 0..8 {
        pump_window_messages();
        overlay.draw(false).unwrap();
        thread::sleep(Duration::from_millis(20));
    }
    let rgb = capture_bitblt(hwnd, bounds.width, bounds.height);
    assert!(
        rgb.chunks_exact(3).any(|pixel| pixel == [0, 255, 0]),
        "BitBlt includes key background"
    );
    assert!(
        rgb.chunks_exact(3)
            .map(|pixel| [pixel[0], pixel[1], pixel[2]])
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            > 2,
        "BitBlt includes model picture"
    );
    let output = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/window-mode-smoke.png");
    image::save_buffer(
        output,
        &rgb,
        bounds.width,
        bounds.height,
        image::ColorType::Rgb8,
    )
    .unwrap();

    // Optional bounded observer interval for external OBS acceptance checks.
    let observer_seconds = std::env::var("BONGOCAT_WINDOW_CAPTURE_SECONDS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(0)
        .min(120);
    let observed = Instant::now();
    while observed.elapsed() < Duration::from_secs(observer_seconds) {
        pump_window_messages();
        if let Err(error) = overlay.draw(false) {
            assert!(error.is_temporary_presentation_unavailable(), "{error}");
        }
        thread::sleep(Duration::from_millis(16));
    }

    let changed = overlay
        .window
        ._state
        .sizing
        .normalize(OverlayWindowBounds::new(
            bounds.x,
            bounds.y,
            bounds.width + 70,
            bounds.height + 70,
        ));
    overlay.resize(changed).unwrap();
    assert_eq!(overlay.window.bounds().unwrap(), changed);
    // SAFETY: user32 dispatch and visibility changes are synchronous on the owner thread.
    unsafe {
        let _ = ShowWindow(hwnd, SW_MINIMIZE);
    }
    pump_window_messages();
    assert_eq!(
        overlay.window.bounds().unwrap(),
        changed,
        "minimize preserves saved client geometry"
    );
    let changed = overlay.window._state.sizing.normalize(OverlayWindowBounds {
        width: changed.width + 20,
        height: changed.height + 20,
        ..changed
    });
    overlay.resize(changed).unwrap();
    assert_eq!(overlay.window.bounds().unwrap(), changed);
    unsafe {
        let _ = ShowWindow(hwnd, SW_RESTORE);
    }
    pump_window_messages();
    assert_eq!(overlay.window.bounds().unwrap(), changed);
    overlay.set_visible(false).unwrap();
    overlay.draw(true).unwrap();
    overlay.set_visible(true).unwrap();
    assert!(overlay.window.is_visible());
    unsafe {
        let _ = ShowWindow(hwnd, SW_MINIMIZE);
    }
    overlay.set_visible(false).unwrap();
    overlay.draw(true).unwrap();
    overlay.set_visible(true).unwrap();
    assert!(!unsafe { IsIconic(hwnd) }.as_bool());
    assert_eq!(overlay.window.bounds().unwrap(), changed);

    for id in ["keyboard", "gamepad", "standard"] {
        let model = catalog.load(&ModelId::parse(id).unwrap()).unwrap();
        let (token, frame) = prepare_switch_frame(
            &client,
            &consumer,
            Arc::new(model),
            Arc::new(preview_input_bindings(id)),
        )
        .unwrap();
        let mut bad_resources = frame.resources.as_ref().clone();
        bad_resources.textures[0].path =
            Path::new(".missing-window-mode-texture.png").to_path_buf();
        let invalid = RenderFrame {
            resources: Arc::new(bad_resources),
            ..frame.clone()
        };
        let generation = overlay.renderer.model_generation;
        assert!(overlay.renderer.sync_frame(&invalid).is_err());
        assert_eq!(overlay.renderer.model_generation, generation);
        overlay.draw_capturing(true).unwrap();
        assert!(overlay.renderer.sync_frame(&frame).unwrap());
        overlay.resize_for_model(frame.snapshot.canvas).unwrap();
        overlay.draw_capturing(true).unwrap();
        report_model_commit(&client, &consumer, token, ModelCommitOutcome::Prepared).unwrap();
        assert_eq!(
            overlay.window.hwnd, hwnd,
            "capture target survives model switch"
        );
    }
    unsafe {
        SendMessageW(hwnd, WM_CLOSE, None, None);
    }
    assert!(overlay.window._state.close_requested);
    assert!(
        unsafe { IsWindow(Some(hwnd)) }.as_bool(),
        "close must leave GPU owner alive until hidden/shutdown"
    );
    drop(overlay);
    runtime.shutdown(RUNTIME_TIMEOUT).unwrap();
    drop(apartment);
}

fn verify_window_frame_and_resize(hwnd: HWND, sizing: WindowSizing) {
    // SAFETY: all frame measurements and message dispatch use this thread's
    // owned HWND. WM_SIZING receives a writable RECT for the synchronous call.
    unsafe {
        for dpi in [120, 144, 192] {
            let mut rect = RECT {
                right: 500,
                bottom: 300,
                ..Default::default()
            };
            AdjustWindowRectExForDpi(
                &mut rect,
                WS_OVERLAPPEDWINDOW & !WS_MAXIMIZEBOX,
                false,
                WS_EX_APPWINDOW,
                dpi,
            )
            .unwrap();
            assert!(rect.left < 0 && rect.top < 0);
            assert!(rect.right - rect.left > 500 && rect.bottom - rect.top > 300);
        }
        let frame = window_frame_rect(hwnd, 0, 0).unwrap();
        let base = sizing.resize_base(GetDpiForWindow(hwnd)).unwrap();
        for edge in [
            WMSZ_LEFT,
            WMSZ_RIGHT,
            WMSZ_TOP,
            WMSZ_BOTTOM,
            WMSZ_TOPLEFT,
            WMSZ_TOPRIGHT,
            WMSZ_BOTTOMLEFT,
            WMSZ_BOTTOMRIGHT,
        ] {
            let mut rect = RECT {
                left: 100,
                top: 100,
                right: 1100,
                bottom: 700,
            };
            assert_eq!(
                SendMessageW(
                    hwnd,
                    WM_SIZING,
                    Some(WPARAM(edge as usize)),
                    Some(LPARAM((&mut rect as *mut RECT) as isize))
                )
                .0,
                1
            );
            let width = (rect.right - rect.left - frame.right + frame.left) as u32;
            let height = (rect.bottom - rect.top - frame.bottom + frame.top) as u32;
            if matches!(edge, WMSZ_LEFT | WMSZ_TOPLEFT | WMSZ_BOTTOMLEFT) {
                assert_eq!(rect.right, 1100, "right anchor for edge {edge}");
            } else {
                assert_eq!(rect.left, 100, "left anchor for edge {edge}");
            }
            if matches!(edge, WMSZ_TOP | WMSZ_TOPLEFT | WMSZ_TOPRIGHT) {
                assert_eq!(rect.bottom, 700, "bottom anchor for edge {edge}");
            } else {
                assert_eq!(rect.top, 100, "top anchor for edge {edge}");
            }
            assert_eq!(
                (width, height),
                sizing.dimensions_for_scale(
                    GetDpiForWindow(hwnd),
                    base.scale_percent_for_width(width)
                )
            );
        }
        let bounds = client_window_bounds(hwnd).unwrap();
        let point = LPARAM(
            (((bounds.y + 20) as u16 as u32) << 16 | ((bounds.x + 20) as u16 as u32)) as isize,
        );
        assert_eq!(
            SendMessageW(hwnd, WM_NCHITTEST, None, Some(point)).0,
            windows::Win32::UI::WindowsAndMessaging::HTCLIENT as isize
        );
    }
}

fn capture_bitblt(hwnd: HWND, width: u32, height: u32) -> Vec<u8> {
    // SAFETY: capture handles never leave this thread. The old selected object
    // is restored before deleting the bitmap/DC, and GetDC is balanced.
    unsafe {
        let source = GetDC(Some(hwnd));
        let memory = CreateCompatibleDC(Some(source));
        let bitmap = CreateCompatibleBitmap(source, width as i32, height as i32);
        let previous = SelectObject(memory, bitmap.into());
        let copied = BitBlt(
            memory,
            0,
            0,
            width as i32,
            height as i32,
            Some(source),
            0,
            0,
            SRCCOPY,
        );
        let mut rgb = Vec::with_capacity(width as usize * height as usize * 3);
        if copied.is_ok() {
            for y in 0..height {
                for x in 0..width {
                    let pixel = GetPixel(memory, x as i32, y as i32).0;
                    rgb.extend([
                        (pixel & 255) as u8,
                        ((pixel >> 8) & 255) as u8,
                        ((pixel >> 16) & 255) as u8,
                    ]);
                }
            }
        }
        SelectObject(memory, previous);
        let _ = DeleteObject(bitmap.into());
        let _ = DeleteDC(memory);
        ReleaseDC(Some(hwnd), source);
        copied.expect("capture model window with BitBlt");
        rgb
    }
}
