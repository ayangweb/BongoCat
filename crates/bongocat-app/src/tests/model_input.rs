//! The input bindings a model derives from the key artwork it ships.

use super::*;

#[test]
fn installed_models_get_default_keyboard_bindings() {
    // An imported model is bound from the same keyboard table as the
    // presets, keyed on the artwork its own package ships; the `keyboard`
    // preset stands in for one that carries both hands.
    let keyboard_images = shipped_key_images("keyboard");
    let bindings =
        input_bindings_for_model(ModelOrigin::Installed, "custom-model", &keyboard_images);
    assert_eq!(bindings.hand_for(PhysicalKey::KEY_A), Some(HandSide::Left));
    assert_eq!(
        bindings.hand_for(PhysicalKey::from_hid_usage(0x52)),
        Some(HandSide::Right)
    );

    // Preset mappings must stay exactly as before the fix.
    let standard = input_bindings_for_model(
        ModelOrigin::Preset,
        "standard",
        &shipped_key_images("standard"),
    );
    assert_eq!(standard.hand_for(PhysicalKey::KEY_A), Some(HandSide::Left));
    assert_eq!(standard.hand_for(PhysicalKey::from_hid_usage(0x4f)), None);
    // The gamepad preset ships no keyboard artwork at all, so no keyboard
    // key reaches it; its gamepad buttons are bound on their own map.
    let gamepad = input_bindings_for_model(
        ModelOrigin::Preset,
        "gamepad",
        &shipped_key_images("gamepad"),
    );
    assert_eq!(gamepad.hand_for(PhysicalKey::KEY_A), None);
    assert_eq!(gamepad.hand_for(PhysicalKey::from_hid_usage(0x4f)), None);
    // The bundled gamepad model keeps the face buttons in `right-keys`, so
    // they are the right hand's — the hand follows the model's artwork, not
    // a hardcoded table (see `gamepad_hands_for_model`).
    for button in [
        GamepadButton::South,
        GamepadButton::East,
        GamepadButton::West,
        GamepadButton::North,
    ] {
        assert_eq!(
            gamepad.hand_for_gamepad(button),
            Some(HandSide::Right),
            "{button:?} is a right-keys image"
        );
    }
    assert_eq!(gamepad.hand_for_gamepad(GamepadButton::Select), None);
}

/// `InputState::model_snapshot` drops any press without a hand assignment, so
/// a function key missing from this map can never draw its `Fn.png` overlay.
#[test]
fn keyboard_models_assign_the_whole_function_row_to_the_left_hand() {
    for (origin, id, images) in [
        (ModelOrigin::Installed, "custom-model", "keyboard"),
        (ModelOrigin::Preset, "standard", "standard"),
        (ModelOrigin::Preset, "keyboard", "keyboard"),
    ] {
        let bindings = input_bindings_for_model(origin, id, &shipped_key_images(images));
        for (first, last) in bongocat_render::FUNCTION_KEY_USAGES {
            for usage in first..=last {
                assert_eq!(
                    bindings.hand_for(PhysicalKey::from_hid_usage(usage)),
                    Some(HandSide::Left),
                    "{id} 0x{usage:02x}"
                );
            }
        }
        // PrintScreen sits directly after F12 but is not a function key: it
        // gets no `Fn` fallback from the resolver, and no shipped model
        // draws `PrintScreen.png`, so it is not bound either.
        assert_eq!(
            bongocat_render::function_key_name(0x46),
            None,
            "{id} PrintScreen must not be a function key"
        );
        assert_eq!(
            bindings.hand_for(PhysicalKey::from_hid_usage(0x46)),
            None,
            "{id} PrintScreen has no artwork to draw"
        );
    }

    // The gamepad model keeps its button-only mapping.
    let gamepad = input_bindings_for_model(
        ModelOrigin::Preset,
        "gamepad",
        &shipped_key_images("gamepad"),
    );
    assert_eq!(gamepad.hand_for(PhysicalKey::from_hid_usage(0x3a)), None);
    assert_eq!(gamepad.hand_for(PhysicalKey::from_hid_usage(0x68)), None);
}

