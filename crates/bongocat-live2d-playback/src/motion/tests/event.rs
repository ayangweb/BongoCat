//! What a motion asks the host to do, in order, and without repeating.

use super::*;

#[test]
fn user_data_crossings_are_ordered_non_repeating_and_bounded() {
    let json = br#"{
      "Version":3,
      "Meta":{"Duration":2.0,"Fps":30.0,"Loop":true,"AreBeziersRestricted":true,
        "CurveCount":0,"TotalSegmentCount":0,"TotalPointCount":0,
        "UserDataCount":3,"TotalUserDataSize":3},
      "Curves":[],
      "UserData":[
        {"Time":0.0,"Value":"A"},
        {"Time":0.5,"Value":"B"},
        {"Time":2.0,"Value":"C"}
      ]
    }"#;
    let clip = MotionClip::from_slice(json, 0.0, 0.0).expect("user data motion");
    assert_eq!(clip.user_data().len(), 3);

    let at_start = clip.user_data_events_between(None, Duration::ZERO);
    assert_eq!(at_start.occurrences[0].value, "A");
    assert_eq!(at_start.occurrences[0].cycle, 0);
    let first_half =
        clip.user_data_events_between(Some(Duration::ZERO), Duration::from_millis(500));
    assert_eq!(
        first_half
            .occurrences
            .iter()
            .map(|event| event.value.as_str())
            .collect::<Vec<_>>(),
        ["B"]
    );
    let wrap =
        clip.user_data_events_between(Some(Duration::from_millis(500)), Duration::from_secs(2));
    assert_eq!(
        wrap.occurrences
            .iter()
            .map(|event| (event.value.as_str(), event.cycle))
            .collect::<Vec<_>>(),
        [("C", 0), ("A", 1)]
    );

    let bounded = clip.user_data_events_between(Some(Duration::ZERO), Duration::from_secs(1_000));
    assert_eq!(
        bounded.occurrences.len(),
        MAX_USER_DATA_OCCURRENCES_PER_EVALUATION
    );
    assert_eq!(bounded.skipped_occurrences, 1_244);
    assert!(
        clip.user_data_events_between(Some(Duration::from_secs(3)), Duration::from_secs(2),)
            .occurrences
            .is_empty()
    );
}

#[test]
fn one_shot_user_data_uses_effective_playback_mode() {
    let json = br#"{
      "Version":3,
      "Meta":{"Duration":2.0,"Fps":30.0,"Loop":true,"AreBeziersRestricted":true,
        "CurveCount":0,"TotalSegmentCount":0,"TotalPointCount":0,
        "UserDataCount":1,"TotalUserDataSize":5},
      "Curves":[],
      "UserData":[{"Time":0.0,"Value":"start"}]
    }"#;
    let clip = MotionClip::from_slice(json, 0.0, 0.0).expect("user data motion");

    let one_shot = clip.user_data_events_between_with_looping(None, Duration::from_secs(2), false);
    assert_eq!(
        one_shot
            .occurrences
            .iter()
            .map(|event| (event.value.as_str(), event.cycle))
            .collect::<Vec<_>>(),
        [("start", 0)]
    );
    assert_eq!(one_shot.skipped_occurrences, 0);
    assert!(
        clip.user_data_events_between_with_looping(
            Some(Duration::ZERO),
            Duration::from_secs(2),
            false,
        )
        .occurrences
        .is_empty(),
        "a one-shot must not reinterpret its start timestamp as a loop boundary"
    );

    let looping = clip.user_data_events_between_with_looping(
        Some(Duration::ZERO),
        Duration::from_secs(2),
        true,
    );
    assert_eq!(
        looping
            .occurrences
            .iter()
            .map(|event| (event.value.as_str(), event.cycle))
            .collect::<Vec<_>>(),
        [("start", 1)]
    );
}
