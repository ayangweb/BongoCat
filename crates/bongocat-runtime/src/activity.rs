//! Bounded, privacy-filtered physical edges for event-driven consumers.
use crate::{InputControl, InputEdge, InputEvent, InputSource, MonotonicMillis, MouseButton};
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex, Weak},
    thread::Thread,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActivityControl {
    Keyboard,
    Space,
    Enter,
    Backspace,
    Tab,
    ShiftLeft,
    ShiftRight,
    CtrlLeft,
    MouseLeft,
    MouseRight,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ActivityEvent {
    pub control: ActivityControl,
    pub edge: InputEdge,
    pub at: MonotonicMillis,
}

impl ActivityEvent {
    pub(crate) fn captured(event: &InputEvent) -> Option<Self> {
        let InputEvent::Edge {
            control,
            edge,
            source: InputSource::Capture,
            at,
        } = event
        else {
            return None;
        };
        let control = match control {
            InputControl::Key(key) => match key.hid_usage() {
                0x2c => ActivityControl::Space,
                0x28 | 0x58 => ActivityControl::Enter,
                0x2a => ActivityControl::Backspace,
                0x2b => ActivityControl::Tab,
                0xe1 => ActivityControl::ShiftLeft,
                0xe5 => ActivityControl::ShiftRight,
                0xe0 => ActivityControl::CtrlLeft,
                _ => ActivityControl::Keyboard,
            },
            InputControl::Mouse(MouseButton::Left) => ActivityControl::MouseLeft,
            InputControl::Mouse(MouseButton::Right) => ActivityControl::MouseRight,
            _ => return None,
        };
        Some(Self {
            control,
            edge: *edge,
            at: *at,
        })
    }
}

#[derive(Default)]
struct State {
    events: VecDeque<ActivityEvent>,
    overflow: u64,
    wake: Option<Thread>,
}

#[derive(Default)]
pub(crate) struct ActivityPublisher(Mutex<Weak<Mutex<State>>>);

/// One subscriber; no key identities, text or pointer positions are retained.
#[derive(Clone)]
pub struct ActivitySubscription(Arc<Mutex<State>>);

impl ActivityPublisher {
    pub(crate) fn subscribe(&self) -> ActivitySubscription {
        let state = Arc::new(Mutex::new(State::default()));
        *self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Arc::downgrade(&state);
        ActivitySubscription(state)
    }

    pub(crate) fn publish(&self, event: ActivityEvent) {
        let Some(state) = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .upgrade()
        else {
            return;
        };
        let mut state = state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.events.len() == 256 {
            state.overflow = state.overflow.saturating_add(1);
            // Audio is best effort, never a reason to block reliable input.
            // Discard the stale batch and report the gap to the consumer.
            state.events.clear();
        }
        state.events.push_back(event);
        if let Some(wake) = &state.wake {
            wake.unpark();
        }
    }
}

impl ActivitySubscription {
    pub fn set_waker(&self, thread: Thread) {
        let mut state = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.wake = Some(thread.clone());
        if !state.events.is_empty() {
            thread.unpark();
        }
    }

    /// The overflow count is cumulative. A gap clears pending playback, not input state.
    pub fn drain(&self) -> (Vec<ActivityEvent>, u64) {
        let mut state = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        (state.events.drain(..).collect(), state.overflow)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{InputResetReason, PhysicalKey};

    #[test]
    fn captured_edges_are_private_and_corrections_are_silent() {
        for (key, expected) in [
            (PhysicalKey::KEY_A, ActivityControl::Keyboard),
            (PhysicalKey::from_hid_usage(0x2c), ActivityControl::Space),
        ] {
            let mut event = InputEvent::Edge {
                control: InputControl::Key(key),
                edge: InputEdge::Down,
                source: InputSource::Capture,
                at: MonotonicMillis::new(1),
            };
            assert_eq!(ActivityEvent::captured(&event).unwrap().control, expected);
            if let InputEvent::Edge { source, .. } = &mut event {
                *source = InputSource::Reconciliation;
            }
            assert!(ActivityEvent::captured(&event).is_none());
        }
        assert!(
            ActivityEvent::captured(&InputEvent::Reset {
                reason: InputResetReason::Test,
                at: MonotonicMillis::new(1),
            })
            .is_none()
        );
    }

    #[test]
    fn bounded_queue_preserves_order_reports_overflow_and_never_replays() {
        let publisher = ActivityPublisher::default();
        let event = ActivityEvent {
            control: ActivityControl::Space,
            edge: InputEdge::Down,
            at: MonotonicMillis::new(0),
        };
        publisher.publish(event);
        let subscriber = publisher.subscribe();
        assert!(subscriber.drain().0.is_empty());
        subscriber.set_waker(std::thread::current());
        for n in 0..256 {
            publisher.publish(ActivityEvent {
                at: MonotonicMillis::new(n),
                ..event
            });
        }
        let (events, overflow) = subscriber.drain();
        assert_eq!(overflow, 0);
        assert_eq!(events.len(), 256);
        assert!(events.windows(2).all(|pair| pair[0].at < pair[1].at));
        for n in 0..257 {
            publisher.publish(ActivityEvent {
                at: MonotonicMillis::new(n),
                ..event
            });
        }
        let (events, overflow) = subscriber.drain();
        assert_eq!(overflow, 1);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].at.value(), 256);
        assert!(subscriber.drain().0.is_empty());
        drop(subscriber);
        publisher.publish(event);
    }
}
