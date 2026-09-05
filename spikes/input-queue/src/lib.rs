#![forbid(unsafe_code)]

use std::collections::{BTreeMap, VecDeque};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueueErrorKind {
    Full,
    Closed,
}

#[derive(Debug, PartialEq, Eq)]
pub struct QueueError<T> {
    pub kind: QueueErrorKind,
    pub item: T,
}

/// Bounded FIFO for key/button edges, device events and commands.
///
/// A failed push always returns the original item. Callers can therefore emit
/// a reset/recovery signal without silently losing an edge or command.
#[derive(Debug)]
pub struct ReliableQueue<T> {
    capacity: usize,
    items: VecDeque<T>,
    closed: bool,
    overflow_count: u64,
    recovery_reset_count: u64,
    recovery_discard_count: u64,
}

impl<T> ReliableQueue<T> {
    pub fn with_capacity(capacity: usize) -> Self {
        assert!(capacity > 0, "reliable queue capacity must be positive");
        Self {
            capacity,
            items: VecDeque::with_capacity(capacity),
            closed: false,
            overflow_count: 0,
            recovery_reset_count: 0,
            recovery_discard_count: 0,
        }
    }

    pub fn push(&mut self, item: T) -> Result<(), QueueError<T>> {
        if self.closed {
            return Err(QueueError {
                kind: QueueErrorKind::Closed,
                item,
            });
        }
        if self.items.len() == self.capacity {
            self.overflow_count += 1;
            return Err(QueueError {
                kind: QueueErrorKind::Full,
                item,
            });
        }
        self.items.push_back(item);
        Ok(())
    }

    /// Push an edge and install a recovery marker when the queue is full.
    ///
    /// Once full, the relative order of already buffered edges can no longer
    /// establish a trustworthy state. The buffered items are therefore
    /// discarded as one observable recovery operation, and `reset` is made the
    /// next item to consume. The rejected item is still returned to the caller.
    pub fn push_with_overflow_reset(&mut self, item: T, reset: T) -> Result<(), QueueError<T>> {
        if self.closed {
            return Err(QueueError {
                kind: QueueErrorKind::Closed,
                item,
            });
        }
        if self.items.len() == self.capacity {
            self.overflow_count += 1;
            self.recovery_reset_count += 1;
            self.recovery_discard_count += self.items.len() as u64;
            self.items.clear();
            self.items.push_back(reset);
            return Err(QueueError {
                kind: QueueErrorKind::Full,
                item,
            });
        }
        self.items.push_back(item);
        Ok(())
    }

    pub fn pop(&mut self) -> Option<T> {
        self.items.pop_front()
    }

    pub fn close(&mut self) {
        self.closed = true;
    }

    pub fn is_closed(&self) -> bool {
        self.closed
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn overflow_count(&self) -> u64 {
        self.overflow_count
    }

    pub fn recovery_reset_count(&self) -> u64 {
        self.recovery_reset_count
    }

    pub fn recovery_discard_count(&self) -> u64 {
        self.recovery_discard_count
    }
}

#[derive(Debug, Default)]
pub struct LatestValue<T> {
    value: Option<T>,
}

impl<T> LatestValue<T> {
    pub fn replace(&mut self, value: T) {
        self.value = Some(value);
    }

    pub fn take(&mut self) -> Option<T> {
        self.value.take()
    }

