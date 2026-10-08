//! The settings that decide what the model tracks.

use super::*;

#[test]
fn model_settings_control_pointer_tracking_and_render_mirroring() {
    let (bootstrap, consumer) = RuntimeRenderer::channel();
    let mut renderer = RuntimeRenderer::start(bootstrap);
    renderer.set_model_settings(ModelSettings {
        mirror: true,
        mirror_pointer_tracking_horizontal: true,
        mirror_pointer_tracking_vertical: false,
        ignore_keyboard: false,
        ignore_gamepad: false,
        show_all_pressed_keys: false,
        toggle_repeated_expression: false,
        allow_motion_overlap: false,
        ignore_pointer: false,
    });
    let token = renderer
        .prepare(1, &preset_model("standard"), ModelInputSnapshot::default())
        .expect("prepare model");
    assert!(renderer.commit(token));
    let initial = consumer.take_latest().expect("initial frame");
    assert!(initial.snapshot.mirror_horizontal);

    renderer
        .evaluate(
            ModelInputSnapshot {
                pointer_x: 0.5,
                pointer_y: 0.25,
                pointer_z: 0.5,
                ..ModelInputSnapshot::default()
            },
            Duration::ZERO,
        )
        .expect("mirrored pointer frame");
    let model = &renderer.active.as_ref().expect("active model").model;
    let mirrored_angle = model
        .parameter_value_by_id("ParamAngleX")
        .expect("parameter value")
        .expect("supported parameter");
    assert!(mirrored_angle < 0.0);

    renderer.set_model_settings(ModelSettings {
        mirror: false,
        mirror_pointer_tracking_horizontal: false,
        mirror_pointer_tracking_vertical: false,
        ignore_keyboard: false,
        ignore_gamepad: false,
        show_all_pressed_keys: false,
        toggle_repeated_expression: false,
        allow_motion_overlap: false,
        ignore_pointer: true,
    });
    renderer
        .evaluate(
            ModelInputSnapshot {
                pointer_x: -1.0,
                pointer_y: 1.0,
                pointer_z: -1.0,
                ..ModelInputSnapshot::default()
            },
            Duration::from_millis(1),
        )
        .expect("ignored pointer frame");
    let ignored_angle = renderer
        .active
        .as_ref()
        .expect("active model")
        .model
        .parameter_value_by_id("ParamAngleX")
        .expect("parameter value")
        .expect("supported parameter");
    let expected_reference_angle =
        0.5 * 15.0 * (std::f64::consts::TAU * 0.001 / 6.5345).sin() as f32;
    assert!(
        (ignored_angle - expected_reference_angle).abs() < 0.0001,
        "ignored pointer must leave only the reference breath: {ignored_angle}"
    );
    let frame = consumer.take_latest().expect("updated frame");
    assert!(!frame.snapshot.mirror_horizontal);
}

