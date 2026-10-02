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
