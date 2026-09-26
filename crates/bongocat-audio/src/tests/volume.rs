//! A volume is refused rather than clamped.

use super::*;

#[test]
fn volume_rejects_non_finite_and_out_of_range_values() {
    assert_eq!(MotionAudioVolume::new(0.0), Some(MotionAudioVolume(0.0)));
    assert_eq!(MotionAudioVolume::new(1.0), Some(MotionAudioVolume::FULL));
    assert_eq!(MotionAudioVolume::new(-0.1), None);
    assert_eq!(MotionAudioVolume::new(1.1), None);
    assert_eq!(MotionAudioVolume::new(f32::NAN), None);
}