    pub fn peek(&self) -> Option<&T> {
        self.value.as_ref()
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LatestValuesDiagnostics {
    pub captured: u64,
    pub coalesced: u64,
    pub consumed: u64,
    pub discarded: u64,
    pub overflows: u64,
    pub rejected_after_close: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LatestValuesErrorKind {
    CapacityExceeded,
    Closed,
}

#[derive(Debug, PartialEq, Eq)]
pub struct LatestValuesError<K, V> {
    pub kind: LatestValuesErrorKind,
    pub key: K,
    pub value: V,
}

/// Fixed-cardinality latest-value map for independent high-frequency streams.
///
/// Replacing an existing key never consumes more capacity. Introducing a new
/// key when full returns the original key/value, so a producer can report a
/// malformed device/profile without affecting reliable edge delivery.
#[derive(Debug)]
pub struct LatestValues<K, V> {
    capacity: usize,
    values: BTreeMap<K, V>,
    closed: bool,
    diagnostics: LatestValuesDiagnostics,
}

impl<K: Ord, V> LatestValues<K, V> {
    pub fn with_capacity(capacity: usize) -> Self {
        assert!(capacity > 0, "latest-values capacity must be positive");
        Self {
            capacity,
            values: BTreeMap::new(),
            closed: false,
            diagnostics: LatestValuesDiagnostics::default(),
        }
    }

    pub fn replace(&mut self, key: K, value: V) -> Result<(), LatestValuesError<K, V>> {
        if self.closed {
            self.diagnostics.rejected_after_close += 1;
            return Err(LatestValuesError {
                kind: LatestValuesErrorKind::Closed,
                key,
                value,
            });
        }
        if !self.values.contains_key(&key) && self.values.len() == self.capacity {
            self.diagnostics.overflows += 1;
            return Err(LatestValuesError {
                kind: LatestValuesErrorKind::CapacityExceeded,
                key,
                value,
            });
        }
        self.diagnostics.captured += 1;
        if self.values.insert(key, value).is_some() {
            self.diagnostics.coalesced += 1;
        }
        Ok(())
    }

    pub fn drain(&mut self) -> Vec<(K, V)> {
        let values = std::mem::take(&mut self.values)
            .into_iter()
            .collect::<Vec<_>>();
        self.diagnostics.consumed += values.len() as u64;
        values
    }

    pub fn discard_where(&mut self, mut predicate: impl FnMut(&K) -> bool) -> usize {
        let before = self.values.len();
        self.values.retain(|key, _| !predicate(key));
        let discarded = before - self.values.len();
        self.diagnostics.discarded += discarded as u64;
        discarded
    }

    pub fn close(&mut self) {
        self.closed = true;
    }

    pub fn len(&self) -> usize {
        self.values.len()
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    pub fn diagnostics(&self) -> LatestValuesDiagnostics {
        self.diagnostics
    }

    pub fn is_fully_accounted(&self) -> bool {
        self.diagnostics.captured
            == self
                .diagnostics
                .coalesced
                .saturating_add(self.diagnostics.consumed)
                .saturating_add(self.diagnostics.discarded)
                .saturating_add(self.values.len() as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
    enum Axis {
        LeftX,
        LeftY,
        RightX,
        RightY,
        LeftTrigger,
        RightTrigger,
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
    struct AxisKey {
        device_slot: u8,
        generation: u64,
        axis: Axis,
    }

    #[test]
    fn reliable_queue_preserves_fifo_order() {
        let mut queue = ReliableQueue::with_capacity(2);
        queue.push("first").unwrap();
        queue.push("second").unwrap();
        assert_eq!(queue.pop(), Some("first"));
        assert_eq!(queue.pop(), Some("second"));
        assert_eq!(queue.pop(), None);
        assert!(queue.is_empty());
    }

    #[test]
    fn overflow_returns_original_item_and_is_observable() {
        let mut queue = ReliableQueue::with_capacity(1);
        queue.push(1).unwrap();
        let error = queue.push(2).unwrap_err();
        assert_eq!(
            error,
            QueueError {
                kind: QueueErrorKind::Full,
                item: 2
            }
        );
        assert_eq!(queue.overflow_count(), 1);
        assert_eq!(queue.pop(), Some(1));
    }

    #[test]
    fn overflow_reset_discards_untrusted_edges_and_preserves_recovery_marker() {
        let mut queue = ReliableQueue::with_capacity(2);
        queue.push("first").unwrap();
        queue.push("second").unwrap();
        let error = queue
            .push_with_overflow_reset("third", "reset")
            .unwrap_err();
        assert_eq!(error.kind, QueueErrorKind::Full);
        assert_eq!(error.item, "third");
        assert_eq!(queue.pop(), Some("reset"));
        assert_eq!(queue.pop(), None);
        assert_eq!(queue.overflow_count(), 1);
        assert_eq!(queue.recovery_reset_count(), 1);
        assert_eq!(queue.recovery_discard_count(), 2);
    }

    #[test]
    fn closed_overflow_reset_does_not_enqueue_recovery_marker() {
        let mut queue = ReliableQueue::with_capacity(1);
        queue.close();
        let error = queue.push_with_overflow_reset("item", "reset").unwrap_err();
        assert_eq!(error.kind, QueueErrorKind::Closed);
        assert_eq!(queue.recovery_reset_count(), 0);
        assert!(queue.is_empty());
    }

    #[test]
    fn close_rejects_new_items_without_dropping_them() {
        let mut queue = ReliableQueue::with_capacity(1);
        queue.push(1).unwrap();
        queue.close();
        let error = queue.push(2).unwrap_err();
        assert_eq!(
            error,
            QueueError {
                kind: QueueErrorKind::Closed,
                item: 2
            }
        );
        assert!(queue.is_closed());
        assert_eq!(queue.pop(), Some(1));
    }

    #[test]
    #[should_panic(expected = "capacity must be positive")]
    fn zero_capacity_is_rejected() {
        let _ = ReliableQueue::<u8>::with_capacity(0);
    }

    #[test]
    fn latest_value_coalesces_high_frequency_updates() {
        let mut latest = LatestValue::default();
        latest.replace(1);
        latest.replace(2);
        assert_eq!(latest.peek(), Some(&2));
        assert_eq!(latest.take(), Some(2));
        assert_eq!(latest.take(), None);
    }

    #[test]
    fn keyed_latest_values_coalesce_each_gamepad_axis_independently() {
        let mut latest = LatestValues::with_capacity(12);
        let left_x = AxisKey {
            device_slot: 0,
            generation: 1,
            axis: Axis::LeftX,
        };
        let right_y = AxisKey {
            device_slot: 0,
            generation: 1,
            axis: Axis::RightY,
        };
        for value in 0..10_000 {
            latest.replace(left_x, value).unwrap();
        }
        latest.replace(right_y, 7_500).unwrap();

        assert_eq!(latest.drain(), vec![(left_x, 9_999), (right_y, 7_500)]);
        assert_eq!(latest.diagnostics().captured, 10_001);
        assert_eq!(latest.diagnostics().coalesced, 9_999);
        assert_eq!(latest.diagnostics().consumed, 2);
        assert!(latest.is_fully_accounted());
    }

    #[test]
    fn keyed_latest_values_bound_untrusted_axis_cardinality() {
        let mut latest = LatestValues::with_capacity(6);
        for axis in [
            Axis::LeftX,
            Axis::LeftY,
            Axis::RightX,
            Axis::RightY,
            Axis::LeftTrigger,
            Axis::RightTrigger,
        ] {
            latest
                .replace(
                    AxisKey {
                        device_slot: 0,
                        generation: 1,
                        axis,
                    },
                    0,
                )
                .unwrap();
        }
        let error = latest
            .replace(
                AxisKey {
                    device_slot: 1,
                    generation: 1,
                    axis: Axis::LeftX,
                },
                1,
            )
            .unwrap_err();
        assert_eq!(error.kind, LatestValuesErrorKind::CapacityExceeded);
        assert_eq!(latest.len(), 6);
        assert_eq!(latest.diagnostics().overflows, 1);
        assert!(latest.is_fully_accounted());
    }

    #[test]
    fn gamepad_disconnect_discards_old_generation_before_reconnect() {
        let mut latest = LatestValues::with_capacity(12);
        let stale = AxisKey {
            device_slot: 0,
            generation: 41,
            axis: Axis::LeftX,
        };
        let current = AxisKey {
            device_slot: 0,
            generation: 42,
            axis: Axis::LeftX,
        };
        latest.replace(stale, 75).unwrap();
        assert_eq!(latest.discard_where(|key| key.generation == 41), 1);
        latest.replace(current, -50).unwrap();

        assert_eq!(latest.drain(), vec![(current, -50)]);
        assert_eq!(latest.diagnostics().discarded, 1);
        assert!(latest.is_fully_accounted());
    }

    #[test]
    fn keyed_latest_values_close_flushes_pending_and_rejects_late_samples() {
        let mut latest = LatestValues::with_capacity(1);
        let key = AxisKey {
            device_slot: 0,
            generation: 1,
            axis: Axis::LeftX,
        };
        latest.replace(key, 10).unwrap();
        latest.close();
        let error = latest.replace(key, 20).unwrap_err();
        assert_eq!(error.kind, LatestValuesErrorKind::Closed);
        assert_eq!(latest.drain(), vec![(key, 10)]);
        assert_eq!(latest.diagnostics().rejected_after_close, 1);
        assert!(latest.is_fully_accounted());
    }
}
