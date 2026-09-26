//! The curve, and the clip that holds them.
//!
//! A clip evaluates at an instant and over a range. A one-shot holds its terminal
//! sample after the clip's natural end rather than dropping to the model's
//! defaults, because a model that visibly springs back the moment its motion
//! finishes is a model a user describes as glitching. The user-data crossings are
//! collected from the same range, so what the host is told and what is drawn come
//! from one evaluation rather than two that could disagree.

use super::*;

impl MotionClip {
    pub fn duration(&self) -> Duration {
        Duration::from_secs_f32(self.duration_seconds)
    }
}

impl MotionClip {
    pub fn is_looping(&self) -> bool {
        self.looping
    }
}

impl MotionClip {
    pub fn fade_out_duration(&self) -> Duration {
        Duration::from_secs_f32(self.fade_out_seconds)
    }
}

impl MotionClip {
    pub fn fade_out_weight(&self, elapsed: Duration) -> f32 {
        1.0 - fade_weight(elapsed.as_secs_f32(), self.fade_out_seconds)
    }
}

impl MotionClip {
    pub fn user_data(&self) -> &[MotionUserDataEvent] {
        &self.user_data
    }
}

impl MotionClip {
    pub fn user_data_events_between(
        &self,
        previous_elapsed: Option<Duration>,
        elapsed: Duration,
    ) -> MotionUserDataEvaluation {
        self.user_data_events_between_with_looping(previous_elapsed, elapsed, self.looping)
    }
}

impl MotionClip {
    /// Evaluates crossings using the effective playback mode rather than the
    /// clip's authored `Meta.Loop` value. Product motions may play a looping
    /// asset exactly once.
    pub fn user_data_events_between_with_looping(
        &self,
        previous_elapsed: Option<Duration>,
        elapsed: Duration,
        looping: bool,
    ) -> MotionUserDataEvaluation {
        if self.user_data.is_empty() || previous_elapsed.is_some_and(|previous| elapsed < previous)
        {
            return MotionUserDataEvaluation::default();
        }

        #[derive(Clone, Copy)]
        struct Candidate {
            absolute_time: f64,
            source_index: usize,
            cycle: u64,
        }

        let previous_seconds = previous_elapsed.map(|value| value.as_secs_f64());
        let elapsed_seconds = elapsed.as_secs_f64();
        let duration_seconds = f64::from(self.duration_seconds);
        let mut total_occurrences = 0u64;
        let mut candidates = Vec::new();

        for (source_index, event) in self.user_data.iter().enumerate() {
            let event_seconds = event.local_time.as_secs_f64();
            if !looping || duration_seconds <= 0.0 {
                let follows_previous = previous_seconds
                    .is_none_or(|previous| event_seconds > previous + f64::from(TIME_TOLERANCE));
                if follows_previous && event_seconds <= elapsed_seconds + f64::from(TIME_TOLERANCE)
                {
                    total_occurrences = total_occurrences.saturating_add(1);
                    candidates.push(Candidate {
                        absolute_time: event_seconds,
                        source_index,
                        cycle: 0,
                    });
                }
                continue;
            }

            let first_cycle = previous_seconds.map_or(0, |previous| {
                (((previous - event_seconds) / duration_seconds).floor() + 1.0).max(0.0) as u64
            });
            let last_cycle = ((elapsed_seconds + f64::from(TIME_TOLERANCE) - event_seconds)
                / duration_seconds)
                .floor();
            if last_cycle < 0.0 || last_cycle < first_cycle as f64 {
                continue;
            }
            let last_cycle = last_cycle as u64;
            let occurrence_count = last_cycle.saturating_sub(first_cycle).saturating_add(1);
            total_occurrences = total_occurrences.saturating_add(occurrence_count);
            for cycle in first_cycle
                ..=last_cycle.min(
                    first_cycle.saturating_add(MAX_USER_DATA_OCCURRENCES_PER_EVALUATION as u64 - 1),
                )
            {
                candidates.push(Candidate {
                    absolute_time: cycle as f64 * duration_seconds + event_seconds,
                    source_index,
                    cycle,
                });
            }
        }

        candidates.sort_by(|left, right| {
            left.absolute_time
                .total_cmp(&right.absolute_time)
                .then(left.cycle.cmp(&right.cycle))
                .then(left.source_index.cmp(&right.source_index))
        });
        candidates.truncate(MAX_USER_DATA_OCCURRENCES_PER_EVALUATION);
        let occurrences = candidates
            .into_iter()
            .map(|candidate| {
                let event = &self.user_data[candidate.source_index];
                MotionUserDataOccurrence {
                    cycle: candidate.cycle,
                    local_time: event.local_time,
                    value: event.value.clone(),
                }
            })
            .collect::<Vec<_>>();
        MotionUserDataEvaluation {
            skipped_occurrences: total_occurrences.saturating_sub(occurrences.len() as u64),
            occurrences,
        }
    }
}

impl MotionClip {
    pub fn evaluate(&self, elapsed: Duration) -> MotionEvaluation {
        self.evaluate_with_looping(elapsed, self.looping)
    }
}

impl MotionClip {
    pub fn evaluate_once(&self, elapsed: Duration) -> MotionEvaluation {
        self.evaluate_with_looping(elapsed, false)
    }
}

impl MotionClip {
    pub(crate) fn evaluate_with_looping(
        &self,
        elapsed: Duration,
        looping: bool,
    ) -> MotionEvaluation {
        let elapsed_seconds = elapsed.as_secs_f32();
        let finished = !looping && elapsed_seconds >= self.duration_seconds;
        let local_seconds = if looping && self.duration_seconds > 0.0 {
            elapsed_seconds.rem_euclid(self.duration_seconds)
        } else {
            elapsed_seconds.min(self.duration_seconds)
        };
        let mut model = MotionModelSample {
            effect_weight: motion_weight(
                elapsed_seconds,
                self.duration_seconds,
                looping,
                self.fade_in_seconds,
                self.fade_out_seconds,
            ),
            ..MotionModelSample::default()
        };
        for curve in self
            .curves
            .iter()
            .filter(|curve| curve.target == MotionCurveTarget::Model)
        {
            let value = curve.evaluate(local_seconds);
            match curve.id.as_str() {
                MODEL_EYE_BLINK_ID => model.eye_blink = Some(value),
                MODEL_LIP_SYNC_ID => model.lip_sync = Some(value),
                MODEL_OPACITY_ID => model.opacity = Some(value),
                _ => {}
            }
        }
        let parameters = self
            .curves
            .iter()
            .filter(|curve| curve.target == MotionCurveTarget::Parameter)
            .map(|curve| MotionParameterSample {
                id: curve.id.clone(),
                value: curve.evaluate(local_seconds),
                weight: curve.weight(
                    elapsed_seconds,
                    self.duration_seconds,
                    looping,
                    self.fade_in_seconds,
                    self.fade_out_seconds,
                ),
            })
            .collect();
        let part_opacities = self
            .curves
            .iter()
            .filter(|curve| curve.target == MotionCurveTarget::PartOpacity)
            .map(|curve| MotionPartOpacitySample {
                id: curve.id.clone(),
                value: curve.evaluate(local_seconds),
            })
            .collect();
        MotionEvaluation {
            local_time: Duration::from_secs_f32(local_seconds),
            finished,
            model,
            parameters,
            part_opacities,
        }
    }
}
