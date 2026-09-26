//! The fixed reference targets the automatic effects drive.
//!
//! The reference breath is the Bongo-Cat-Mver behaviour, reproduced from fixed
//! parameter ids rather than from a model3 group: a compatible model uses the
//! conventional ids even when its index omits a Breath group, and a model that
//! has none of them still moves the ones it does have.

pub(crate) const MAX_MODEL_EFFECT_TARGETS: usize = 64;

// These are the fixed Cubism Framework breath parameters used by the
// Bongo-Cat-Mver reference. They are intentionally independent of model3
// groups: compatible models use the conventional IDs even when their model3
// omits a Breath group.
pub(crate) const REFERENCE_BREATH_TARGETS: [(&str, f32, f32, f32); 5] = [
    ("ParamAngleX", 0.0, 15.0, 6.5345),
    ("ParamAngleY", 0.0, 8.0, 3.5345),
    ("ParamAngleZ", 0.0, 10.0, 5.5345),
    ("ParamBodyAngleX", 0.0, 4.0, 15.5345),
    ("ParamBreath", 0.5, 0.5, 3.2345),
];

pub(crate) const AUTOMATIC_BREATH_CONTRIBUTION_WEIGHT: f32 = 0.5;

pub(crate) const GENERIC_BREATH_PERIOD: f32 = 4.0;
