//! The one place the worker is waited on at.
//!
//! This exists because of a latency, and the latency is worth writing down before the
//! mechanism is. A plugin answers on its own schedule: the user pressed a button on a
//! panel, or changed a setting in the settings window, and the plugin writes a new scene
//! a millisecond later. That answer arrives on a session's reader thread, into that
//! session's own queue — and the worker was sitting in `recv_timeout` on the *command*
//! channel, which nothing about a plugin's answer touches. So the answer sat unprocessed
//! for up to [`crate::worker::EVALUATION_INTERVAL`], and a panel that had been told what to
//! draw kept drawing what it had drawn for a tenth of a second.
//!
//! A tenth of a second is not a long time in a chat window. It is a long time for the thing
//! a user judges a native panel by: they press a control and the picture is already
//! supposed to have changed.
//!
//! So the worker waits on **both** sources. One `Mutex` and one `Condvar`, with commands in
//! a bounded queue and a single coalesced flag for "a plugin said something":
//!
//! * **Commands are queued and bounded**, exactly as the channel they replace was. A full
//!   queue drops a command, and the caller is told so. Nothing about ordering changes:
//!   commands come out first-in-first-out, so a `Shutdown` queued behind an install still
//!   happens after it.
//! * **A plugin's answer is a flag, not a queue.** It carries no payload — the payload is
//!   already in the session's queue, and duplicating it here would be a second copy of
//!   every message on its way to the worker. Ten plugins all speaking at once is one
//!   boolean, because the answer to "is there work?" does not depend on how many
//!   conversations produced it.
//!
//! Both are waited on together, so a press, a configuration change, a catalog refresh, a
//! plugin's panel and a plugin's exit all wake the worker at the moment they happen rather
//! than at the next evaluation. What is left of the evaluation interval is only the tick a
//! clock-driven plugin needs — a countdown that says nothing until the second changes — and
//! that is the one thing a timer is for.

use crate::worker::PluginCommand;
use std::collections::VecDeque;
use std::fmt;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// How many commands may be queued before one is dropped.
///
/// The same bound the channel this replaced used, and for the same reason: the senders are
/// the settings service and the GPUI thread, and neither may block on a worker that is
/// inside a download. A dropped command is a click that did not register; a blocked
/// settings window is not.
const COMMAND_CAPACITY: usize = 32;

/// What woke the worker.
#[derive(Debug, PartialEq)]
pub enum Arrival {
    /// A command from the product.
    Command(PluginCommand),
    /// A plugin said something, and its session's queue has it.
    ///
    /// No payload on purpose — see the module comment. The worker reads the sessions, so
    /// this only has to say that there is something to read.
    Spoke,
    /// Nothing arrived before the timeout.
    TimedOut,
    /// Every sender is gone.
    ///
    /// Distinct from a timeout because it is the end of the worker's life rather than a
    /// pause in it, and the loop treats the two differently.
    Closed,
}

/// The worker's inbox: commands, and the fact that a plugin spoke.
#[derive(Default)]
pub struct Inbox {
    state: Mutex<State>,
    signal: Condvar,
}

#[derive(Default)]
struct State {
    commands: VecDeque<PluginCommand>,
    /// Whether a plugin has said something the worker has not looked at yet.
    ///
    /// A flag rather than a count, and a count would be no better: one `true` and one
    /// `false` both mean "go and read the sessions", and the sessions are the only place
    /// the messages actually are.
    spoke: bool,
    /// Whether the last sender is gone. Set once, by the last `Endpoint` drop.
    closed: bool,
}

impl Inbox {
    pub fn new() -> Self {
        Self::default()
    }

    /// Queue a command, reporting whether it fit.
    ///
    /// `false` means the queue was full or the worker is gone, and the caller's answer is
    /// the same either way: the click did not register.
    pub fn send(&self, command: PluginCommand) -> bool {
        let mut state = self.lock();
        if state.closed || state.commands.len() >= COMMAND_CAPACITY {
            return false;
        }
        state.commands.push_back(command);
        // Notified after the release of the implicit guard borrow, and only on the way in:
        // a command is always worth waking for, while `spoke` is not — ten plugins saying
        // the same thing is still one flag and one wake-up.
        self.signal.notify_one();
        true
    }