/// The globe key is gated on artwork like every other key, and it is the one
/// key outside the HID Keyboard/Keypad page, so no range over that page can
/// reach it — it has to be bound explicitly.
///
/// Every model shipped today predates the key and carries no artwork for it,
/// so pressing it must do nothing at all: no key layer and no paw movement
/// (ADR-0042). A model that does ship the image is bound to the left hand,
/// under the canonical name or the pre-rename spelling, because the binding
/// asks the same `can_draw` the renderer draws with.
#[test]
fn the_globe_key_binds_only_for_a_model_that_ships_its_image() {
    let globe = PhysicalKey::from_hid_usage(bongocat_render::GLOBE_KEY_USAGE);
    for (origin, id, images) in [
        (ModelOrigin::Installed, "custom-model", "keyboard"),
        (ModelOrigin::Preset, "standard", "standard"),
        (ModelOrigin::Preset, "keyboard", "keyboard"),
        (ModelOrigin::Preset, "gamepad", "gamepad"),
    ] {
        let bindings = input_bindings_for_model(origin, id, &shipped_key_images(images));
        assert_eq!(
            bindings.hand_for(globe),
            None,
            "{id} ships no Globe.png, so the key must be inert"
        );
    }

    for name in ["Globe", "Function"] {
        let root = tempdir().expect("root");
        let left_keys = root.path().join("resources/left-keys");
        fs::create_dir_all(&left_keys).expect("left keys directory");
        fs::write(left_keys.join(format!("{name}.png")), b"globe").expect("globe image");
        let inventory = KeyImageInventory::read(root.path());
        let bindings = input_bindings_for_model(ModelOrigin::Installed, "custom-model", &inventory);
        assert_eq!(
            bindings.hand_for(globe),
            Some(HandSide::Left),
            "{name}.png must bind the globe key"
        );
        // And the shared function-row image is still a different key's.
        assert_eq!(
            bindings.hand_for(PhysicalKey::from_hid_usage(0x3a)),
            None,
            "{name}.png must not bind a function key"
        );
    }
}

/// The static table still covers the whole standard 104/105-key layout plus
/// the keypad, and the key image a model ships is the only thing that can
/// remove a key from it. `InputState::model_snapshot` drops a press whose key
/// has no hand assignment before the resolver ever sees it, so an unbound key
/// is inert end to end: no key layer and no paw movement. The whole block
/// goes to the left hand because `left-keys` is where the shipped key images
/// live; the arrow cluster is the right hand's.
#[test]
fn keyboard_models_bind_every_drawable_key_of_the_standard_layout() {
    let standard_images = shipped_key_images("standard");
    let keyboard_images = shipped_key_images("keyboard");
    for (origin, id, images, binds_arrows) in [
        (
            ModelOrigin::Installed,
            "custom-model",
            &keyboard_images,
            true,
        ),
        (ModelOrigin::Preset, "standard", &standard_images, false),
        (ModelOrigin::Preset, "keyboard", &keyboard_images, true),
    ] {
        let bindings = input_bindings_for_model(origin, id, images);
        // Walk the whole adapter union, not the ranges the implementation
        // happens to use: the modifier block sits outside `0x04..=0x65` and
        // outside `0x68..=0x73`, and a rewrite of those two loops dropped all
        // eight of them once already.
        for usage in adapter_keyboard_usages() {
            let expected = if (0x4f..=0x52).contains(&usage) {
                // The four arrows are the right hand's cluster.
                binds_arrows
                    .then(|| images.can_draw(KeySide::Right, usage))
                    .filter(|drawable| *drawable)
                    .map(|_| HandSide::Right)
            } else {
                images
                    .can_draw(KeySide::Left, usage)
                    .then_some(HandSide::Left)
            };
            assert_eq!(
                bindings.hand_for(PhysicalKey::from_hid_usage(usage)),
                expected,
                "{id} 0x{usage:02x}"
            );
        }
        // Spot checks, so the rule stays visible instead of being only a
        // mirror of the code under test.
        for (usage, expected, why) in [
            (0x04, true, "every keyboard model ships KeyA.png"),
            (0x1e, true, "every keyboard model ships Num1.png"),
            (0x4c, true, "every keyboard model ships Delete.png"),
            (0x59, true, "keypad 1 falls back to Num1.png"),
            (0x58, true, "keypad Enter falls back to Enter.png"),
            (0xe0, true, "Control.png is the shared family image"),
            (0xe1, true, "every keyboard model ships ShiftLeft.png"),
            (0xe2, true, "every keyboard model ships AltLeft.png"),
            (0xe3, true, "MetaLeft.png is not shipped, Meta.png is"),
            (0xe5, true, "every keyboard model ships ShiftRight.png"),
            (0xe6, true, "every keyboard model ships AltRight.png"),
            (0xe7, true, "MetaRight.png is not shipped, Meta.png is"),
            (0x37, false, "no shipped model draws Dot.png"),
            (0x46, false, "no shipped model draws PrintScreen.png"),
            (0x53, false, "no shipped model draws NumLock.png"),
            (0x63, false, "keypad . has no artwork to fall back to"),
        ] {
            assert_eq!(
                bindings
                    .hand_for(PhysicalKey::from_hid_usage(usage))
                    .is_some(),
                expected,
                "{id} 0x{usage:02x}: {why}"
            );
        }
    }

    // The union walk above covers the arrows too: they are the right hand's
    // cluster for the models that ship that artwork, and `standard` has no
    // `right-keys` directory at all.

    // The gamepad model keeps its button-only mapping.
    let gamepad = input_bindings_for_model(
        ModelOrigin::Preset,
        "gamepad",
        &shipped_key_images("gamepad"),
    );
    assert_eq!(gamepad.hand_for(PhysicalKey::from_hid_usage(0x58)), None);
    assert_eq!(gamepad.hand_for(PhysicalKey::from_hid_usage(0x59)), None);
}

