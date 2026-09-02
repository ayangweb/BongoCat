use super::*;

pub fn open_settings_window(
    client: SettingsClient,
    window_state: SettingsWindowState,
    request_quit: impl Fn(&mut App) + 'static,
    cx: &mut App,
) -> Result<SettingsWindowHandle, String> {
    let window_bounds = initial_window_bounds(&window_state, cx);
    let accessibility_error = Rc::new(RefCell::new(None));
    let open_accessibility_error = Rc::clone(&accessibility_error);
    let settings_view = Rc::new(RefCell::new(None));
    let opened_settings_view = Rc::clone(&settings_view);
    let handle = cx
        .open_window(
            WindowOptions {
                window_bounds: Some(window_bounds),
                window_min_size: Some(size(px(WINDOW_MIN_WIDTH), px(WINDOW_MIN_HEIGHT))),
                titlebar: Some(TitlebarOptions {
                    title: Some("BongoCat Settings".into()),
                    ..Default::default()
                }),
                focus: false,
                show: false,
                ..Default::default()
            },
            move |window, cx| {
                if !cx.has_global::<Theme>() {
                    gpui_component::init(cx);
                }
                install_component_theme(window, cx);
                window
                    .observe_window_appearance(|window, cx| {
                        install_component_theme(window, cx);
                    })
                    .detach();
                if let Some(placement) = placement_from_window(window) {
                    window_state.update(placement);
                }
                let request_quit = Rc::new(request_quit);
                let view = cx.new(|cx| {
                    let observed_window_state = window_state.clone();
                    cx.observe_window_bounds(window, move |_, window, _| {
                        if let Some(placement) = placement_from_window(window) {
                            observed_window_state.update(placement);
                        }
                    })
                    .detach();
                    let mut view = SettingsView::new(client, request_quit, window, cx);
                    view.refresh(cx);
                    view
                });
                opened_settings_view.borrow_mut().replace(view.clone());
                #[cfg(any(target_os = "macos", target_os = "windows"))]
                {
                    let result = HasWindowHandle::window_handle(window)
                        .map_err(|error| error.to_string())
                        .and_then(|handle| {
                            SettingsAccessibilityBridge::attach(
                                handle.as_raw(),
                                view.read(cx).accessibility_tree(),
                            )
                            .map_err(|error| error.to_string())
                        });
                    match result {
                        Ok((bridge, receiver)) => {
                            view.update(cx, |view, cx| {
                                view.accessibility = Some(bridge);
                                view.start_accessibility_actions(receiver, cx);
                            });
                        }
                        Err(error) => *open_accessibility_error.borrow_mut() = Some(error),
                    }
                }
                #[cfg(target_os = "windows")]
                {
                    let weak_view = view.downgrade();
                    window.on_window_should_close(cx, move |window, cx| {
                        let result = bongocat_platform::hide_native_window(window);
                        let _ = weak_view.update(cx, |view, cx| match result {
                            Ok(()) => {
                                view.window_hidden = true;
                                cx.notify();
                            }
                            Err(_) => view.report_service_error(
                                SettingsError::new(crate::SettingsErrorCode::WindowUnavailable),
                                cx,
                            ),
                        });
                        false
                    });
                }
                window.focus(&view.read(cx).overlay_focus);
                if open_accessibility_error.borrow().is_none() {
                    window.activate_window();
                }
                cx.new(|cx| Root::new(view, window, cx))
            },
        )
        .map_err(|error| error.to_string())?;
    if let Some(error) = accessibility_error.borrow_mut().take() {
        let _ = handle.update(cx, |_, window, _| window.remove_window());
        return Err(format!("attach settings accessibility bridge: {error}"));
    }
    cx.activate(true);
    let view = settings_view
        .borrow_mut()
        .take()
        .ok_or_else(|| "settings view was not created".to_owned())?;
    Ok(SettingsWindowHandle {
        window: handle,
        view: view.downgrade(),
    })
}

fn initial_window_bounds(window_state: &SettingsWindowState, cx: &App) -> WindowBounds {
    let centered = || {
        WindowBounds::Windowed(Bounds::centered(
            None,
            size(px(WINDOW_WIDTH), px(WINDOW_HEIGHT)),
            cx,
        ))
    };
    let Some(placement) = window_state.placement() else {
        return centered();
    };
    let bounds = Bounds::new(
        point(px(placement.x as f32), px(placement.y as f32)),
        size(px(placement.width as f32), px(placement.height as f32)),
    );
    if !cx
        .displays()
        .iter()
        .any(|display| bounds.intersects(&display.bounds()))
    {
        return centered();
    }
    if placement.maximized {
        WindowBounds::Maximized(bounds)
    } else {
        WindowBounds::Windowed(bounds)
    }
}

fn placement_from_window(window: &Window) -> Option<SettingsWindowPlacement> {
    let window_bounds = window.window_bounds();
    let maximized = match window_bounds {
        WindowBounds::Windowed(_) => false,
        WindowBounds::Maximized(_) => true,
        WindowBounds::Fullscreen(_) => return None,
    };
    let bounds = window_bounds.get_bounds();
    SettingsWindowPlacement::new(
        rounded_i32(bounds.origin.x)?,
        rounded_i32(bounds.origin.y)?,
        rounded_u32(bounds.size.width)?,
        rounded_u32(bounds.size.height)?,
        maximized,
    )
}

fn rounded_i32(value: gpui::Pixels) -> Option<i32> {
    let value = f32::from(value);
    if !value.is_finite() || value < i32::MIN as f32 || value > i32::MAX as f32 {
        return None;
    }
    Some(value.round() as i32)
}

fn rounded_u32(value: gpui::Pixels) -> Option<u32> {
    let value = f32::from(value);
    if !value.is_finite() || value < 0.0 || value > u32::MAX as f32 {
        return None;
    }
    Some(value.round() as u32)
}
