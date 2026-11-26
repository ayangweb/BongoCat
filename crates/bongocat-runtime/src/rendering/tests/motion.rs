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
    renderer.active.as_mut().expect("active model").motions = vec![MotionPlayback {
        motion: MotionId::new("CAT_motion", 0).expect("motion id"),
        clip: motion,
        looping: false,
        started_at: Duration::ZERO,
        completed: false,
        fade_out_started_at: None,
        last_event_elapsed: None,
    }];

    for now in [Duration::from_secs(2), Duration::from_secs(3)] {
        renderer
            .evaluate(ModelInputSnapshot::default(), now)
            .expect("completed fading motion frame");
        let active = renderer.active.as_ref().expect("active model");
        assert!(
            active
                .motions
                .last()
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
    renderer.active.as_mut().expect("active model").motions = vec![MotionPlayback {
        motion: MotionId::new("CAT_motion", 0).expect("motion id"),
        clip: motion,
        looping: false,
        started_at: Duration::ZERO,
        completed: false,
        fade_out_started_at: None,
        last_event_elapsed: None,
    }];

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
            evaluation.finished_motions.is_empty(),
            "natural completion must keep the motion layer current"
        );
        assert_eq!(
            evaluation.motion_user_data.len(),
            expected_user_data_events,
            "one-shot UserData must follow the effective playback mode exactly once"
        );
        assert_eq!(
            renderer.motion_is_settled(&MotionId::new("CAT_motion", 0).expect("motion id"), now),
            settled
        );
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
        renderer.stop_motion(
            &MotionId::new("CAT_motion", 0).expect("motion id"),
            Duration::from_secs(3)
        ),
        MotionStopStatus::Finished
    );
    assert!(
        renderer
            .active
            .as_ref()
            .expect("active model")
            .motions
            .is_empty()
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
    renderer.active.as_mut().expect("active model").motions = vec![MotionPlayback {
        motion: MotionId::new("CAT_motion", 0).expect("motion id"),
        clip: motion,
        looping: false,
        started_at: Duration::ZERO,
        completed: false,
        fade_out_started_at: None,
        last_event_elapsed: None,
    }];

    assert_eq!(
        renderer.stop_motion(
            &MotionId::new("CAT_motion", 0).expect("motion id"),
            Duration::from_secs(1)
        ),
        MotionStopStatus::Fading
    );
    assert!(!renderer.motion_is_settled(
        &MotionId::new("CAT_motion", 0).expect("motion id"),
        Duration::from_secs(1)
    ));
    assert!(renderer.motion_is_settled(
        &MotionId::new("CAT_motion", 0).expect("motion id"),
        Duration::from_secs(2)
    ));
}

#[allow(clippy::too_many_arguments)]
fn overlap_test_layer(
    group: &str,
    index: usize,
    parameter: &str,
    duration: f64,
    value: f64,
    started_at: Duration,
    fade_in: f32,
    fade_out: f32,
) -> MotionPlayback {
    let bytes = serde_json::to_vec(&serde_json::json!({
        "Version": 3,
        "Meta": {
            "Duration": duration, "Fps": 30.0, "Loop": false,
            "AreBeziersRestricted": true, "CurveCount": 1,
            "TotalSegmentCount": 1, "TotalPointCount": 2,
            "UserDataCount": 1, "TotalUserDataSize": group.len()
        },
        "Curves": [
            {"Target": "Parameter", "Id": parameter, "Segments": [0, 0, 0, duration, value]}
        ],
        "UserData": [{"Time": 0.25, "Value": group}]
    }))
    .expect("motion bytes");
    MotionPlayback {
        motion: MotionId::new(group, index).expect("motion identity"),
        clip: MotionClip::from_slice(&bytes, fade_in, fade_out).expect("motion clip"),
        looping: false,
        started_at,
        completed: false,
        fade_out_started_at: None,
        last_event_elapsed: None,
    }
}

#[test]
fn overlap_evaluates_independent_clocks_and_removes_only_the_faded_layer() {
    let (bootstrap, _consumer) = RuntimeRenderer::channel();
    let mut renderer = RuntimeRenderer::start(bootstrap);
    let token = renderer
        .prepare(1, &preset_model("standard"), ModelInputSnapshot::default())
        .expect("prepare model");
    assert!(renderer.commit(token));
    let short = overlap_test_layer("Hold", 0, "Param", 1.0, 1.0, Duration::ZERO, 0.0, 0.0);
    let long = overlap_test_layer(
        "Tap",
        0,
        "ParamMouthOpenY",
        4.0,
        1.0,
        Duration::from_secs(1),
        1.0,
        1.0,
    );
    let short_id = short.motion.clone();
    let long_id = long.motion.clone();
    renderer.active.as_mut().expect("active model").motions = vec![long, short];
    let frame = renderer
        .evaluate(ModelInputSnapshot::default(), Duration::from_millis(1500))
        .expect("overlap frame");
    let active = renderer.active.as_ref().expect("active model");
    assert!(active.motions[1].completed);
    assert!(!active.motions[0].completed);
    let parameter = |id| {
        active
            .model
            .parameter_value_by_id(id)
            .expect("parameter")
            .expect("supported parameter")
    };
    assert!((parameter("Param") - 1.0).abs() < 0.0001);
    assert!(
        (parameter("ParamMouthOpenY") - 0.0625).abs() < 0.0001,
        "the later layer has its own elapsed time and half fade-in weight"
    );
    assert_eq!(
        frame
            .motion_user_data
            .iter()
            .map(|event| &event.motion)
            .collect::<Vec<_>>(),
        vec![&long_id, &short_id]
    );
    assert!(
        renderer
            .evaluate(ModelInputSnapshot::default(), Duration::from_millis(1500))
            .expect("same clock frame")
            .motion_user_data
            .is_empty()
    );
    assert_eq!(
        renderer.stop_motion(&long_id, Duration::from_millis(1500)),
        MotionStopStatus::Fading
    );
    assert_eq!(
        renderer.stop_motion(&long_id, Duration::from_secs(2)),
        MotionStopStatus::Fading
    );
    assert_eq!(
        renderer.active.as_ref().expect("active model").motions[0].fade_out_started_at,
        Some(Duration::from_millis(1500))
    );
    let faded = renderer
        .evaluate(ModelInputSnapshot::default(), Duration::from_millis(2500))
        .expect("fade completion");
    assert_eq!(faded.finished_motions, vec![long_id]);
    let active = renderer.active.as_ref().expect("active model");
    assert_eq!(active.motions.len(), 1);
    assert_eq!(active.motions[0].motion, short_id);
    assert!(
        (active
            .model
            .parameter_value_by_id("Param")
            .expect("parameter")
            .expect("supported parameter")
            - 1.0)
            .abs()
            < 0.0001
    );
}

