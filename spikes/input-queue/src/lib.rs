#![forbid(unsafe_code)]

use std::collections::VecDeque;

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
}

impl<T> ReliableQueue<T> {
    pub fn with_capacity(capacity: usize) -> Self {
        assert!(capacity > 0, "reliable queue capacity must be positive");
        Self {
            capacity,
            items: VecDeque::with_capacity(capacity),
            closed: false,
            overflow_count: 0,
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
