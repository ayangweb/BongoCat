//! One frame, in the order the user can see.

use super::*;

#[test]
fn frame_evaluation_order_is_motion_expression_input_effects_then_core_update() {
    let (bootstrap, _consumer) = RuntimeRenderer::channel();
    let mut renderer = RuntimeRenderer::start(bootstrap);
    let token = renderer
        .prepare(1, &preset_model("standard"), ModelInputSnapshot::default())
        .expect("prepare model");
    assert!(renderer.commit(token));

    let motion = MotionClip::from_slice(
        br#"{
          "Version":3,
          "Meta":{"Duration":1.0,"Fps":30.0,"Loop":true,"AreBeziersRestricted":true,
            "CurveCount":3,"TotalSegmentCount":3,"TotalPointCount":6,
            "UserDataCount":0,"TotalUserDataSize":0},
          "Curves":[
            {"Target":"Parameter","Id":"ParamEyeLOpen","Segments":[0,0.7,0,1,0.7]},
            {"Target":"Parameter","Id":"ParamMouthOpenY","Segments":[0,0.2,0,1,0.2]},
            {"Target":"Parameter","Id":"ParamAngleX","Segments":[0,20,0,1,20]}
          ]
        }"#,
        0.0,
        0.0,
    )
    .expect("motion");
    let expression = ExpressionClip::from_slice(
        br#"{
          "Type":"Live2D Expression","FadeInTime":0.0,"FadeOutTime":0.0,
          "Parameters":[
            {"Id":"ParamEyeROpen","Value":0.7,"Blend":"Overwrite"},
            {"Id":"ParamMouthOpenY","Value":0.8,"Blend":"Overwrite"},
            {"Id":"ParamAngleX","Value":10.0,"Blend":"Overwrite"}
          ]
        }"#,
    )
    .expect("expression");
    let active = renderer.active.as_mut().expect("active model");
    active.motion = Some(MotionPlayback {
        clip: motion,
        looping: true,
        started_at: Duration::ZERO,
        completed: false,
        fade_out_started_at: None,
        last_event_elapsed: None,
    });
    active.expressions.push(ExpressionPlayback {
        clip: expression,
        started_at: Duration::ZERO,
        fade_in_completed: false,
        fade_out_started_at: None,
    });

    renderer
        .evaluate(
            ModelInputSnapshot {
                pointer_x: -0.5,
                ..ModelInputSnapshot::default()
            },
            Duration::ZERO,
        )
        .expect("evaluate frame");
    let model = &renderer.active.as_ref().expect("active model").model;
    for (id, expected) in [
        ("ParamEyeLOpen", 0.0),
        ("ParamEyeROpen", 0.0),
        ("ParamMouthOpenY", 0.8),
        ("ParamAngleX", -15.0),
    ] {
        let actual = model
            .parameter_value_by_id(id)
            .expect("parameter value")
            .expect("supported parameter");
        assert!((actual - expected).abs() < 0.0001, "{id}: {actual}");
    }

    renderer
        .evaluate(
            ModelInputSnapshot {
                pointer_x: -0.5,
                ..ModelInputSnapshot::default()
            },
            Duration::from_secs(2),
        )
        .expect("expression persistence frame");
    let active = renderer.active.as_ref().expect("active model");
    assert_eq!(active.expressions.len(), 1);
    let mouth = active
        .model
        .parameter_value_by_id("ParamMouthOpenY")
        .expect("parameter value")
        .expect("supported parameter");
    assert!(
        (mouth - 0.8).abs() < 0.0001,
        "the latest expression must remain active after its fade-in completes: {mouth}"
    );
}

#[test]
fn latest_expression_stays_full_weight_after_clock_rollback() {
    let (bootstrap, _consumer) = RuntimeRenderer::channel();
    let mut renderer = RuntimeRenderer::start(bootstrap);
    let token = renderer
        .prepare(1, &preset_model("standard"), ModelInputSnapshot::default())
        .expect("prepare model");
    assert!(renderer.commit(token));
    let expression = ExpressionClip::from_slice(
        br#"{
          "Type":"Live2D Expression","FadeInTime":1.0,"FadeOutTime":1.0,
          "Parameters":[
            {"Id":"ParamMouthOpenY","Value":0.8,"Blend":"Overwrite"}
          ]
        }"#,
    )
    .expect("expression");
    renderer
        .active
        .as_mut()
        .expect("active model")
        .expressions
        .push(ExpressionPlayback {
            clip: expression,
            started_at: Duration::ZERO,
            fade_in_completed: false,
            fade_out_started_at: None,
        });

    for (now, expected) in [
        (Duration::from_secs(2), 0.8),
        (Duration::from_millis(500), 0.8),
    ] {
        renderer
            .evaluate(ModelInputSnapshot::default(), now)
            .expect("expression frame");
        let active = renderer.active.as_ref().expect("active model");
        assert!(
            active
                .expressions
                .first()
                .is_some_and(|playback| playback.fade_in_completed)
        );
        let mouth = active
            .model
            .parameter_value_by_id("ParamMouthOpenY")
            .expect("parameter value")
            .expect("supported parameter");
        assert!(
            (mouth - expected).abs() < 0.0001,
            "a completed expression fade-in must not restart after clock rollback: {mouth}"
        );
    }
}
