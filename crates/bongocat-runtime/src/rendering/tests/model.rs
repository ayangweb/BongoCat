//! The settings that decide what the model tracks.

use super::*;

#[test]
fn model_settings_control_pointer_tracking_and_render_mirroring() {
    let (bootstrap, consumer) = RuntimeRenderer::channel();
    let mut renderer = RuntimeRenderer::start(bootstrap);
    renderer.set_model_settings(ModelSettings {
        mirror: true,
        mirror_pointer_tracking: true,
        ignore_keyboard: false,
        ignore_gamepad: false,
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
        mirror_pointer_tracking: false,
        ignore_keyboard: false,
        ignore_gamepad: false,
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
