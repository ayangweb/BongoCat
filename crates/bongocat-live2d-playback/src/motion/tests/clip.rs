//! What a motion looks like at an instant, and at its natural end.

use super::*;

#[test]
fn evaluates_all_segment_kinds_and_natural_completion() {
    let json = br#"{
      "Version":3,
      "Meta":{"Duration":4.0,"Fps":30.0,"Loop":false,"AreBeziersRestricted":false,
        "CurveCount":1,"TotalSegmentCount":4,"TotalPointCount":7,
        "UserDataCount":0,"TotalUserDataSize":0},
      "Curves":[{"Target":"Parameter","Id":"ParamTest","Segments":[
        0,0, 0,1,1, 1,1.25,1,1.75,3,2,4, 2,3,7, 3,4,9
      ]}]
    }"#;
    let clip = MotionClip::from_slice(json, 0.0, 0.0).expect("synthetic motion");
    assert!((clip.evaluate(Duration::from_millis(500)).parameters[0].value - 0.5).abs() < 0.001);
    let bezier = clip.evaluate(Duration::from_millis(1500)).parameters[0].value;
    assert!((1.0..4.0).contains(&bezier));
    assert_eq!(
        clip.evaluate(Duration::from_millis(2500)).parameters[0].value,
        4.0
    );
    assert_eq!(
        clip.evaluate(Duration::from_millis(3500)).parameters[0].value,
        9.0
    );
    assert!(clip.evaluate(Duration::from_secs(4)).finished);
}

#[test]
fn applies_sine_fade_and_wraps_loop_time() {
    let json = br#"{
      "Version":3,
      "Meta":{"Duration":2.0,"Fps":30.0,"Loop":true,"AreBeziersRestricted":true,
        "CurveCount":1,"TotalSegmentCount":1,"TotalPointCount":2,
        "UserDataCount":0,"TotalUserDataSize":0},
      "Curves":[{"Target":"Parameter","Id":"ParamTest","Segments":[0,0, 0,2,1]}]
    }"#;
    let clip = MotionClip::from_slice(json, 0.0, 0.0).expect("synthetic motion");
    let at_start = clip.evaluate(Duration::ZERO);
    assert_eq!(at_start.parameters[0].weight, 1.0);
    let wrapped = clip.evaluate(clip.duration() + Duration::from_millis(100));
    let initial = clip.evaluate(Duration::from_millis(100));
    assert!((wrapped.parameters[0].value - initial.parameters[0].value).abs() < 0.001);
}

#[test]
fn explicit_fade_out_preserves_the_first_frame_and_reaches_zero() {
    let json = br#"{
      "Version":3,
      "Meta":{"Duration":2.0,"Fps":30.0,"Loop":true,"AreBeziersRestricted":true,
        "CurveCount":1,"TotalSegmentCount":1,"TotalPointCount":2,
        "UserDataCount":0,"TotalUserDataSize":0},
      "Curves":[{"Target":"Parameter","Id":"P","Segments":[0,1,0,2,1]}]
    }"#;
    let clip = MotionClip::from_slice(json, 0.0, 1.0).expect("synthetic motion");

    assert_eq!(clip.fade_out_duration(), Duration::from_secs(1));
    assert_eq!(clip.fade_out_weight(Duration::ZERO), 1.0);
    assert!((clip.fade_out_weight(Duration::from_millis(500)) - 0.5).abs() < 0.0001);
    assert_eq!(clip.fade_out_weight(Duration::from_secs(1)), 0.0);
    assert_eq!(clip.fade_out_weight(Duration::from_secs(2)), 0.0);
}

#[test]
fn evaluates_part_opacity_separately_from_weighted_parameters() {
    let json = br#"{
      "Version":3,
      "Meta":{"Duration":1.0,"Fps":30.0,"Loop":false,"AreBeziersRestricted":true,
        "CurveCount":2,"TotalSegmentCount":2,"TotalPointCount":4,
        "UserDataCount":0,"TotalUserDataSize":0},
      "Curves":[
        {"Target":"Parameter","Id":"Param","Segments":[0,0,0,1,1]},
        {"Target":"PartOpacity","Id":"Part","Segments":[0,1,0,1,0]}
      ]
    }"#;
    let clip = MotionClip::from_slice(json, 0.5, 0.5).expect("synthetic motion");
    let evaluation = clip.evaluate(Duration::from_millis(500));

    assert_eq!(evaluation.parameters.len(), 1);
    assert_eq!(evaluation.parameters[0].id, "Param");
    assert_eq!(evaluation.part_opacities.len(), 1);
    assert_eq!(evaluation.part_opacities[0].id, "Part");
    assert!((evaluation.part_opacities[0].value - 0.5).abs() < 0.0001);
}

#[test]
fn evaluates_known_model_curves_separately_and_uses_the_last_value() {
    let json = br#"{
      "Version":3,
      "Meta":{"Duration":1.0,"Fps":30.0,"Loop":false,"AreBeziersRestricted":true,
        "CurveCount":5,"TotalSegmentCount":5,"TotalPointCount":10,
        "UserDataCount":0,"TotalUserDataSize":0},
      "Curves":[
        {"Target":"Model","Id":"EyeBlink","Segments":[0,0.1,0,1,0.1]},
        {"Target":"Model","Id":"LipSync","Segments":[0,0.2,0,1,0.2]},
        {"Target":"Model","Id":"Opacity","Segments":[0,0.4,0,1,0.4]},
        {"Target":"Model","Id":"Unknown","Segments":[0,9,0,1,9]},
        {"Target":"Model","Id":"EyeBlink","Segments":[0,0.5,0,1,0.5]}
      ]
    }"#;
    let clip = MotionClip::from_slice(json, 0.0, 0.0).expect("model curves");
    let evaluation = clip.evaluate(Duration::from_millis(500));

    assert_eq!(
        evaluation.model,
        MotionModelSample {
            eye_blink: Some(0.5),
            lip_sync: Some(0.2),
            opacity: Some(0.4),
            effect_weight: 1.0,
        }
    );
    assert!(evaluation.parameters.is_empty());
    assert!(evaluation.part_opacities.is_empty());
}

#[test]
fn one_shot_terminal_evaluation_keeps_natural_fade_weights() {
    let json = br#"{
      "Version":3,
      "Meta":{"Duration":1.0,"Fps":30.0,"Loop":false,"AreBeziersRestricted":true,
        "CurveCount":1,"TotalSegmentCount":1,"TotalPointCount":2,
        "UserDataCount":0,"TotalUserDataSize":0},
      "Curves":[
        {"Target":"Parameter","Id":"Param","Segments":[0,0,0,1,1]}
      ]
    }"#;
    let clip = MotionClip::from_slice(json, 0.5, 0.5).expect("fading motion");
    let terminal = clip.evaluate_once(Duration::from_secs(1));

    assert!(terminal.finished);
    assert_eq!(terminal.local_time, Duration::from_secs(1));
    assert_eq!(terminal.model.effect_weight, 0.0);
    let parameter = terminal
        .parameters
        .first()
        .expect("terminal parameter sample");
    assert_eq!(parameter.value, 1.0);
    assert_eq!(
        parameter.weight, 0.0,
        "completion freezes the fully evaluated duration sample, including its natural fade"
    );
}
