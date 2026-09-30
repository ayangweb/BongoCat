//! The timer: what round is on screen, and what happens when it ends.
//!
//! Everything here is arithmetic and one small state machine, and both of them are the
//! plugin's alone. A pomodoro is not a countdown the host knows how to run — it is
//! rounds, a counter of how many have been finished, and a decision about what follows
//! each one — so the whole of it belongs in the plugin that draws it.
//!
//! The unit is the host's monotonic `elapsed_ms` rather than wall time, and the reason
//! is worth stating once: a timer driven by the wall clock jumps when the clock does,
//! and a user who changed their timezone mid-round did not ask for their session to lose
//! four minutes. Every number below is a difference between two elapsed readings.

/// What a round is for.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RoundKind {
    /// A stretch of work.
    Focus,
    /// A short pause.
    ShortBreak,
    /// A long pause.
    LongBreak,
}

impl RoundKind {
    /// Whether this round is work rather than a pause.
    ///
    /// The one thing that distinguishes a focus round from a break for the two rules
    /// that care: a finished focus round is what counts towards the next long break, and
    /// finishing one is worth telling the user about in different words.
    pub const fn is_focus(self) -> bool {
        matches!(self, Self::Focus)
    }
}

/// Where a round is in its own life.
///
/// A separate field rather than something derived, because "not running" and "over" are
/// different states with different consequences: a paused round still has time left and
/// an over one does not, and only the second one is a boundary worth reacting to.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Phase {
    /// Counting down.
    Counting,
    /// Stopped, with the time already served kept.
    Paused,
    /// Reached zero, and waiting for whatever comes next.
    Over,
}

/// The one round the panel is about.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Round {
    pub kind: RoundKind,
    /// How long the round lasts, in milliseconds.
    pub length_ms: u64,
    /// The session's elapsed reading when this round began counting.
    pub started_ms: u64,
    /// How many focus rounds have finished so far, which is what picks the long break.
    pub completed_rounds: u32,
    pub phase: Phase,
    /// When the round was paused, for the shift a resume has to apply.
    ///
    /// `None` whenever the round is not paused, so "is it running" is one check rather
    /// than a pair that has to agree.
    pub paused_at_ms: Option<u64>,
}

impl Round {
    /// A round that has not started counting yet.
    pub fn new(kind: RoundKind, length_ms: u64, started_ms: u64, completed_rounds: u32) -> Self {
        Self {
            kind,
            length_ms,
            started_ms,
            completed_rounds,
            phase: Phase::Counting,
            paused_at_ms: None,
        }
    }

    /// The time this round has used, never more than its length.
    ///
    /// Pausing is one line here rather than a flag every reader has to check: a paused
    /// round is measured up to the moment it was paused, because that reading does not
    /// move. So the countdown's arithmetic has one rule in every phase — it is always
    /// `from started_ms to now` — and pausing only changes which `now` is.
    fn used_ms(&self, elapsed_ms: u64) -> u64 {
        let end = self.paused_at_ms.unwrap_or(elapsed_ms);
        end.saturating_sub(self.started_ms).min(self.length_ms)
    }

    /// The milliseconds left, never below zero.
    pub fn remaining_ms(&self, elapsed_ms: u64) -> u64 {
        self.length_ms.saturating_sub(self.used_ms(elapsed_ms))
    }

    /// The whole seconds left, as the panel shows them.
    ///
    /// Rounded up, because a timer reading `0:00` for its last second is a timer that
    /// appears to have finished a second before it has.
    pub fn seconds_left(&self, elapsed_ms: u64) -> u64 {
        self.remaining_ms(elapsed_ms).div_ceil(1000)
    }