/// The two pointer axes are two independent corrections, so a test that only
/// ever turned one of them on would pass for a product that had wired both
/// switches to the same axis. Every case below uses the same input at the same
/// instant, so only the setting under test can move a parameter.
#[test]
fn pointer_tracking_flips_each_axis_on_its_own() {
    /// The same non-zero pointer on all three axes, so a setting that reaches
    /// the wrong one is visible instead of being multiplied by zero.
    fn tracked() -> ModelInputSnapshot {
        ModelInputSnapshot {
            pointer_x: 0.5,
            pointer_y: 0.25,
            pointer_z: 0.4,
            ..ModelInputSnapshot::default()
        }
    }

    /// The three angles the pointer drives, read back after one frame.
    fn angles(renderer: &mut RuntimeRenderer, horizontal: bool, vertical: bool) -> (f32, f32, f32) {
        renderer.set_model_settings(ModelSettings {
            mirror: false,
            mirror_pointer_tracking_horizontal: horizontal,
            mirror_pointer_tracking_vertical: vertical,
            ignore_keyboard: false,
            ignore_gamepad: false,
            show_all_pressed_keys: false,
            toggle_repeated_expression: false,
            allow_motion_overlap: false,
            ignore_pointer: false,
        });
        renderer
            .evaluate(tracked(), Duration::ZERO)
            .expect("tracked pointer frame");
        let model = &renderer.active.as_ref().expect("active model").model;
        let read = |id: &str| {
            model
                .parameter_value_by_id(id)
                .expect("parameter value")
                .expect("supported parameter")
        };
        (
            read("ParamAngleX"),
            read("ParamAngleY"),
            read("ParamAngleZ"),
        )
    }

    let (bootstrap, _consumer) = RuntimeRenderer::channel();
    let mut renderer = RuntimeRenderer::start(bootstrap);
    let token = renderer
        .prepare(1, &preset_model("standard"), ModelInputSnapshot::default())
        .expect("prepare model");
    assert!(renderer.commit(token));

    let (plain_x, plain_y, plain_z) = angles(&mut renderer, false, false);
    let (flipped_x, flipped_y, flipped_z) = angles(&mut renderer, true, false);
    assert!(
        flipped_x < 0.0 && plain_x > 0.0,
        "the horizontal switch must reverse the X axis: {plain_x} then {flipped_x}"
    );
    assert!(
        (flipped_x + plain_x).abs() < f32::EPSILON,
        "the horizontal switch must be an exact sign flip: {plain_x} then {flipped_x}"
    );
    assert!(
        (flipped_z + plain_z).abs() < f32::EPSILON,
        "the horizontal switch must reverse Z with X: {plain_z} then {flipped_z}"
    );
    assert_eq!(
        flipped_y, plain_y,
        "the horizontal switch must leave the Y axis alone"
    );

    let (raised_x, raised_y, raised_z) = angles(&mut renderer, false, true);
    assert_eq!(
        raised_x, plain_x,
        "the vertical switch must leave the X axis alone"
    );
    assert_eq!(raised_z, plain_z, "the vertical switch must leave Z alone");
    assert!(
        raised_y < 0.0 && plain_y > 0.0,
        "the vertical switch must reverse the Y axis: {plain_y} then {raised_y}"
    );
    assert!(
        (raised_y + plain_y).abs() < f32::EPSILON,
        "the vertical switch must be an exact sign flip: {plain_y} then {raised_y}"
    );

    let (both_x, both_y, both_z) = angles(&mut renderer, true, true);
    assert_eq!(
        (both_x, both_y, both_z),
        (flipped_x, raised_y, flipped_z),
        "enabling both switches must combine the two single-axis results"
    );
}