/// The reason the artwork gate exists at all: `CatParamLeftHandDown` and
/// `CatParamRightHandDown` are driven by the same hand assignment the key
/// overlay layer is, so a key the active model has no image for must not
/// move the paw either. The bundled `standard` model ships `KeyA.png` but no
/// `Dot.png`, which makes the two keys a complete pair: one draws and
/// presses, the other does nothing at all.
#[test]
fn a_key_the_active_model_cannot_draw_never_moves_the_paw() {
    let base = tempdir().expect("temp directory");
    let layout = StorageLayout::under(base.path(), BUILD_ENVIRONMENT);
    let mut application = Application::start_with_layout_internal(
        layout,
        repository_preset_root().as_path(),
        true,
        Language::EnglishUnitedStates,
    )
    .expect("start rendering application");
    let token = application
        .prepare_model(ModelOrigin::Preset, "standard")
        .expect("prepare standard model");
    let consumer = application
        .take_render_consumer()
        .expect("take render consumer");
    let frame = wait_for_model_commit_frame(&consumer, token);
    consumer
        .report_model_commit(ModelCommitFeedback {
            token: frame.model_commit.expect("commit token"),
            outcome: ModelCommitOutcome::Prepared,
        })
        .expect("commit standard model");
    application
        .runtime_client()
        .wait_for_command(token.command_sequence, RUNTIME_TIMEOUT)
        .expect("standard model activation");

    let input = application.input_producer();
    let mut sequence = 0;
    // `0xe1` (left Shift) is the modifier case: the bundled model draws it
    // through `ShiftLeft.png`, and a binding table that misses the modifier
    // block drops it before the paw ever moves. `0xe7` (right Meta) has no
    // `MetaRight.png` and is drawn through the shared `Meta.png`.
    for (hid_usage, drawable) in [(0x04u16, true), (0xe1, true), (0xe7, true), (0x37, false)] {
        for edge in [InputEdge::Down, InputEdge::Up] {
            sequence += 1;
            let published = input
                .publish(InputEvent::Edge {
                    control: InputControl::Key(PhysicalKey::from_hid_usage(hid_usage)),
                    edge,
                    source: InputSource::Capture,
                    at: MonotonicMillis::new(sequence),
                })
                .expect("key edge");
            let snapshot = application
                .runtime_client()
                .wait_for_input_sequence(published, RUNTIME_TIMEOUT)
                .expect("key projection");
            let pressed = edge == InputEdge::Down;
            let reason = format!("0x{hid_usage:02x} {edge:?}");
            assert_eq!(
                snapshot.model_input.left_hand_down,
                pressed && drawable,
                "{reason} left paw"
            );
            assert!(
                !snapshot.model_input.right_hand_down,
                "{reason} right paw must stay up"
            );
            assert_eq!(
                snapshot
                    .model_input
                    .key_presses
                    .iter()
                    .any(|press| press.key == KeyIdentity::Keyboard(hid_usage)),
                pressed && drawable,
                "{reason} key overlay"
            );
        }
    }
    application.shutdown().expect("clean shutdown");
}

