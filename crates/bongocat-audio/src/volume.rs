//! How loud, and the bound that says so.
//!
//! The value is a newtype so a volume cannot reach the backend unchecked: a
//! non-finite or out-of-range value is refused at construction rather than turned
//! into a silent gain of zero, which is what a clamp would do and what a user
//! turning the slider to the bottom would hear as a bug.

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MotionAudioVolume(pub(crate) f32);

impl MotionAudioVolume {
    pub const FULL: Self = Self(1.0);

    pub fn new(value: f32) -> Option<Self> {
        value
            .is_finite()
            .then_some(value)
            .filter(|value| (0.0..=1.0).contains(value))
            .map(Self)
    }

    pub const fn get(self) -> f32 {
        self.0
    }
}
