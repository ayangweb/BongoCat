//! The queue between the tool and the plugin.
//!
//! A hook is fired by another program — Claude Code, or whatever else the user wires up —
//! and it is fired in a *separate short-lived process*, once per event, with no connection
//! to anything that is already running. So there is no pipe to hand an event down and no
//! socket to connect to; what there is, is a file both sides can name.
//!
//! This is deliberately the least interesting mechanism available, and each part of it is
//! chosen for what it cannot go wrong with:
//!
//! * **A file, not a port.** A listening socket needs a port, which collides with another
//!   instance, needs a firewall decision, and leaves something listening that has to be
//!   found and killed. A file needs nothing.
//! * **An append, not a rewrite.** The hook only ever appends one line, with the file opened
//!   for append, so two events arriving at the same moment cannot lose one another and the
//!   hook never has to read what is already there.
//! * **A byte offset, not a line count.** The running side remembers how far it has read.
//!   Appending never moves the bytes before that point, so "what is new" is a question with
//!   an exact answer and no scanning.
//!
//! What the hook writes is deliberately small — a tool's name and a state, never the tool's
//! input — because this file is a record of what the user has been doing, and a file that
//! accumulates a week of file contents is a file that has to be defended.

use crate::event::Event;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

/// The file both sides name.
pub const FILE_NAME: &str = "hook-events.jsonl";

/// Where the queue lives for a plugin given its data directory.
pub fn queue_path(data_dir: &Path) -> PathBuf {
    data_dir.join(FILE_NAME)
}

/// The seconds since the epoch, which is the one clock two processes agree about.
///
/// Not a local time and not a date: two short-lived processes need to be able to compare
/// their readings, and only an absolute instant means anything to both of them. Nothing here
/// formats it, so no timezone is involved.
pub fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_secs() as i64)
        .unwrap_or_default()
}

/// Append one event, and always succeed.
///
/// The `bool` says whether the line reached the file, and it is `false` for every reason a
/// file could not be written — which the hook reports and then does nothing about, because a
/// hook that fails takes the user's tool call down with it. A monitoring tool that can break
/// the thing it monitors is worse than one that misses an event.
///
/// The write is one `write` of one line on a file opened for append, which is the shape the
/// operating system makes atomic: a line is either wholly there or not there at all, even if
/// two hooks fire at the same instant.
pub fn append(path: &Path, event: &Event) -> bool {
    let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) else {
        return false;
    };
    let mut line = event.to_line();
    line.push('\n');
    let written = file.write_all(line.as_bytes()).and_then(|()| file.flush());
    written.is_ok()
}

/// The reading end: how far into the file this reader has got.
///
/// Held rather than re-read from scratch every tick, because "what is new" has to be
/// answerable without scanning a file that grows all day.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Reader {
    offset: u64,
}

impl Reader {
    /// A reader that has read nothing.
    pub fn new() -> Self {
        Self::default()
    }