/// The whole reported bug, end to end and on the real product: a gamepad
/// button press on the model that is actually active has to end up as a
/// resolved key overlay in the frame the renderer consumes.
///
/// Every earlier layer has its own test; this one is what would have caught
/// the original report. Before the fix the press produced a paw and no
/// overlay, for **every** button, because `KeyPress` could not express a
/// gamepad button at all. It also pins the two naming facts the fix rests
/// on: the bundled model's stems are the product's button names, and the
/// hand a button is drawn with comes from the directory it lives in.
#[test]
fn gamepad_button_presses_reach_the_render_frame_as_key_overlays() {
    let base = tempdir().expect("temp directory");
    let layout = StorageLayout::under(base.path(), BUILD_ENVIRONMENT);
    let mut application = Application::start_with_layout_internal(
        layout,
        repository_preset_root().as_path(),
        true,
        Language::EnglishUnitedStates,
    )
    .expect("start rendering application");
    let token = application
        .prepare_model(ModelOrigin::Preset, "gamepad")
        .expect("prepare gamepad model");
    let consumer = application
        .take_render_consumer()
        .expect("take render consumer");
    let frame = wait_for_model_commit_frame(&consumer, token);
    consumer
        .report_model_commit(ModelCommitFeedback {
            token: frame.model_commit.expect("commit token"),
            outcome: ModelCommitOutcome::Prepared,
        })
        .expect("commit gamepad model");
    application
        .runtime_client()
        .wait_for_command(token.command_sequence, RUNTIME_TIMEOUT)
        .expect("gamepad model activation");

    let axis = application.gamepad_axis_producer();
    let connection = axis.connect(0).expect("gamepad connection");
    let input = application.input_producer();
    input
        .publish(InputEvent::GamepadConnected {
            connection,
            at: MonotonicMillis::new(0),
        })
        .expect("connection event");
    let mut sequence = 1;
    // Every button the bundled model ships artwork for, on the side its own
    // directory puts it. The side is part of the contract, not a detail: a
    // `South` press resolves against `right-keys`, because that is where
    // `South.png` is, and the right paw is what draws it.
    for (button, side, stem) in [
        (
            GamepadButton::South,
            bongocat_render::KeySide::Right,
            "South",
        ),
        (GamepadButton::East, bongocat_render::KeySide::Right, "East"),
        (GamepadButton::West, bongocat_render::KeySide::Right, "West"),
        (
            GamepadButton::North,
            bongocat_render::KeySide::Right,
            "North",
        ),
        (
            GamepadButton::RightShoulder,
            bongocat_render::KeySide::Right,
            "RightShoulder",
        ),
        (
            GamepadButton::RightTrigger,
            bongocat_render::KeySide::Right,
            "RightTrigger",
        ),
        (
            GamepadButton::DpadUp,
            bongocat_render::KeySide::Left,
            "DpadUp",
        ),
        (
            GamepadButton::DpadDown,
            bongocat_render::KeySide::Left,
            "DpadDown",
        ),
        (
            GamepadButton::DpadLeft,
            bongocat_render::KeySide::Left,
            "DpadLeft",
        ),
        (
            GamepadButton::DpadRight,
            bongocat_render::KeySide::Left,
            "DpadRight",
        ),
        (
            GamepadButton::LeftShoulder,
            bongocat_render::KeySide::Left,
            "LeftShoulder",
        ),
        (
            GamepadButton::LeftTrigger,
            bongocat_render::KeySide::Left,
            "LeftTrigger",
        ),
    ] {
        sequence += 1;
        let published = input
            .publish(InputEvent::Edge {
                control: InputControl::Gamepad(GamepadButtonKey { connection, button }),
                edge: InputEdge::Down,
                source: InputSource::Capture,
                at: MonotonicMillis::new(sequence),
            })
            .expect("button press");
        let snapshot = application
            .runtime_client()
            .wait_for_input_sequence(published, RUNTIME_TIMEOUT)
            .expect("button projection");
        assert_eq!(
            snapshot.model_input.key_presses.iter().count(),
            1,
            "{button:?} must produce exactly one overlay"
        );
        let press = snapshot
            .model_input
            .key_presses
            .iter()
            .next()
            .unwrap_or_else(|| panic!("{button:?} never reached the model snapshot"));
        assert_eq!(press.key, KeyIdentity::Gamepad(button));
        assert_eq!(press.side, side, "{button:?} is a {side:?} image");
        assert_eq!(
            side == bongocat_render::KeySide::Left,
            snapshot.model_input.left_hand_down,
            "{button:?} left paw"
        );
        assert_eq!(
            side == bongocat_render::KeySide::Right,
            snapshot.model_input.right_hand_down,
            "{button:?} right paw"
        );

        // And the frame the renderer is handed must actually draw it, from
        // the model's own file. This is the step that was never reachable.
        let frame = wait_for_render_frame(&consumer, |snapshot| snapshot.active_keys.len() == 1);
        let overlay = frame.snapshot.active_keys[0];
        assert_eq!(overlay.side, side, "{button:?}");
        let resources = Arc::clone(&frame.resources);
        let asset = &resources.key_assets[overlay.asset_id.index()];
        assert_eq!(asset.name, stem, "{button:?}");
        assert_eq!(
            asset.name,
            button.key_image_name(),
            "the model's stem must be the product's button name"
        );
        assert_eq!(
            asset.path.file_stem().and_then(|stem| stem.to_str()),
            Some(stem),
            "{button:?} resolved to {}",
            asset.path.display()
        );

        sequence += 1;
        let released = input
            .publish(InputEvent::Edge {
                control: InputControl::Gamepad(GamepadButtonKey { connection, button }),
                edge: InputEdge::Up,
                source: InputSource::Capture,
                at: MonotonicMillis::new(sequence),
            })
            .expect("button release");
        let snapshot = application
            .runtime_client()
            .wait_for_input_sequence(released, RUNTIME_TIMEOUT)
            .expect("button release projection");
        assert_eq!(snapshot.model_input.key_presses.iter().count(), 0);
        assert!(!snapshot.model_input.left_hand_down);
        assert!(!snapshot.model_input.right_hand_down);
    }

    // The bundled model ships no `Select.png`, `Start.png` or stick artwork,
    // so those four buttons stay inert rather than borrowing another
    // button's image (ADR-0042).
    for button in [
        GamepadButton::Select,
        GamepadButton::Start,
        GamepadButton::LeftStick,
        GamepadButton::RightStick,
    ] {
        sequence += 1;
        let published = input
            .publish(InputEvent::Edge {
                control: InputControl::Gamepad(GamepadButtonKey { connection, button }),
                edge: InputEdge::Down,
                source: InputSource::Capture,
                at: MonotonicMillis::new(sequence),
            })
            .expect("button press");
        let snapshot = application
            .runtime_client()
            .wait_for_input_sequence(published, RUNTIME_TIMEOUT)
            .expect("button projection");
        assert_eq!(
            snapshot.model_input.key_presses.iter().count(),
            0,
            "{button:?} has no artwork and must be inert"
        );
        assert!(!snapshot.model_input.left_hand_down);
        assert!(!snapshot.model_input.right_hand_down);
    }

    let _ = consumer.take_latest();
    application.shutdown().expect("clean shutdown");
}

