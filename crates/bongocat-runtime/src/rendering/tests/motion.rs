//! A finished motion holds its terminal values rather than springing back.

use super::*;

#[test]
fn completed_motion_holds_the_post_natural_fade_terminal_evaluation() {
    let (bootstrap, _consumer) = RuntimeRenderer::channel();
    let mut renderer = RuntimeRenderer::start(bootstrap);
    let token = renderer
        .prepare(1, &preset_model("standard"), ModelInputSnapshot::default())
        .expect("prepare model");
    assert!(renderer.commit(token));
    let mouth_default = renderer
        .active
        .as_ref()
        .expect("active model")
        .model
        .parameter_value_by_id("ParamMouthOpenY")
        .expect("mouth parameter")
        .expect("supported parameter");

    let motion = MotionClip::from_slice(
        br#"{
          "Version":3,
          "Meta":{"Duration":1.0,"Fps":30.0,"Loop":false,"AreBeziersRestricted":true,
            "CurveCount":2,"TotalSegmentCount":2,"TotalPointCount":4,
            "UserDataCount":0,"TotalUserDataSize":0},
          "Curves":[
            {"Target":"Parameter","Id":"ParamMouthOpenY","Segments":[0,0,0,1,1]},
            {"Target":"PartOpacity","Id":"Part","Segments":[0,0,0,1,0.25]}
          ]
        }"#,
        0.0,
        1.0,
    )
    .expect("fading motion");
    renderer.active.as_mut().expect("active model").motion = Some(MotionPlayback {
        clip: motion,
        looping: false,
        started_at: Duration::ZERO,
        completed: false,
        fade_out_started_at: None,
        last_event_elapsed: None,
    });

    for now in [Duration::from_secs(2), Duration::from_secs(3)] {
        renderer
            .evaluate(ModelInputSnapshot::default(), now)
            .expect("completed fading motion frame");
        let active = renderer.active.as_ref().expect("active model");
        assert!(
            active
                .motion
                .as_ref()
                .is_some_and(|playback| playback.completed)
        );
        assert_eq!(
            active
                .model
                .parameter_value_by_id("ParamMouthOpenY")
                .expect("mouth parameter")
                .expect("supported parameter"),
            mouth_default,
            "the held sample includes the resource's completed natural fade"
        );
        assert_eq!(
            active
                .model
                .part_opacity_by_id("Part")
                .expect("part opacity")
                .expect("supported part"),
            0.25,
            "PartOpacity keeps its independent R5 sink value"
        );
    }
}

#[test]
fn completed_motion_holds_its_terminal_parameters_until_stopped() {
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
            "CurveCount":2,"TotalSegmentCount":2,"TotalPointCount":4,
            "UserDataCount":1,"TotalUserDataSize":5},
          "Curves":[
            {"Target":"Parameter","Id":"Param","Segments":[0,0,0,1,1]},
            {"Target":"PartOpacity","Id":"Part","Segments":[0,0,0,1,0.25]}
          ],
          "UserData":[{"Time":0.0,"Value":"start"}]
        }"#,
        0.0,
        0.0,
    )
    .expect("motion");
    renderer.active.as_mut().expect("active model").motion = Some(MotionPlayback {
        clip: motion,
        looping: false,
        started_at: Duration::ZERO,
        completed: false,
        fade_out_started_at: None,
        last_event_elapsed: None,
    });

    for (now, expected_user_data_events, expected_value, expected_part, settled) in [
        (Duration::ZERO, 1, 0.0, 0.0, false),
        (Duration::from_secs(2), 0, 1.0, 0.25, true),
        (Duration::from_secs(3), 0, 1.0, 0.25, true),
        (Duration::from_secs(1), 0, 1.0, 0.25, true),
    ] {
        let evaluation = renderer
            .evaluate(ModelInputSnapshot::default(), now)
            .expect("completed motion frame");
        assert!(
            !evaluation.motion_finished,
            "natural completion must keep the motion layer current"
        );
        assert_eq!(
            evaluation.motion_user_data.len(),
            expected_user_data_events,
            "one-shot UserData must follow the effective playback mode exactly once"
        );
        assert_eq!(renderer.motion_is_settled(now), settled);
        let active = renderer.active.as_ref().expect("active model");
        let value = active
            .model
            .parameter_value_by_id("Param")
            .expect("parameter value")
            .expect("supported parameter");
        assert!(
            (value - expected_value).abs() < 0.0001,
            "the evaluated motion parameter must be reapplied after defaults at {now:?}: {value}"
        );
        let part_opacity = active
            .model
            .part_opacity_by_id("Part")
            .expect("part opacity")
            .expect("supported part");
        assert!(
            (part_opacity - expected_part).abs() < 0.0001,
            "the completed PartOpacity sample must remain current at {now:?}: {part_opacity}"
        );
    }

    let part_opacity_before_stop = {
        let active = renderer.active.as_ref().expect("active model");
        active
            .model
            .part_opacity_by_id("Part")
            .expect("current part opacity")
            .expect("supported part")
    };
    assert!(part_opacity_before_stop < 1.0);
    assert_eq!(
        renderer.stop_motion(Duration::from_secs(3)),
        MotionStopStatus::Finished
    );
    assert!(
        renderer
            .active
            .as_ref()
            .expect("active model")
            .motion
            .is_none()
    );
    renderer
        .evaluate(ModelInputSnapshot::default(), Duration::from_secs(4))
        .expect("frame after zero-duration stop");
    assert!(
        renderer
            .active
            .as_ref()
            .expect("active model")
            .model
            .part_opacity_by_id("Part")
            .expect("part opacity after stop")
            .expect("supported part")
            > 0.99,
        "stopping a motion must restore Core part opacity before the next layer"
    );
}

#[test]
fn hidden_motion_fade_becomes_settled_without_frame_evaluation() {
    let (bootstrap, _consumer) = RuntimeRenderer::channel();
    let mut renderer = RuntimeRenderer::start(bootstrap);
    let token = renderer
        .prepare(1, &preset_model("standard"), ModelInputSnapshot::default())
        .expect("prepare model");
    assert!(renderer.commit(token));

    let motion = MotionClip::from_slice(
        br#"{
          "Version":3,
          "Meta":{"Duration":10.0,"Fps":30.0,"Loop":false,
            "AreBeziersRestricted":true,"CurveCount":1,"TotalSegmentCount":1,
            "TotalPointCount":2,"UserDataCount":0,"TotalUserDataSize":0},
          "Curves":[
            {"Target":"Parameter","Id":"Param","Segments":[0,0,0,1,1]}
          ]
        }"#,
        0.0,
        1.0,
    )
    .expect("motion");
    renderer.active.as_mut().expect("active model").motion = Some(MotionPlayback {
        clip: motion,
        looping: false,
        started_at: Duration::ZERO,
        completed: false,
        fade_out_started_at: None,
        last_event_elapsed: None,
    });

    assert_eq!(
        renderer.stop_motion(Duration::from_secs(1)),
        MotionStopStatus::Fading
    );
    assert!(!renderer.motion_is_settled(Duration::from_secs(1)));
    assert!(renderer.motion_is_settled(Duration::from_secs(2)));
}
