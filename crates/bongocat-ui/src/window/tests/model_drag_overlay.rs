//! Dragging and dropping a model folder into the window.

use super::*;

#[gpui_kit::test]
fn dragging_a_model_folder_shows_and_clears_a_full_window_overlay(cx: &mut TestAppContext) {
    let (view, visual) = settings_view(cx);
    visual.update(|window, cx| window.render_frame(cx));
    let viewport = visual.update(|window, _| window.viewport_size());
    let position = point(px(24.0), px(24.0));

    visual.update(|window, cx| {
        let _ = window.dispatch_event(
            FileDropEvent::Entered {
                position,
                paths: ExternalPaths(
                    [std::env::temp_dir().join("bongocat-model-folder")]
                        .into_iter()
                        .collect(),
                ),
            }
            .to_platform_input(),
            cx,
        );
    });
    visual.update(|window, cx| window.render_frame(cx));

    assert_eq!(
        view.read_with(visual, |view, _| view.model_drag),
        Some(ModelDragOverlayState::Ready)
    );
    let overlay = rendered_bounds(visual, ElementId::from("model-drop-overlay"));
    assert_eq!(overlay.origin, point(px(0.0), px(0.0)));
    assert_eq!(
        overlay.size, viewport,
        "the model drop affordance must cover the complete settings window"
    );

    // Leave the viewport before Exited, just like a real DragLeave/DragExit. The
    // window-level listener must still clear the overlay after the hitbox loses
    // hover state.
    visual.update(|window, cx| {
        let _ = window.dispatch_event(
            FileDropEvent::Pending {
                position: point(px(-24.0), px(-24.0)),
            }
            .to_platform_input(),
            cx,
        );
        let _ = window.dispatch_event(FileDropEvent::Exited.to_platform_input(), cx);
    });
    assert!(
        view.read_with(visual, |view, _| view.model_drag.is_none()),
        "leaving the window must clear the drag state before the next frame"
    );
    visual.update(|window, cx| window.render_frame(cx));
    assert!(
        visual.update(|window, _| {
            window
                .try_find(ElementId::from("model-drop-overlay"))
                .is_none()
        }),
        "leaving the window must remove the temporary drop affordance"
    );
}

#[gpui_kit::test]
fn dropping_one_folder_enters_the_existing_inspection_command(cx: &mut TestAppContext) {
    static NEXT_DROP_TEST: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let source = std::env::temp_dir().join(format!(
        "bongocat-model-drop-test-{}-{}",
        std::process::id(),
        NEXT_DROP_TEST.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
    ));
    std::fs::create_dir(&source).expect("create dropped model folder");
    let canonical = source
        .canonicalize()
        .expect("canonical dropped model folder");
    let (view, visual, endpoint) = settings_view_with_endpoint(cx);
    visual.update(|window, cx| window.render_frame(cx));
    let position = point(px(24.0), px(24.0));

    visual.update(|window, cx| {
        let _ = window.dispatch_event(
            FileDropEvent::Entered {
                position,
                paths: ExternalPaths([source.clone()].into_iter().collect()),
            }
            .to_platform_input(),
            cx,
        );
        let _ = window.dispatch_event(FileDropEvent::Submit { position }.to_platform_input(), cx);
    });
    assert!(
        view.read_with(visual, |view, _| {
            matches!(view.model_import.state, ModelImportState::ValidatingDrop)
        }),
        "the drop must enter background validation before service inspection"
    );
    assert!(
        view.read_with(visual, |view, _| view.model_drag.is_none()),
        "submitting a drag must remove the overlay immediately"
    );

    visual.run_until_parked();
    assert_eq!(
        view.read_with(visual, |view, _| view.model_import.source_root.clone()),
        Some(canonical.clone())
    );
    assert!(
        view.read_with(visual, |view, _| {
            matches!(view.model_import.state, ModelImportState::Inspecting)
        }),
        "the validated path must enter the existing source inspection flow"
    );
    assert!(matches!(
        endpoint.try_recv().expect("inspection command"),
        crate::SettingsCommand::InspectModelSource { source_root, .. }
            if source_root == canonical
    ));
    assert!(
        view.read_with(visual, |view, _| view.model_drag.is_none()),
        "validation must not leave a stale drag affordance"
    );

    std::fs::remove_dir(&source).expect("remove dropped model folder");
}

#[gpui_kit::test]
fn dragging_while_a_model_source_is_busy_shows_a_waiting_overlay(cx: &mut TestAppContext) {
    let (view, visual, endpoint) = settings_view_with_endpoint(cx);
    visual.update(|window, cx| window.render_frame(cx));
    view.update(visual, |view, _| {
        view.model_import.state = ModelImportState::Inspecting;
    });
    let position = point(px(24.0), px(24.0));

    visual.update(|window, cx| {
        let _ = window.dispatch_event(
            FileDropEvent::Entered {
                position,
                paths: ExternalPaths(
                    [std::env::temp_dir().join("busy-model")]
                        .into_iter()
                        .collect(),
                ),
            }
            .to_platform_input(),
            cx,
        );
    });
    assert_eq!(
        view.read_with(visual, |view, _| view.model_drag),
        Some(ModelDragOverlayState::Busy)
    );

    visual.update(|window, cx| {
        let _ = window.dispatch_event(FileDropEvent::Submit { position }.to_platform_input(), cx);
    });
    assert!(
        view.read_with(visual, |view, _| {
            matches!(view.model_import.state, ModelImportState::Inspecting)
        }),
        "a busy source must not be replaced by a dropped folder"
    );
    assert!(view.read_with(visual, |view, _| view.model_drag.is_none()));
    assert!(endpoint.try_recv().is_err());
}

#[gpui_kit::test]
fn dropping_multiple_items_is_rejected_without_starting_import(cx: &mut TestAppContext) {
    let (view, visual) = settings_view(cx);
    visual.update(|window, cx| window.render_frame(cx));
    let position = point(px(24.0), px(24.0));
    let paths = ExternalPaths(
        [
            std::env::temp_dir().join("bongocat-model-a"),
            std::env::temp_dir().join("bongocat-model-b"),
        ]
        .into_iter()
        .collect(),
    );

    visual.update(|window, cx| {
        let _ = window.dispatch_event(
            FileDropEvent::Entered {
                position,
                paths: paths.clone(),
            }
            .to_platform_input(),
            cx,
        );
    });
    assert_eq!(
        view.read_with(visual, |view, _| view.model_drag),
        Some(ModelDragOverlayState::InvalidSelection)
    );
    visual.update(|window, cx| {
        let _ = window.dispatch_event(FileDropEvent::Submit { position }.to_platform_input(), cx);
    });

    assert!(
        view.read_with(visual, |view, _| {
            matches!(view.model_import.state, ModelImportState::Idle)
        }),
        "a rejected drag must leave the import draft idle"
    );
    assert!(
        visual.update(|window, _| window.try_find("notification").is_some()),
        "a multi-item drop must explain the one-folder boundary"
    );
    assert!(
        visual.update(|window, _| {
            window
                .try_find(ElementId::from("model-drop-overlay"))
                .is_none()
        }),
        "a rejected drag must not leave its overlay behind"
    );
}