/// The binding test above proves the Map; this proves the press actually
/// survives the whole path for the model that is active at runtime. F1 and
/// F13 bracket the two HID function-key ranges.
#[test]
fn function_key_presses_reach_the_model_snapshot_with_the_left_hand() {
    let base = tempdir().expect("temp directory");
    let layout = StorageLayout::under(base.path(), BUILD_ENVIRONMENT);
    let mut application = Application::start_with_layout_internal(
        layout,
        repository_preset_root().as_path(),
        true,
        Language::EnglishUnitedStates,
    )
    .expect("start rendering application");
    let token = application
        .prepare_model(ModelOrigin::Preset, "standard")
        .expect("prepare standard model");
    let consumer = application
        .take_render_consumer()
        .expect("take render consumer");
    let frame = wait_for_model_commit_frame(&consumer, token);
    consumer
        .report_model_commit(ModelCommitFeedback {
            token: frame.model_commit.expect("commit token"),
            outcome: ModelCommitOutcome::Prepared,
        })
        .expect("commit standard model");
    application
        .runtime_client()
        .wait_for_command(token.command_sequence, RUNTIME_TIMEOUT)
        .expect("standard model activation");

    let input = application.input_producer();
    let mut sequence = 0;
    for hid_usage in [0x3au16, 0x68] {
        for edge in [InputEdge::Down, InputEdge::Up] {
            sequence += 1;
            let published = input
                .publish(InputEvent::Edge {
                    control: InputControl::Key(PhysicalKey::from_hid_usage(hid_usage)),
                    edge,
                    source: InputSource::Capture,
                    at: MonotonicMillis::new(sequence),
                })
                .expect("key edge");
            let snapshot = application
                .runtime_client()
                .wait_for_input_sequence(published, RUNTIME_TIMEOUT)
                .expect("key projection");
            let presses = snapshot.model_input.key_presses;
            if edge == InputEdge::Down {
                assert!(snapshot.model_input.left_hand_down, "0x{hid_usage:02x}");
                let press = presses
                    .iter()
                    .find(|press| press.key == KeyIdentity::Keyboard(hid_usage))
                    .unwrap_or_else(|| {
                        panic!("0x{hid_usage:02x} never reached the model snapshot")
                    });
                assert_eq!(press.side, bongocat_render::KeySide::Left);
            } else {
                assert!(
                    !presses
                        .iter()
                        .any(|press| press.key == KeyIdentity::Keyboard(hid_usage)),
                    "0x{hid_usage:02x} must be released"
                );
            }
        }
    }
    application.shutdown().expect("clean shutdown");
}
