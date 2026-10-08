//! The options the renderer can actually honour.

use super::*;
use windows::Win32::UI::WindowsAndMessaging::{SW_MINIMIZE, SW_RESTORE};

#[test]
fn window_mode_numeric_scale_uses_model_size_instead_of_saved_geometry() {
    let catalog = PresetModelCatalog::open(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../resources/models"),
        ModelPackageLimits::default(),
    )
    .unwrap();
    let (runtime, consumer) = RuntimeOwner::start_with_rendering(false, 64);
    let client = runtime.client();
    client.wait_for_revision(1, RUNTIME_TIMEOUT).unwrap();
    let settings = bongocat_runtime::OverlaySettings {
        window_mode: true,
        keep_inside_screen: false,
        ..Default::default()
    };
    let sequence = client
        .send(RuntimeCommand::SetOverlaySettings(settings))
        .unwrap();
    client.wait_for_command(sequence, RUNTIME_TIMEOUT).unwrap();
    let model = catalog.load(&ModelId::parse("standard").unwrap()).unwrap();
    let (token, frame) = prepare_switch_frame(
        &client,
        &consumer,
        Arc::new(model),
        Arc::new(preview_input_bindings("standard")),
    )
    .unwrap();
    let apartment = ComApartment::initialize().unwrap();
    let options = OverlaySessionOptions::default().with_runtime_settings(settings);
    let mut overlay = NativeOverlay::create(
        &frame,
        options,
        Some(OverlayWindowBounds::new(100, 100, 720, 308)),
        None,
        None,
    )
    .unwrap();
    overlay.draw_capturing(true).unwrap();
    report_model_commit(&client, &consumer, token, ModelCommitOutcome::Prepared).unwrap();
    // Exercise the real typed-command/tick path without capturing desktop input.
    let mut session = ProductOverlaySession {
        overlay,
        runtime_client: client.clone(),
        render_consumer: consumer,
        _com_apartment: apartment,
        input_service: None,
        input_start_error: None,
        input_diagnostics: None,
        input_stopped: true,
        frames_presented: 0,
        dynamic_snapshots: 0,
        model_commit_rejections: 0,
        previous_snapshot: Arc::clone(&frame.snapshot),
        options,
        last_frame: frame,
        retry_backoff: FrameRetryBackoff::default(),
        context_menu_sender: None,
        resize_sender: None,
        hover: PointerHoverHide::default(),
        idle: IdleHide::default(),
        placement: OverlayPlacementConstraint::default(),
        session_started: Instant::now(),
    };
    for (window_mode, scale_percent, minimized) in [
        (true, 125, false),
        (true, 50, false),
        (true, 101, false),
        (true, 100, false),
        (true, 25, false),
        (true, 125, true),
        (false, 200, false),
        (true, 100, false),
    ] {
        let previous_hwnd = session.overlay.window.hwnd;
        let previous_mode = session.options.window_mode;
        if minimized {
            // SAFETY: visibility changes and message pumping stay on the HWND owner thread.
            unsafe {
                let _ = ShowWindow(previous_hwnd, SW_MINIMIZE);
            }
            pump_window_messages();
        }
        let sequence = client
            .send(RuntimeCommand::SetOverlaySettings(
                bongocat_runtime::OverlaySettings {
                    scale_percent,
                    window_mode,
                    ..settings
                },
            ))
            .unwrap();
        client.wait_for_command(sequence, RUNTIME_TIMEOUT).unwrap();
        session.tick().unwrap();
        assert_session_scale(&session, scale_percent);
        if window_mode == previous_mode {
            assert_eq!(session.overlay.window.hwnd, previous_hwnd);
        } else {
            assert_ne!(session.overlay.window.hwnd, previous_hwnd);
        }
        if minimized {
            // SAFETY: restore the owned HWND after its normal placement was resized.
            unsafe {
                let _ = ShowWindow(session.overlay.window.hwnd, SW_RESTORE);
            }
            pump_window_messages();
            session.tick().unwrap();
            assert_session_scale(&session, scale_percent);
        }
        session.overlay.draw_capturing(true).unwrap();
    }
    let hwnd = session.overlay.window.hwnd;
    for (model_id, minimized) in [("keyboard", false), ("gamepad", true), ("standard", false)] {
        if minimized {
            // SAFETY: minimize only this test's HWND on its owner thread.
            unsafe {
                let _ = ShowWindow(hwnd, SW_MINIMIZE);
            }
            pump_window_messages();
        }
        let model = catalog.load(&ModelId::parse(model_id).unwrap()).unwrap();
        let sequence = client
            .send(RuntimeCommand::ActivateModelWithBindings {
                model: Arc::new(model),
                input_bindings: Arc::new(preview_input_bindings(model_id)),
            })
            .unwrap();
        client
            .wait_for_model_preparation(sequence, RUNTIME_TIMEOUT)
            .unwrap();
        session.tick().unwrap();
        client.wait_for_command(sequence, RUNTIME_TIMEOUT).unwrap();
        assert_eq!(session.overlay.window.hwnd, hwnd);
        assert_session_scale(&session, 100);
        if minimized {
            // SAFETY: restoration stays on the same HWND owner thread.
            unsafe {
                let _ = ShowWindow(hwnd, SW_RESTORE);
            }
            pump_window_messages();
            session.tick().unwrap();
            assert_session_scale(&session, 100);
        }
        for scale_percent in [125, 100] {
            let sequence = client
                .send(RuntimeCommand::SetOverlaySettings(
                    bongocat_runtime::OverlaySettings {
                        scale_percent,
                        ..settings
                    },
                ))
                .unwrap();
            client.wait_for_command(sequence, RUNTIME_TIMEOUT).unwrap();
            session.tick().unwrap();
            assert_session_scale(&session, scale_percent);
        }
        session.overlay.draw_capturing(true).unwrap();
    }
    runtime.shutdown(RUNTIME_TIMEOUT).unwrap();
    session.finish_after_runtime_shutdown().unwrap();
}