#[test]
fn overlapping_parameter_precedence_uses_start_order() {
    let (bootstrap, _consumer) = RuntimeRenderer::channel();
    let mut renderer = RuntimeRenderer::start(bootstrap);
    let token = renderer
        .prepare(1, &preset_model("standard"), ModelInputSnapshot::default())
        .expect("prepare model");
    assert!(renderer.commit(token));
    let orders = [
        [("Hold", 0, 0.2), ("Hold", 1, 0.4), ("Tap", 0, 0.8)],
        [("Tap", 0, 0.8), ("Hold", 1, 0.4), ("Hold", 0, 0.2)],
    ];
    for (order, expected) in [(orders[0], 0.8), (orders[1], 0.2)] {
        renderer.active.as_mut().expect("active model").motions = order
            .into_iter()
            .map(|(group, index, value)| {
                overlap_test_layer(group, index, "Param", 1.0, value, Duration::ZERO, 0.0, 0.0)
            })
            .collect();
        renderer
            .evaluate(ModelInputSnapshot::default(), Duration::from_secs(1))
            .expect("layered frame");
        let value = renderer
            .active
            .as_ref()
            .expect("active model")
            .model
            .parameter_value_by_id("Param")
            .expect("parameter")
            .expect("supported parameter");
        assert!(
            (value - expected).abs() < 0.0001,
            "the newest layer must win shared-parameter conflicts: {value}"
        );
    }
}

#[test]
fn overlapping_standard_model_groups_can_replay_after_terminal_poses_are_held() {
    let (bootstrap, _consumer) = RuntimeRenderer::channel();
    let mut renderer = RuntimeRenderer::start(bootstrap);
    renderer.set_model_settings(ModelSettings {
        allow_motion_overlap: true,
        ..ModelSettings::default()
    });
    let committed = preset_model("standard");
    let token = renderer
        .prepare(1, &committed, ModelInputSnapshot::default())
        .expect("prepare model");
    assert!(renderer.commit(token));
    let mut reference = Live2dModel::load(&committed).expect("reference model");

    for (step, group) in [
        "CAT_motion",
        "CAT_motion_lock",
        "CAT_motion",
        "CAT_motion_lock",
        "CAT_motion",
    ]
    .into_iter()
    .enumerate()
    {
        let started_at = Duration::from_secs(step as u64 * 3);
        // Both commands can share a clock sample: insertion order must still
        // resolve conflicts with older layers from the other group.
        for index in 0..2 {
            renderer
                .start_motion(
                    &MotionId::new(group, index).expect("motion identity"),
                    started_at,
                    false,
                )
                .expect("start preset motion");
        }
        let elapsed = Duration::from_millis(300);
        renderer
            .evaluate(ModelInputSnapshot::default(), started_at + elapsed)
            .expect("replay frame");
        reference
            .restore_parameter_defaults()
            .expect("reference defaults");
        for index in 0..2 {
            let clip = reference
                .motion_clip(group, index)
                .expect("reference clip")
                .clone();
            reference
                .apply_motion_once_with_weight(&clip, elapsed, 1.0)
                .expect("reference motion");
        }
        let active = renderer.active.as_ref().expect("active model");
        for id in ["Param", "Param2", "Param3"] {
            let actual = active
                .model
                .parameter_value_by_id(id)
                .expect("parameter")
                .expect("supported parameter");
            let expected = reference
                .parameter_value_by_id(id)
                .expect("reference")
                .expect("supported reference parameter");
            assert!(
                (actual - expected).abs() < 0.0001,
                "{group} replay {step}: {id} expected {expected}, got {actual}"
            );
        }
        assert_eq!(active.motions.len(), if step == 0 { 2 } else { 4 });
        renderer
            .evaluate(
                ModelInputSnapshot::default(),
                started_at + Duration::from_secs(3),
            )
            .expect("terminal frame");
        assert!(
            renderer
                .active
                .as_ref()
                .expect("active model")
                .motions
                .iter()
                .all(|playback| playback.completed)
        );
    }
}