/// The analog sticks a gamepad-mode model draws are driven by
/// `CatParamStickShowLeftHand` / `CatParamStickShowRightHand`, and the
/// pre-rewrite input layer pressed that side's paw for the same condition. The
/// two parameters were declared by the product and written by nobody, so moving
/// a stick produced no artwork and no paw at all.
///
/// The paw only follows where the model has a stick to show. `standard` declares
/// no `CatParamStickShow*`, and a paw pressing down for a stick that can never
/// appear is the invisible-feedback rule of ADR-0042 applied to the stick.
#[test]
fn a_displaced_stick_shows_and_presses_its_paw_only_where_the_model_has_a_stick() {
    fn read(renderer: &RuntimeRenderer, id: &str) -> Option<f32> {
        renderer
            .active
            .as_ref()
            .expect("active model")
            .model
            .parameter_value_by_id(id)
            .expect("parameter value")
    }

    fn activate(model_id: &str) -> RuntimeRenderer {
        let (bootstrap, _consumer) = RuntimeRenderer::channel();
        let mut renderer = RuntimeRenderer::start(bootstrap);
        let token = renderer
            .prepare(1, &preset_model(model_id), ModelInputSnapshot::default())
            .expect("prepare model");
        assert!(renderer.commit(token));
        renderer
    }

    /// A stick at rest, and the same stick pushed away from center on X.
    fn stick(input: ModelInputSnapshot, x: f32) -> ModelInputSnapshot {
        ModelInputSnapshot {
            stick_left_x: x,
            ..input
        }
    }

    let mut renderer = activate("gamepad");
    let rest = stick(ModelInputSnapshot::default(), 0.0);
    let moved = stick(ModelInputSnapshot::default(), 1.0);
    renderer
        .evaluate(rest, Duration::ZERO)
        .expect("resting frame");
    let resting_left = read(&renderer, "CatParamStickShowLeftHand").expect("left stick parameter");
    let resting_right =
        read(&renderer, "CatParamStickShowRightHand").expect("right stick parameter");
    let resting_paw = read(&renderer, "CatParamLeftHandDown").expect("left paw parameter");
    renderer
        .evaluate(moved, Duration::from_millis(1))
        .expect("displaced frame");
    assert!(
        read(&renderer, "CatParamStickShowLeftHand").expect("left stick parameter") > resting_left,
        "a displaced left stick must raise CatParamStickShowLeftHand"
    );
    assert_eq!(
        read(&renderer, "CatParamStickShowRightHand").expect("right stick parameter"),
        resting_right,
        "the left stick must not raise the right stick"
    );
    assert!(
        read(&renderer, "CatParamLeftHandDown").expect("left paw parameter") > resting_paw,
        "a displaced left stick must press the left paw, as it did before the rewrite"
    );
    assert_eq!(
        read(&renderer, "CatParamRightHandDown").expect("right paw parameter"),
        0.0,
        "the left stick must not press the right paw"
    );

    // Lifting the stick back to center clears both again in the same frame,
    // so a released stick cannot leave artwork or a pressed paw behind.
    renderer
        .evaluate(rest, Duration::from_millis(2))
        .expect("released frame");
    assert_eq!(
        read(&renderer, "CatParamStickShowLeftHand").expect("left stick parameter"),
        resting_left
    );
    assert_eq!(
        read(&renderer, "CatParamLeftHandDown").expect("left paw parameter"),
        resting_paw
    );

    // The stick button alone also shows the stick, on its own side.
    renderer
        .evaluate(
            ModelInputSnapshot {
                stick_right_down: true,
                ..ModelInputSnapshot::default()
            },
            Duration::from_millis(3),
        )
        .expect("stick button frame");
    assert_eq!(
        read(&renderer, "CatParamStickShowLeftHand").expect("left stick parameter"),
        resting_left,
        "the right stick button must not raise the left stick"
    );
    assert_eq!(
        read(&renderer, "CatParamRightHandDown").expect("right paw parameter"),
        1.0,
        "a held right stick button must press the right paw"
    );
    assert_eq!(
        read(&renderer, "CatParamStickLeftDown").expect("left stick button parameter"),
        0.0,
        "the right stick button must not raise the left stick's own parameter"
    );

    // A model with no stick of its own keeps its paws where its own keys put
    // them: a stick on a keyboard model moves nothing at all.
    let mut keyboard = activate("keyboard");
    keyboard
        .evaluate(moved, Duration::ZERO)
        .expect("displaced frame on the keyboard preset");
    assert_eq!(
        read(&keyboard, "CatParamStickShowLeftHand"),
        None,
        "the keyboard preset declares no left stick"
    );
    assert_eq!(
        read(&keyboard, "CatParamLeftHandDown").expect("left paw parameter"),
        0.0,
        "a stick must not press a paw on a model that has no stick to show"
    );
}