    /// Say that a plugin has queued something for the worker to read.
    pub fn spoke(&self) {
        let mut state = self.lock();
        state.spoke = true;
        self.signal.notify_one();
    }

    /// Say that no sender is left.
    ///
    /// The worker still reads what was queued before it stops — a `Shutdown` sent and then
    /// the endpoint dropped has to be honoured, not lost to the drop.
    pub fn close(&self) {
        let mut state = self.lock();
        state.closed = true;
        self.signal.notify_one();
    }

    /// Wait for something to do, for at most `timeout`.
    ///
    /// Commands first, then the flag: a command is a decision somebody made and a flag is
    /// only a hint that the sessions are worth reading, so when both are waiting the
    /// decision is handled first and the flag is still there for the turn after.
    pub fn wait(&self, timeout: Duration) -> Arrival {
        let deadline = Instant::now().checked_add(timeout);
        let mut state = self.lock();
        loop {
            if let Some(command) = state.commands.pop_front() {
                return Arrival::Command(command);
            }
            if state.spoke {
                state.spoke = false;
                return Arrival::Spoke;
            }
            if state.closed {
                return Arrival::Closed;
            }
            // A deadline rather than a repeated full-length wait, so a spurious wake-up
            // cannot extend the wait without bound. `None` only if `Instant` could not
            // represent the deadline at all, which is treated as "no time left" rather
            // than as "wait for ever".
            let Some(deadline) = deadline else {
                return Arrival::TimedOut;
            };
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Arrival::TimedOut;
            }
            state = self
                .signal
                .wait_timeout(state, left)
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .0;
        }
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl fmt::Debug for Inbox {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let state = self.lock();
        formatter
            .debug_struct("Inbox")
            .field("queued", &state.commands.len())
            .field("spoke", &state.spoke)
            .field("closed", &state.closed)
            .finish()
    }
}

/// A session's handle on the worker: enough to say "I have something for you".
///
/// One cheap clone per session, held by that session's reader thread, and the only thing
/// the host side of a plugin needs beyond the message queue it already writes to. A reader
/// thread that did not have this would leave the worker asleep on a message it had already
/// been handed.
#[derive(Clone, Debug)]
pub struct Wake(Arc<Inbox>);

impl Wake {
    pub fn new(inbox: Arc<Inbox>) -> Self {
        Self(inbox)
    }