    /// Every event appended since the last call, oldest first.
    ///
    /// A line this build cannot read is skipped rather than fatal, and a line that is only
    /// half written — a hook that was killed between the write and the flush is the one way
    /// that can happen — is left for the next call, because the offset only moves past whole
    /// lines. That is why the file is walked line by line rather than handed to a parser:
    /// a partial tail is a fact about a file being appended to, and a reader that treated it
    /// as corruption would stop reading a perfectly good queue.
    pub fn read_new(&mut self, path: &Path) -> Vec<Event> {
        let Ok(metadata) = std::fs::metadata(path) else {
            return Vec::new();
        };
        let length = metadata.len();
        if length < self.offset {
            // The file was truncated or replaced. Nothing this reader can do about who did
            // it, and the honest answer is to start again from the beginning rather than to
            // read from an offset that is now past the end.
            self.offset = 0;
        }
        if length == self.offset {
            return Vec::new();
        }
        let Ok(mut file) = File::open(path) else {
            return Vec::new();
        };
        if file.seek(SeekFrom::Start(self.offset)).is_err() {
            return Vec::new();
        }
        let mut text = String::new();
        if file.read_to_string(&mut text).is_err() {
            return Vec::new();
        }
        let mut events = Vec::new();
        let mut consumed = 0_u64;
        for line in text.split_inclusive('\n') {
            if !line.ends_with('\n') {
                // A tail without its terminator: a write in progress, not a bad line. Left
                // for the next tick, and `consumed` deliberately does not include it.
                break;
            }
            let trimmed = line.trim();
            if !trimmed.is_empty()
                && let Some(event) = Event::from_line(trimmed)
            {
                events.push(event);
            }
            consumed += line.len() as u64;
        }
        self.offset += consumed;
        events
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::Activity;

    fn event(session: &str, at_unix: i64) -> Event {
        Event {
            source: "claude-code".to_owned(),
            session: session.to_owned(),
            activity: Activity::Running,
            event: "PreToolUse".to_owned(),
            tool: Some("Bash".to_owned()),
            at_unix,
        }
    }

    #[test]
    fn an_appended_event_is_read_back_once() {
        let dir = tempfile::tempdir().expect("a data directory");
        let path = queue_path(dir.path());
        assert!(append(&path, &event("a", 10)));
        let mut reader = Reader::new();
        let read = reader.read_new(&path);
        assert_eq!(read.len(), 1);
        assert_eq!(read[0].session, "a");
        assert_eq!(read[0].at_unix, 10);
        assert_eq!(read[0].tool.as_deref(), Some("Bash"));
    }

    #[test]
    fn a_reader_reads_each_event_once_and_no_more() {
        let dir = tempfile::tempdir().expect("a data directory");
        let path = queue_path(dir.path());
        assert!(append(&path, &event("a", 10)));
        let mut reader = Reader::new();
        assert_eq!(reader.read_new(&path).len(), 1);
        assert!(
            reader.read_new(&path).is_empty(),
            "because a tick that changes nothing must not rebuild anything, and the queue is \
             the thing that most often does not change"
        );
        assert!(append(&path, &event("b", 20)));
        let read = reader.read_new(&path);
        assert_eq!(read.len(), 1, "so only what is new comes back");
        assert_eq!(read[0].session, "b");
    }

    #[test]
    fn several_appends_in_a_row_all_arrive() {
        let dir = tempfile::tempdir().expect("a data directory");
        let path = queue_path(dir.path());
        for minute in 0..5 {
            assert!(append(&path, &event(&format!("s{minute}"), 100 + minute)));
        }
        let mut reader = Reader::new();
        let read = reader.read_new(&path);
        assert_eq!(
            read.iter().map(|e| e.session.as_str()).collect::<Vec<_>>(),
            ["s0", "s1", "s2", "s3", "s4"],
            "in the order they were appended, oldest first"
        );
    }

    #[test]
    fn a_file_that_does_not_exist_yet_reads_as_nothing_rather_than_as_a_failure() {
        // The running plugin starts before the first hook has ever fired, and a queue that
        // had to exist before it could be read would be a queue that only exists after an
        // event — which is the opposite of a starting point.
        let dir = tempfile::tempdir().expect("a data directory");
        let mut reader = Reader::new();
        assert!(reader.read_new(&queue_path(dir.path())).is_empty());
    }

    #[test]
    fn a_line_that_is_only_half_written_is_left_for_the_next_read() {
        // A hook killed between the write and the flush leaves a tail with no terminator.
        // Treating that as corruption would stop the reader permanently, and the queue would
        // look broken when the only thing wrong with it is that a line is still arriving.
        let dir = tempfile::tempdir().expect("a data directory");
        let path = queue_path(dir.path());
        append(&path, &event("a", 10));
        let whole = std::fs::read_to_string(&path).expect("reads");
        std::fs::write(&path, format!("{whole}{{\"at\":11,\"sess")).expect("writes a half line");
        let mut reader = Reader::new();
        assert_eq!(reader.read_new(&path).len(), 1, "the whole line is read");
        assert!(
            reader.read_new(&path).is_empty(),
            "and the half line is not read, because it is not a line yet"
        );
        std::fs::write(&path, format!("{whole}{{\"at\":11,\"session\":\"b\"}}\n"))
            .expect("completes the line");
        let read = reader.read_new(&path);
        assert_eq!(read.len(), 1, "so once it is finished it arrives");
        assert_eq!(read[0].session, "b");
    }

    #[test]
    fn a_line_that_is_not_readable_is_skipped_and_the_reader_carries_on() {
        // The queue is written by another program, so a line this build cannot read is a
        // fact about that program rather than about this one, and stopping would turn one
        // odd line into a permanently blind queue.
        let dir = tempfile::tempdir().expect("a data directory");
        let path = queue_path(dir.path());
        std::fs::write(
            &path,
            "not json\n{\"at\":2,\"session\":\"b\",\"activity\":\"running\"}\n",
        )
        .expect("writes");
        let mut reader = Reader::new();
        let read = reader.read_new(&path);
        assert_eq!(
            read.len(),
            1,
            "so the bad line is skipped and the good one is read"
        );
        assert_eq!(read[0].session, "b");
        assert!(
            reader.read_new(&path).is_empty(),
            "and the bad line is not re-read on every tick, which would make it a permanent \
             cost"
        );
    }

    #[test]
    fn a_file_that_is_replaced_under_the_reader_is_read_again_rather_than_past_its_end() {
        // Something outside this plugin emptied the file. The reader cannot know why, and
        // the one thing it must not do is read from an offset that is now past the end.
        let dir = tempfile::tempdir().expect("a data directory");
        let path = queue_path(dir.path());
        for minute in 0..3 {
            append(&path, &event(&format!("s{minute}"), 100 + minute));
        }
        let mut reader = Reader::new();
        assert_eq!(reader.read_new(&path).len(), 3);
        std::fs::write(&path, "").expect("emptied");
        assert!(
            reader.read_new(&path).is_empty(),
            "so nothing is read from past the end"
        );
        std::fs::write(
            &path,
            format!(
                "{}\n",
                Event::from_line(&event("fresh", 200).to_line())
                    .expect("an event")
                    .to_line()
            ),
        )
        .expect("writes one");
        let read = reader.read_new(&path);
        assert_eq!(
            read.len(),
            1,
            "and the next event after it is read normally"
        );
        assert_eq!(read[0].session, "fresh");
    }

    #[test]
    fn a_hook_that_cannot_write_its_line_says_so_and_writes_nothing() {
        // A hook's only job is to record an event, and its only failure mode that matters is
        // this one. The directory it was pointed at does not exist is the usual cause.
        let missing = PathBuf::from("/definitely/not/a/directory/anywhere");
        assert!(
            !append(&missing.join(FILE_NAME), &event("a", 1)),
            "so the caller can report it rather than a hook that pretends it worked"
        );
    }

    #[test]
    fn two_appends_at_the_same_moment_both_land() {
        // The property that makes appending the right mechanism at all: a line is either
        // wholly there or not there. Two hooks firing in the same instant — which is exactly
        // what happens when a tool calls two hooks at once — must not lose one.
        let dir = tempfile::tempdir().expect("a data directory");
        let path = queue_path(dir.path());
        let handles: Vec<_> = (0..8)
            .map(|index| {
                let path = path.clone();
                std::thread::spawn(move || {
                    for round in 0..25 {
                        append(&path, &event(&format!("t{index}-{round}"), 1));
                    }
                })
            })
            .collect();
        for handle in handles {
            handle.join().expect("a hook that returns");
        }
        let mut reader = Reader::new();
        assert_eq!(
            reader.read_new(&path).len(),
            200,
            "so every line a hook wrote is a line this side reads"
        );
    }
}
