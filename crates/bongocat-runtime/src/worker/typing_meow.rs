//! Typing feedback is best effort and never changes the reliable input stream.

use crate::*;

#[derive(Default)]
pub(crate) struct TypingMeow {
    last_play: Option<Duration>,
}

impl TypingMeow {
    pub(crate) fn should_play(
        &mut self,
        keyboard_down: bool,
        fresh_press: bool,
        enabled: bool,
        now: Duration,
    ) -> bool {
        if !enabled || !keyboard_down || !fresh_press {
            return false;
        }
        if self
            .last_play
            .is_some_and(|last| now.saturating_sub(last) < Duration::from_millis(180))
        {
            return false;
        }
        self.last_play = Some(now);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepted_input_distinguishes_fresh_press_from_repeat_and_stale_sequence() {
        let mut state = InputState::default();
        let mut gate = TypingMeow::default();
        let cases = [
            (0, InputEdge::Down, true),
            (1, InputEdge::Down, false),
            (2, InputEdge::Up, false),
            (2, InputEdge::Down, false),
            (3, InputEdge::Down, true),
        ];
        for (index, (sequence, edge, expected)) in cases.into_iter().enumerate() {
            let before = state.diagnostics.captured_down;
            state.apply(SequencedInputEvent {
                sequence,
                event: InputEvent::Edge {
                    control: InputControl::Key(PhysicalKey::KEY_A),
                    edge,
                    source: InputSource::Capture,
                    at: MonotonicMillis::new(index as u64 * 200),
                },
            });
            assert_eq!(
                gate.should_play(
                    edge == InputEdge::Down,
                    state.diagnostics.captured_down > before,
                    true,
                    Duration::from_millis(index as u64 * 200),
                ),
                expected
            );
        }
    }

    #[test]
    fn mute_repeats_and_non_keyboard_events_do_not_consume_cooldown() {
        let mut gate = TypingMeow::default();
        assert!(!gate.should_play(true, true, false, Duration::ZERO));
        assert!(!gate.should_play(true, false, true, Duration::ZERO));
        assert!(!gate.should_play(false, true, true, Duration::ZERO));
        assert!(gate.should_play(true, true, true, Duration::ZERO));
        assert!(!gate.should_play(true, true, true, Duration::from_millis(179)));
        assert!(gate.should_play(true, true, true, Duration::from_millis(180)));
        assert!(!gate.should_play(true, true, true, Duration::from_millis(100)));
    }
}