/// The complete stick matrix: every one of the six analog axes plus both stick
/// clicks, on the real bundled `gamepad` model, each asserted on **every** Live2D
/// parameter a stick can reach.
///
/// The point is that no control may be missing, shifted onto a neighbour, or
/// leaking into the other side. Each row moves exactly one control and checks all
/// ten stick-and-paw parameters, so a mapping that sent the left stick's X onto
/// `CatParamStickLY`, or left the right stick's Y unwritten, fails here instead of
/// being something a user notices.
#[test]
fn every_stick_control_reaches_its_own_parameter_and_nothing_else() {
    /// The ten parameters a stick can move, in the order they are checked.
    const STICK_PARAMETERS: [&str; 10] = [
        "CatParamStickLX",
        "CatParamStickLY",
        "CatParamStickRX",
        "CatParamStickRY",
        "CatParamStickLeftDown",
        "CatParamStickRightDown",
        "CatParamStickShowLeftHand",
        "CatParamStickShowRightHand",
        "CatParamLeftHandDown",
        "CatParamRightHandDown",
    ];

    let (bootstrap, _consumer) = RuntimeRenderer::channel();
    let mut renderer = RuntimeRenderer::start(bootstrap);
    let token = renderer
        .prepare(1, &preset_model("gamepad"), ModelInputSnapshot::default())
        .expect("prepare model");
    assert!(renderer.commit(token));

    fn values(renderer: &RuntimeRenderer) -> Vec<f32> {
        let model = &renderer.active.as_ref().expect("active model").model;
        STICK_PARAMETERS
            .iter()
            .map(|id| {
                model
                    .parameter_value_by_id(id)
                    .expect("parameter value")
                    .unwrap_or_else(|| panic!("{id} must exist on the bundled gamepad model"))
            })
            .collect()
    }

    /// One row of the matrix: one control, and the eleven values it must leave
    /// at rest plus the one it must move.
    fn check(
        renderer: &mut RuntimeRenderer,
        label: &str,
        input: ModelInputSnapshot,
        moved: &[usize],
    ) {
        let resting = {
            renderer
                .evaluate(ModelInputSnapshot::default(), Duration::ZERO)
                .expect("resting frame");
            values(renderer)
        };
        renderer
            .evaluate(input, Duration::from_millis(1))
            .expect("stick frame");
        let moved_values = values(renderer);
        for (index, (rest, moved_value)) in resting.iter().zip(&moved_values).enumerate() {
            let parameter = STICK_PARAMETERS[index];
            if moved.contains(&index) {
                assert!(
                    (moved_value - rest).abs() > f32::EPSILON,
                    "{label} must move {parameter}: {rest} then {moved_value}"
                );
            } else {
                assert_eq!(
                    moved_value, rest,
                    "{label} must leave {parameter} alone, but it moved to {moved_value}"
                );
            }
        }
    }

    const LEFT_X: usize = 0;
    const LEFT_Y: usize = 1;
    const RIGHT_X: usize = 2;
    const RIGHT_Y: usize = 3;
    const LEFT_DOWN: usize = 4;
    const RIGHT_DOWN: usize = 5;
    const SHOW_LEFT: usize = 6;
    const SHOW_RIGHT: usize = 7;
    const LEFT_PAW: usize = 8;
    const RIGHT_PAW: usize = 9;

    // Each axis moves its own position parameter, raises its own stick's
    // visibility and its own paw, and touches nothing else — not even the other
    // axis of the same stick, which is the misalignment that matters most.
    check(
        &mut renderer,
        "left stick X",
        ModelInputSnapshot {
            stick_left_x: 1.0,
            ..ModelInputSnapshot::default()
        },
        &[LEFT_X, SHOW_LEFT, LEFT_PAW],
    );
    check(
        &mut renderer,
        "left stick Y",
        ModelInputSnapshot {
            stick_left_y: -1.0,
            ..ModelInputSnapshot::default()
        },
        &[LEFT_Y, SHOW_LEFT, LEFT_PAW],
    );
    check(
        &mut renderer,
        "right stick X",
        ModelInputSnapshot {
            stick_right_x: 1.0,
            ..ModelInputSnapshot::default()
        },
        &[RIGHT_X, SHOW_RIGHT, RIGHT_PAW],
    );
    check(
        &mut renderer,
        "right stick Y",
        ModelInputSnapshot {
            stick_right_y: -1.0,
            ..ModelInputSnapshot::default()
        },
        &[RIGHT_Y, SHOW_RIGHT, RIGHT_PAW],
    );

    // A click shows the stick and presses its paw without claiming to be a
    // position: no XY parameter may move for a button press.
    check(
        &mut renderer,
        "left stick click",
        ModelInputSnapshot {
            stick_left_down: true,
            ..ModelInputSnapshot::default()
        },
        &[LEFT_DOWN, SHOW_LEFT, LEFT_PAW],
    );
    check(
        &mut renderer,
        "right stick click",
        ModelInputSnapshot {
            stick_right_down: true,
            ..ModelInputSnapshot::default()
        },
        &[RIGHT_DOWN, SHOW_RIGHT, RIGHT_PAW],
    );

    // Both sticks at once, because a model with two of them has to be able to
    // show both and press both paws at the same time.
    check(
        &mut renderer,
        "both sticks",
        ModelInputSnapshot {
            stick_left_x: -1.0,
            stick_left_y: 1.0,
            stick_right_x: 1.0,
            stick_right_y: -1.0,
            stick_right_down: true,
            ..ModelInputSnapshot::default()
        },
        &[
            LEFT_X, LEFT_Y, RIGHT_X, RIGHT_Y, RIGHT_DOWN, SHOW_LEFT, SHOW_RIGHT, LEFT_PAW,
            RIGHT_PAW,
        ],
    );

    // An analog trigger reaches no stick parameter at all: it is its own button
    // with its own image, and nothing on the model reads a continuous trigger
    // value. This row is what would break if the trigger were ever folded into a
    // stick by mistake.
    check(
        &mut renderer,
        "both triggers",
        ModelInputSnapshot {
            left_trigger: 1.0,
            right_trigger: 1.0,
            ..ModelInputSnapshot::default()
        },
        &[],
    );

    // Releasing everything returns every one of the ten to its resting value in
    // the same frame, so a released stick cannot leave artwork or a pressed paw.
    renderer
        .evaluate(ModelInputSnapshot::default(), Duration::from_millis(2))
        .expect("released frame");
    let released = values(&renderer);
    renderer
        .evaluate(ModelInputSnapshot::default(), Duration::from_millis(3))
        .expect("resting frame");
    assert_eq!(
        values(&renderer),
        released,
        "releasing every stick control must restore every stick parameter"
    );
}