    /// How much of the round is left, in `[0, 1]`.
    ///
    /// From the *seconds the panel shows* rather than from the milliseconds underneath,
    /// and that is a decision about work rather than about arithmetic: the host
    /// rasterizes a panel whenever its contents differ, so a ring that moved smoothly
    /// would mean re-uploading a texture sixty times a second to show a number that
    /// changes once. The number and the ring therefore move together, once a second,
    /// and the panel sits still in between.
    pub fn fraction_left(&self, elapsed_ms: u64) -> f32 {
        let total = self.length_ms.div_ceil(1000);
        if total == 0 {
            return 0.0;
        }
        #[allow(clippy::cast_precision_loss)]
        let left = self.seconds_left(elapsed_ms) as f32 / total as f32;
        left.clamp(0.0, 1.0)
    }

    /// Whether this round has reached zero and has not been noticed yet.
    pub fn is_over(&self, elapsed_ms: u64) -> bool {
        self.phase == Phase::Counting && self.remaining_ms(elapsed_ms) == 0
    }

    /// Stop counting, keeping the time already served.
    pub fn pause(&mut self, elapsed_ms: u64) {
        if self.phase != Phase::Counting {
            return;
        }
        self.paused_at_ms = Some(elapsed_ms);
        self.phase = Phase::Paused;
    }

    /// Start counting again, having served no time while stopped.
    ///
    /// The shift is applied to `started_ms` rather than remembered separately, so the
    /// countdown's arithmetic has one rule instead of two: it always measures from
    /// `started_ms` to now.
    pub fn resume(&mut self, elapsed_ms: u64) {
        let Some(paused_at) = self.paused_at_ms.take() else {
            return;
        };
        self.started_ms = self
            .started_ms
            .saturating_add(elapsed_ms.saturating_sub(paused_at));
        self.phase = Phase::Counting;
    }

    /// Throw this round away and start the same kind again from now.
    pub fn restart(&mut self, elapsed_ms: u64) {
        self.started_ms = elapsed_ms;
        self.paused_at_ms = None;
        self.phase = Phase::Counting;
    }
}

/// A length in minutes, as whole milliseconds.
///
/// Through `u64` rather than `Duration` because every number a timer compares is a
/// quantity rather than a point in time, and `Duration` would add a type to each of
/// those comparisons for nothing.
pub fn minutes(count: i64) -> u64 {
    #[allow(clippy::cast_sign_loss)]
    let count = count.max(0) as u64;
    count.saturating_mul(60_000)
}

