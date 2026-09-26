//! A motion that cannot be played is refused, not clamped.

use super::*;

#[test]
fn rejects_bad_meta_and_truncated_segments() {
    let bad_count = br#"{
      "Version":3,
      "Meta":{"Duration":1.0,"Fps":30.0,"Loop":false,"AreBeziersRestricted":true,
        "CurveCount":2,"TotalSegmentCount":1,"TotalPointCount":2,
        "UserDataCount":0,"TotalUserDataSize":0},
      "Curves":[{"Target":"Parameter","Id":"P","Segments":[0,0,0,1,1]}]
    }"#;
    assert_eq!(
        MotionClip::from_slice(bad_count, 0.0, 0.0)
            .expect_err("bad count")
            .code,
        PlaybackErrorCode::MotionInvalid
    );

    let truncated = br#"{
      "Version":3,
      "Meta":{"Duration":1.0,"Fps":30.0,"Loop":false,"AreBeziersRestricted":true,
        "CurveCount":1,"TotalSegmentCount":1,"TotalPointCount":4,
        "UserDataCount":0,"TotalUserDataSize":0},
      "Curves":[{"Target":"Parameter","Id":"P","Segments":[0,0,1,0.2,0.3]}]
    }"#;
    assert_eq!(
        MotionClip::from_slice(truncated, 0.0, 0.0)
            .expect_err("truncated segment")
            .code,
        PlaybackErrorCode::MotionInvalid
    );
}