fn assert_session_scale(session: &ProductOverlaySession, scale_percent: u16) {
    let bounds = session.window_bounds().unwrap();
    let canvas = session.last_frame.snapshot.canvas;
    // SAFETY: the session owns this HWND on the calling test thread.
    let dpi = unsafe { GetDpiForWindow(session.overlay.window.hwnd) };
    let minimum_width = 64.0_f64
        .max(64.0 * f64::from(canvas.width) / f64::from(canvas.height))
        .ceil() as u32;
    let expected_width = (350.0 * f64::from(dpi) * f64::from(scale_percent) / 9600.0).ceil() as u32;
    assert_eq!(
        bounds.width,
        expected_width.max(minimum_width),
        "scale {scale_percent}%"
    );
    assert_eq!(
        bounds.height,
        (f64::from(bounds.width) * f64::from(canvas.height) / f64::from(canvas.width)).ceil()
            as u32
    );
    assert_eq!(
        (
            session.overlay.renderer.width,
            session.overlay.renderer.height
        ),
        (bounds.width, bounds.height)
    );
}

#[test]
fn product_options_reject_values_outside_renderer_boundaries() {
    for options in [
        OverlaySessionOptions {
            scale_percent: 24,
            ..OverlaySessionOptions::default()
        },
        OverlaySessionOptions {
            opacity_percent: 0,
            ..OverlaySessionOptions::default()
        },
        OverlaySessionOptions {
            corner_radius_percent: 51,
            ..OverlaySessionOptions::default()
        },
        OverlaySessionOptions {
            hide_on_pointer_hover_delay_ms: MAXIMUM_HIDE_ON_POINTER_HOVER_DELAY_MS + 1,
            ..OverlaySessionOptions::default()
        },
        OverlaySessionOptions {
            hide_on_idle_delay_ms: MAXIMUM_HIDE_ON_IDLE_DELAY_MS + 1,
            ..OverlaySessionOptions::default()
        },
        OverlaySessionOptions {
            maximum_fps: 14,
            ..OverlaySessionOptions::default()
        },
        OverlaySessionOptions {
            maximum_fps: 241,
            ..OverlaySessionOptions::default()
        },
    ] {
        assert!(validate_options(options).is_err());
    }
}
