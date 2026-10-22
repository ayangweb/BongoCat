//! A small, locally synthesized cat-like vowel glide. No microphone input is used.

pub(crate) fn samples() -> Vec<f32> {
    const RATE: usize = 44_100;
    const LENGTH: usize = RATE * 18 / 100;
    let mut phase = 0.0_f32;
    (0..LENGTH)
        .map(|index| {
            let progress = index as f32 / (LENGTH - 1) as f32;
            let pitch = 720.0 - 370.0 * progress;
            phase += std::f32::consts::TAU * pitch / RATE as f32;
            let envelope = (progress * std::f32::consts::PI).sin().powi(2);
            let vowel = phase.sin()
                + 0.45 * (phase * 2.0).sin()
                + (0.35 * (1.0 - progress)) * (phase * 3.0).sin();
            0.22 * envelope * vowel
        })
        .collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn clip_is_short_finite_audible_and_fades_at_both_ends() {
        let samples = super::samples();
        assert_eq!(samples.len(), 7_938);
        assert!(samples.iter().all(|s| s.is_finite() && s.abs() < 0.5));
        assert!(samples.iter().any(|s| s.abs() > 0.1));
        assert!(samples.first().unwrap().abs() < 0.0001);
        assert!(samples.last().unwrap().abs() < 0.0001);
    }
}