    /// A plugin has queued a message the worker has not read yet.
    pub fn spoke(&self) {
        self.0.spoke();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a_command() -> PluginCommand {
        PluginCommand::RefreshCatalog
    }

    #[test]
    fn a_command_comes_back_the_moment_it_is_sent() {
        // The property the whole file exists for: a product-side command does not wait for
        // the evaluation interval, so a press reaches the worker while the user is still
        // holding the mouse down.
        let inbox = Inbox::new();
        assert!(inbox.send(a_command()));
        assert_eq!(
            inbox.wait(Duration::from_secs(3600)),
            Arrival::Command(a_command()),
            "and it comes back without the caller having waited for anything"
        );
    }

    #[test]
    fn a_plugin_answering_wakes_a_worker_that_would_otherwise_sleep_for_an_hour() {
        // The idle answer: a worker whose plugins are all silent waits on the order of an
        // hour, because nothing it runs can change without a command. Before this existed
        // a plugin that *did* speak was queued behind that hour, which is why a panel could
        // keep drawing what it had drawn after the user changed what it should draw.
        let inbox = Arc::new(Inbox::new());
        let reader = Wake::new(Arc::clone(&inbox));
        let worker = {
            let inbox = Arc::clone(&inbox);
            std::thread::spawn(move || inbox.wait(Duration::from_secs(3600)))
        };
        std::thread::sleep(Duration::from_millis(20));
        reader.spoke();
        assert_eq!(
            worker.join().expect("the worker thread"),
            Arrival::Spoke,
            "so the answer is read when it arrives rather than at the next evaluation"
        );
    }

    #[test]
    fn ten_plugins_speaking_are_one_wake_up_and_one_flag() {
        // The coalescing, stated as a number: the payload is in the sessions, so the flag
        // only has to say there is something to read, and nine extra wakes would be nine
        // extra turns of a loop that finds nothing.
        let inbox = Arc::new(Inbox::new());
        let worker = {
            let inbox = Arc::clone(&inbox);
            std::thread::spawn(move || inbox.wait(Duration::from_millis(50)))
        };
        for _ in 0..10 {
            Wake::new(Arc::clone(&inbox)).spoke();
        }
        assert_eq!(worker.join().expect("the worker thread"), Arrival::Spoke);
        assert_eq!(
            inbox.wait(Duration::ZERO),
            Arrival::TimedOut,
            "and the flag was consumed once, so the next wait is a real wait"
        );
    }

    #[test]
    fn an_answer_arriving_while_the_worker_reads_is_not_lost() {
        // The race that decides whether this mechanism works at all: the worker is between
        // reading the sessions and going to sleep. The reader's flag is set *after* the
        // message is queued, so either the worker reads the message or it sees the flag —
        // there is no order in which both are missed.
        let inbox = Inbox::new();
        inbox.spoke();
        assert_eq!(
            inbox.wait(Duration::ZERO),
            Arrival::Spoke,
            "a flag set before the wait is not missed by it"
        );
        assert_eq!(inbox.wait(Duration::ZERO), Arrival::TimedOut);
    }

    #[test]
    fn commands_keep_their_order_and_come_before_the_flag() {
        // Order is not decoration: `Shutdown` behind an install has to happen after the
        // install, and a command queued before a plugin spoke is the older news.
        let inbox = Inbox::new();
        inbox.send(PluginCommand::Uninstall(
            bongocat_plugin_protocol::PluginId::new("pomodoro").expect("valid"),
        ));
        inbox.spoke();
        assert!(
            matches!(inbox.wait(Duration::ZERO), Arrival::Command(_)),
            "a decision somebody made is handled before a hint that a session has news"
        );
        assert_eq!(
            inbox.wait(Duration::ZERO),
            Arrival::Spoke,
            "and the hint is still waiting for the turn after, not consumed by the command"
        );
    }

    #[test]
    fn a_full_queue_drops_a_command_rather_than_blocking_its_sender() {
        // The bound is the same one the channel had: the settings thread and the GPUI thread
        // must not wait on a worker that is inside a download.
        let inbox = Inbox::new();
        let mut queued = 0;
        while inbox.send(a_command()) {
            queued += 1;
            assert!(queued <= COMMAND_CAPACITY, "the queue is bounded");
        }
        assert_eq!(queued, COMMAND_CAPACITY);
    }

    #[test]
    fn a_queued_command_is_still_read_after_the_last_sender_goes_away() {
        // The endpoint the product holds for the whole run drops at shutdown, and the stop
        // it queued must be honoured rather than lost to the drop that announced it.
        let inbox = Arc::new(Inbox::new());
        assert!(inbox.send(PluginCommand::Shutdown));
        inbox.close();
        assert_eq!(
            inbox.wait(Duration::from_secs(3600)),
            Arrival::Command(PluginCommand::Shutdown)
        );
        assert_eq!(
            inbox.wait(Duration::ZERO),
            Arrival::Closed,
            "and only then is the worker finished"
        );
    }

    #[test]
    fn a_command_for_a_closed_worker_is_refused_rather_than_queued_for_ever() {
        let inbox = Inbox::new();
        inbox.close();
        assert!(!inbox.send(a_command()));
    }

    #[test]
    fn a_zero_timeout_is_a_look_and_not_a_sleep() {
        // What `Turn::Again` turns into: something was read, so the loop goes straight round
        // without paying for a timer.
        let inbox = Inbox::new();
        assert_eq!(inbox.wait(Duration::ZERO), Arrival::TimedOut);
        assert!(
            inbox.send(a_command()),
            "and a look costs a command nothing to queue behind"
        );
        assert_eq!(inbox.wait(Duration::ZERO), Arrival::Command(a_command()));
    }
}