/// The two stick axes and the stick button are three separate facts. The XY
/// parameters carry the position and the `*Down` parameter carries only the
/// click, so a projection that collapsed them would make a gamepad look like it
/// had one control where it has three.
#[test]
fn the_stick_axes_and_the_stick_button_are_projected_independently() {
    let (bootstrap, _consumer) = RuntimeRenderer::channel();
    let mut renderer = RuntimeRenderer::start(bootstrap);
    let token = renderer
        .prepare(1, &preset_model("gamepad"), ModelInputSnapshot::default())
        .expect("prepare model");
    assert!(renderer.commit(token));

    fn read(renderer: &RuntimeRenderer, id: &str) -> f32 {
        renderer
            .active
            .as_ref()
            .expect("active model")
            .model
            .parameter_value_by_id(id)
            .expect("parameter value")
            .expect("supported parameter")
    }

    renderer
        .evaluate(
            ModelInputSnapshot {
                stick_right_y: -0.5,
                ..ModelInputSnapshot::default()
            },
            Duration::ZERO,
        )
        .expect("stick frame");
    let displaced_right_y = read(&renderer, "CatParamStickRY");
    renderer
        .evaluate(ModelInputSnapshot::default(), Duration::from_millis(1))
        .expect("resting frame");
    let neutral_right_y = read(&renderer, "CatParamStickRY");
    assert!(
        (displaced_right_y - neutral_right_y).abs() > f32::EPSILON,
        "the right stick's Y axis must reach CatParamStickRY: {neutral_right_y} then {displaced_right_y}"
    );
    assert_eq!(
        read(&renderer, "CatParamStickLY"),
        neutral_right_y,
        "the right stick must not reach the left stick's Y parameter"
    );
    assert_eq!(
        read(&renderer, "CatParamStickRightDown"),
        0.0,
        "moving a stick is not pressing its button"
    );
    assert_eq!(
        read(&renderer, "CatParamStickLeftDown"),
        0.0,
        "the left stick button stays up while only the right stick moves"
    );
}
