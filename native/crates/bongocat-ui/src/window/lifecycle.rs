use super::*;

pub fn open_settings_window(
    client: SettingsClient,
    window_state: SettingsWindowState,
    request_quit: impl Fn(&mut App) + 'static,
    cx: &mut App,
) -> Result<SettingsWindowHandle, String> {
    let (window_bounds, display_id) = initial_window_bounds(&window_state, cx);
    let initial_content_size = window_bounds.get_bounds().size;
    let normalize_initial_content_size = matches!(window_bounds, WindowBounds::Windowed(_));
    let accessibility_error = Rc::new(RefCell::new(None));
    let open_accessibility_error = Rc::clone(&accessibility_error);
    let settings_view = Rc::new(RefCell::new(None));
    let opened_settings_view = Rc::clone(&settings_view);
    let handle = cx
        .open_window(
            WindowOptions {
                window_bounds: Some(window_bounds),
                display_id,
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
                    gpui_kit::init(cx);
                }
                install_component_theme(window, cx);
                window
                    .observe_window_appearance(|window, cx| {
                        install_component_theme(window, cx);
                    })
                    .detach();
                let request_quit = Rc::new(request_quit);
                let view = cx.new(|cx| {
                    let observed_window_state = window_state.clone();
                    cx.observe_window_bounds(window, move |_, window, cx| {
                        if let Some(placement) = placement_from_window(window, cx)
                            && let Some(revision) = observed_window_state.update(placement)
                        {
                            let pending_window_state = observed_window_state.clone();
                            let executor = cx.background_executor().clone();
                            cx.spawn(async move |_, _| {
                                executor.timer(Duration::from_millis(150)).await;
                                for _ in 0..20 {
                                    if pending_window_state.request_persist_if_current(revision) {
                                        break;
                                    }
                                    executor.timer(Duration::from_millis(50)).await;
                                }
                            })
                            .detach();
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
                let overlay_focus = view.read(cx).overlay_focus.clone();
                window.focus(&overlay_focus, cx);
                if normalize_initial_content_size && window.viewport_size() != initial_content_size
                {
                    window.resize(initial_content_size);
                } else if let Some(placement) = placement_from_window(window, cx)
                    && let Some(revision) = window_state.update(placement)
                {
                    let _ = window_state.request_persist_if_current(revision);
                }
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

fn initial_window_bounds(
    window_state: &SettingsWindowState,
    cx: &App,
) -> (WindowBounds, Option<DisplayId>) {
    let content_top_inset = settings_window_content_top_inset();
    let centered = || {
        let window_size = size(px(WINDOW_WIDTH), px(WINDOW_HEIGHT));
        let Some(display) = bongocat_platform::current_display_bounds() else {
            let bounds = Bounds::centered(None, window_size, cx);
            return (
                WindowBounds::Windowed(Bounds::new(
                    point(bounds.origin.x, bounds.origin.y - px(content_top_inset)),
                    bounds.size,
                )),
                None,
            );
        };
        let global_x = display.x + (display.width - WINDOW_WIDTH) / 2.0;
        let global_y = display.y + (display.height - WINDOW_HEIGHT) / 2.0;
        let (x, y) = bongocat_platform::local_window_origin(display, global_x, global_y);
        (
            WindowBounds::Windowed(Bounds::new(
                point(px(x), px(y - content_top_inset)),
                window_size,
            )),
            gpui_display_id(display.display_id, cx),
        )
    };
    let Some(placement) = window_state.placement() else {
        return centered();
    };
    let Some(display) = bongocat_platform::display_bounds_for_window(
        placement.x as f32,
        placement.y as f32,
        placement.width as f32,
        placement.height as f32,
    ) else {
        return centered();
    };
    let (x, y) =
        bongocat_platform::local_window_origin(display, placement.x as f32, placement.y as f32);
    let bounds = Bounds::new(
        point(px(x), px(y - content_top_inset)),
        size(px(placement.width as f32), px(placement.height as f32)),
    );
    let bounds = if placement.maximized {
        WindowBounds::Maximized(bounds)
    } else {
        WindowBounds::Windowed(bounds)
    };
    (bounds, gpui_display_id(display.display_id, cx))
}

fn gpui_display_id(display_id: Option<u32>, cx: &App) -> Option<DisplayId> {
    let display_id = display_id?;
    cx.displays()
        .into_iter()
        .find(|display| u32::try_from(u64::from(display.id())).ok() == Some(display_id))
        .map(|display| display.id())
}

fn placement_from_window(window: &Window, cx: &App) -> Option<SettingsWindowPlacement> {
    let window_bounds = window.window_bounds();
    let maximized = match window_bounds {
        WindowBounds::Windowed(_) => false,
        WindowBounds::Maximized(_) => true,
        WindowBounds::Fullscreen(_) => return None,
    };
    let bounds = window_bounds.get_bounds();
    let content_size = window.viewport_size();
    let content_top_inset = settings_window_content_top_inset();
    let (x, y) = bongocat_platform::global_window_origin(
        window
            .display(cx)
            .and_then(|display| u32::try_from(u64::from(display.id())).ok()),
        f32::from(bounds.origin.x),
        f32::from(bounds.origin.y) + content_top_inset,
    );
    SettingsWindowPlacement::new(
        rounded_f32_i32(x)?,
        rounded_f32_i32(y)?,
        rounded_u32(content_size.width)?,
        rounded_u32(content_size.height)?,
        maximized,
    )
}

fn settings_window_content_top_inset() -> f32 {
    #[cfg(target_os = "macos")]
    {
        bongocat_platform::window_content_top_inset()
    }
    #[cfg(target_os = "windows")]
    {
        0.0
    }
}

fn rounded_f32_i32(value: f32) -> Option<i32> {
    if !value.is_finite() || value < i32::MIN as f32 || value > i32::MAX as f32 {
        return None;
    }
    Some(value.round() as i32)
}

fn rounded_u32(value: Pixels) -> Option<u32> {
    let value = f32::from(value);
    if !value.is_finite() || value < 0.0 || value > u32::MAX as f32 {
        return None;
    }
    Some(value.round() as u32)
}