/// Whole seconds as `MM:SS`.
///
/// Minutes are not wrapped at 60, because a round can be two hours long and a countdown
/// that read `0:00` at the two-hour mark would be a bug rather than a style. Two hours
/// reads as `120:00`, which is a number a person can read.
pub fn format_seconds(seconds: u64) -> String {
    format!("{:02}:{:02}", seconds / 60, seconds % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(round_minutes: i64) -> Round {
        Round::new(RoundKind::Focus, minutes(round_minutes), 0, 0)
    }

    #[test]
    fn a_round_shows_the_time_that_is_left_and_not_the_time_that_has_passed() {
        let round = at(25);
        assert_eq!(round.remaining_ms(0), minutes(25));
        assert_eq!(round.remaining_ms(60_000), minutes(24));
        assert_eq!(round.remaining_ms(minutes(25)), 0);
        assert_eq!(
            round.remaining_ms(minutes(60)),
            0,
            "a session open longer than the round leaves a timer at zero, and a timer that counts \
             past zero is lying about how much time is left"
        );
    }

    #[test]
    fn the_last_second_reads_as_one_second_because_a_timer_showing_zero_early_is_wrong() {
        let round = at(1);
        assert_eq!(round.seconds_left(59_000), 1, "one second left reads as 1");
        assert_eq!(
            round.seconds_left(59_999),
            1,
            "even at the last millisecond"
        );
        assert_eq!(
            round.seconds_left(60_000),
            0,
            "and zero once it is truly over"
        );
    }

    #[test]
    fn the_ring_moves_with_the_number_and_not_between_it() {
        // The host re-uploads a panel whenever its contents differ, so a ring computed
        // from milliseconds would mean a texture upload per frame to show a number that
        // changes once a second. Both come from the same whole second, so they cannot
        // disagree, and the panel is still for every tick in between.
        let round = at(1);
        let steps = |from: u64, to: u64| {
            let mut distinct = Vec::new();
            for tenth in (from..=to).step_by(10) {
                let fraction = round.fraction_left(tenth);
                if distinct.last().copied() != Some(fraction) {
                    distinct.push(fraction);
                }
            }
            distinct
        };
        assert_eq!(
            steps(0, 999).len(),
            1,
            "a second is one fraction, so the panel is still for every tick inside it"
        );
        let minute = steps(0, minutes(1));
        assert_eq!(
            minute.len(),
            61,
            "and a minute is sixty steps rather than six thousand, one per second the number \
             itself changes: {}",
            minute.len()
        );
        assert!(
            (minute[0] - 1.0).abs() < 1e-6,
            "a full round is a full ring"
        );
        assert!((minute[60] - 0.0).abs() < 1e-6, "and an empty one is empty");
    }

    #[test]
    fn a_zero_length_round_is_an_over_round_rather_than_a_division_by_zero() {
        let round = Round::new(RoundKind::Focus, 0, 0, 0);
        assert_eq!(round.fraction_left(0), 0.0);
        assert!(round.is_over(0));
    }

    #[test]
    fn seconds_are_shown_as_minutes_and_seconds_even_past_an_hour() {
        assert_eq!(format_seconds(0), "00:00");
        assert_eq!(format_seconds(9), "00:09");
        assert_eq!(format_seconds(1500), "25:00");
        assert_eq!(
            format_seconds(7200),
            "120:00",
            "a two-hour round is allowed, and a countdown that read 00:00 at the two-hour mark \
             would be a bug rather than a style"
        );
    }

    #[test]
    fn pausing_freezes_the_remaining_time_and_resuming_does_not_serve_the_gap() {
        let mut round = at(25);
        round.pause(minutes(10));
        assert_eq!(round.phase, Phase::Paused);
        assert_eq!(
            round.remaining_ms(minutes(10)),
            minutes(15),
            "ten minutes served, and the other fifteen are still there"
        );
        assert_eq!(
            round.remaining_ms(minutes(20)),
            minutes(15),
            "ten minutes of stopping for a coffee do not come off a round you are working in"
        );
        round.resume(minutes(20));
        assert_eq!(round.phase, Phase::Counting);
        assert_eq!(round.remaining_ms(minutes(20)), minutes(15));
        assert_eq!(
            round.remaining_ms(minutes(35)),
            0,
            "and it runs out fifteen minutes after the resume, not ten"
        );
    }

    #[test]
    fn a_round_that_is_over_stays_over_rather_than_counting_past_zero() {
        let round = at(1);
        assert!(round.is_over(minutes(2)));
        assert_eq!(round.remaining_ms(minutes(9)), 0);
        assert!(round.is_over(minutes(9)));
    }

    #[test]
    fn pausing_an_over_round_does_nothing_because_there_is_nothing_left_to_freeze() {
        let mut round = at(1);
        round.phase = Phase::Over;
        round.pause(minutes(5));
        assert_eq!(
            round.phase,
            Phase::Over,
            "an over round is not a running one that happens to be at zero, and treating it as one \
             would let a user pause a finished timer and see it stay finished"
        );
    }

    #[test]
    fn restarting_keeps_the_kind_and_the_count_but_throws_the_time_away() {
        let mut round = Round::new(RoundKind::LongBreak, minutes(15), 0, 3);
        round.restart(minutes(30));
        assert_eq!(round.kind, RoundKind::LongBreak);
        assert_eq!(
            round.completed_rounds, 3,
            "three rounds are still three rounds"
        );
        assert_eq!(round.remaining_ms(minutes(30)), minutes(15));
    }
}
