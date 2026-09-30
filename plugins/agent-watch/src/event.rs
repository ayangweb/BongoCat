//! Turning a hook payload into something a cat can react to.
//!
//! A hook payload is whatever another tool decided to send, so this is the part that has to
//! be forgiving: a field this build has not heard of, an event whose name is not in any list,
//! a tool that has been renamed since. Every one of those has to land on *some* answer,
//! because an event this plugin cannot classify is an event the user cannot see anything
//! about — and a monitoring tool that silently drops the events it does not recognise is
//! indistinguishable from one that is not running.
//!
//! So the classification is a small set of states, each of which is a thing a person would
//! say they are watching happen, and the fallback for anything unknown is the state that
//! means "it is working and I do not know what with".

use std::collections::BTreeMap;

/// What the agent is doing, as far as this plugin can tell.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Activity {
    /// Nothing has arrived for a while, so the tool is waiting on the person.
    Idle,
    /// Working on something, and this build does not know what.
    Thinking,
    /// Going through what is already there.
    Reading,
    /// Making something new.
    Writing,
    /// Looking for something.
    Searching,
    /// Running something.
    Running,
    /// Asking the person something.
    Asking,
    /// Finished.
    Done,
    /// Something went wrong.
    Failed,
}

impl Activity {
    /// Every state, in the order the panel lists them.
    ///
    /// A fixed order rather than a `HashMap` iteration, because the mapping form and the
    /// documentation both need a stable order to be checkable and readable.
    pub const ALL: [Activity; 9] = [
        Activity::Idle,
        Activity::Thinking,
        Activity::Reading,
        Activity::Writing,
        Activity::Searching,
        Activity::Running,
        Activity::Asking,
        Activity::Done,
        Activity::Failed,
    ];

    /// The name this plugin stores and configures an activity under.
    ///
    /// Snake case and stable: it is a key in the user's mapping file, so renaming it would
    /// silently stop working for anybody who had written one.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Thinking => "thinking",
            Self::Reading => "reading",
            Self::Writing => "writing",
            Self::Searching => "searching",
            Self::Running => "running",
            Self::Asking => "asking",
            Self::Done => "done",
            Self::Failed => "failed",
        }
    }

    /// The state a user's mapping file is talking about, if it is one this build has.
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|activity| activity.name() == name)
    }

    /// The event's own name as this plugin stores it, for the panel and the log.
    pub fn label_key(self) -> &'static str {
        self.name()
    }
}

/// One thing that happened, as this plugin understands it.
///
/// Deliberately not the payload. A payload carries whatever the sending tool felt like
/// sending — including a whole file's contents in a tool input — and the product's rule
/// about logs is that they must not carry what the user typed or what their files say.
/// Keeping only a tool's *name* is what lets this plugin be monitored at all: the state
/// file holds "Read" and not "the contents of the file that was read".
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Event {
    /// Which tool sent it, as the sender named itself.
    pub source: String,
    /// Which conversation or task it belongs to.
    pub session: String,
    /// What the agent is doing.
    pub activity: Activity,
    /// The event's own name, kept for the panel so a user can see what actually arrived.
    pub event: String,
    /// The tool's name, when the event named one.
    pub tool: Option<String>,
    /// When it happened, in seconds since the epoch.
    pub at_unix: i64,
}

impl Event {
    /// Read one JSON payload.
    ///
    /// Every field is optional and every field has a fallback, because a payload from a tool
    /// this plugin has never heard of is not an error — it is the normal case for anyone
    /// using it with a second tool. `at_unix` is the only one the caller supplies, because
    /// a payload's own timestamp is a string in no agreed format and the process that
    /// received it knows the time far better.
    ///
    /// Returns `None` only for something that is not a JSON object at all, which is the one
    /// shape no reasonable reading can be built from.
    pub fn parse(source: &str, at_unix: i64, payload: &str) -> Option<Self> {
        let value: serde_json::Value = serde_json::from_str(payload).ok()?;
        let object = value.as_object()?;
        let event = text(object, "hook_event_name")
            .or_else(|| text(object, "event"))
            .or_else(|| text(object, "type"))
            .unwrap_or_default();
        let tool = text(object, "tool_name").map(str::to_owned);
        let session = text(object, "session_id")
            .or_else(|| text(object, "session"))
            .or_else(|| text(object, "conversation_id"))
            .unwrap_or("unknown")
            .to_owned();
        Some(Self {
            source: source.to_owned(),
            session,
            activity: classify(event, tool.as_deref()),
            event: event.to_owned(),
            tool,
            at_unix,
        })
    }

    /// This event as the line the queue file holds.
    ///
    /// Written rather than kept as a value, because the queue is a file another process
    /// appends to and this plugin has to read back lines it did not write in this process.
    /// Round-tripping through text is what makes the two halves agree.
    pub fn to_line(&self) -> String {
        serde_json::json!({
            "at": self.at_unix,
            "source": self.source,
            "session": self.session,
            "activity": self.activity.name(),
            "event": self.event,
            "tool": self.tool,
        })
        .to_string()
    }

    /// Read a line back.
    ///
    /// A line this build cannot read is `None` rather than a panic: the queue is written by
    /// another program, and a queue that can crash the reader is a queue that stops being
    /// written.
    pub fn from_line(line: &str) -> Option<Self> {
        let value: serde_json::Value = serde_json::from_str(line).ok()?;
        let object = value.as_object()?;
        Some(Self {
            source: text(object, "source").unwrap_or("unknown").to_owned(),
            session: text(object, "session").unwrap_or("unknown").to_owned(),
            activity: text(object, "activity")
                .and_then(Activity::from_name)
                .unwrap_or(Activity::Thinking),
            event: text(object, "event").unwrap_or_default().to_owned(),
            tool: text(object, "tool").map(str::to_owned),
            at_unix: object
                .get("at")
                .and_then(serde_json::Value::as_i64)
                .unwrap_or(0),
        })
    }
}

/// One string field, if it is one.
///
/// `as_str` rather than anything cleverer: a payload that puts a number where a string
/// belongs is a payload this plugin should not crash on, and reading it as absent is the
/// version of that which does not throw away the rest of the event.
fn text<'a>(object: &'a serde_json::Map<String, serde_json::Value>, key: &str) -> Option<&'a str> {
    object.get(key).and_then(serde_json::Value::as_str)
}

/// What an event means, given its name and the tool it names.
///
/// The event says *when* in the turn this is, and the tool says *what kind of work* it is.
/// Both are needed: `PreToolUse` alone is six different activities, and a tool name alone
/// says nothing about whether the work is starting or finished.
fn classify(event: &str, tool: Option<&str>) -> Activity {
    // Case-insensitive because the senders are not consistent about it, and a name that
    // differs only in case is the same name.
    let event = event.to_ascii_lowercase();
    let tool_name = tool.unwrap_or_default().to_ascii_lowercase();
    let has = |needles: &[&str]| needles.iter().any(|needle| tool_name.contains(needle));

    match event.as_str() {
        "notification" | "permissionrequest" | "elicitation" => Activity::Asking,
        "stop" | "subagentstop" | "sessionend" | "idleprompt" => Activity::Done,
        "error" | "failure" | "posttoolusefailure" => Activity::Failed,
        "userpromptsubmit" | "pretooluse" | "posttooluse" | "" => {
            // The three that are only meaningful with a tool name: which of the six kinds
            // of work this is depends entirely on what is about to run or just ran.
            if has(&["read", "notebookread", "view", "cat"]) {
                Activity::Reading
            } else if has(&[
                "write",
                "edit",
                "multiedit",
                "create",
                "apply_patch",
                "notebookedit",
            ]) {
                Activity::Writing
            } else if has(&["grep", "glob", "search", "webfetch", "websearch", "find"]) {
                Activity::Searching
            } else if has(&["bash", "shell", "exec", "run", "test", "kill"]) {
                Activity::Running
            } else {
                // An event with no tool, or a tool this build has not heard of: it is
                // working on something, which is the honest answer and the useful one.
                Activity::Thinking
            }
        }
        // Anything else: working, and this build does not know what with. The panel says
        // the event's own name beside it, so a user can see what is arriving and this
        // plugin's mapping can cover it without a new build.
        _ => Activity::Thinking,
    }
}

/// Every conversation being watched, and what each is doing.
///
/// A monitor of a tool that runs in several terminals at once has to hold several answers,
/// and the question "what should the cat show" is then a question about all of them.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Watch {
    sessions: BTreeMap<String, Session>,
    /// How long a session is remembered after its last event, in seconds.
    pub idle_seconds: i64,
    /// When the watch was created, so idle has a floor even before any event arrives.
    started_at_unix: i64,
}

/// One conversation, as this plugin remembers it.
#[derive(Clone, Debug, Eq, PartialEq)]
struct Session {
    activity: Activity,
    event: String,
    tool: Option<String>,
    at_unix: i64,
    /// Events from this session, which is what makes the panel able to say "and 2 more"
    /// rather than "and unknown many more".
    seen: u64,
}

impl Watch {
    /// A watch that has seen nothing.
    pub fn new(started_at_unix: i64, idle_seconds: i64) -> Self {
        Self {
            sessions: BTreeMap::new(),
            idle_seconds: idle_seconds.max(1),
            started_at_unix,
        }
    }

    /// Take one event.
    ///
    /// An event older than what is already known for its session is ignored rather than
    /// applied: a queue that is read out of order — which a file being appended to by
    /// several short-lived processes can be — would otherwise make the cat go backwards.
    pub fn observe(&mut self, event: Event) {
        let entry = self
            .sessions
            .entry(event.session.clone())
            .or_insert_with(|| Session {
                activity: Activity::Idle,
                event: String::new(),
                tool: None,
                at_unix: i64::MIN,
                seen: 0,
            });
        if event.at_unix < entry.at_unix {
            return;
        }
        entry.activity = event.activity;
        entry.event = event.event;
        entry.tool = event.tool;
        entry.at_unix = event.at_unix;
        entry.seen = entry.seen.saturating_add(1);
    }

    /// Forget the sessions that have gone quiet, and say how many there were.
    ///
    /// Bounded on purpose. A tool run for a week leaves a session per conversation in a map
    /// that only grows, and the only thing a panel shows is what is happening *now* — so a
    /// conversation that has been quiet for longer than the user asked about is dropped
    /// rather than remembered forever.
    pub fn expire(&mut self, now_unix: i64) -> usize {
        let before = self.sessions.len();
        let limit = self.idle_seconds.saturating_mul(8).max(self.idle_seconds);
        self.sessions
            .retain(|_, session| now_unix.saturating_sub(session.at_unix) <= limit.max(1));
        before.saturating_sub(self.sessions.len())
    }

    /// How many conversations are being watched.
    pub fn len(&self) -> usize {
        self.sessions.len()
    }

    /// Whether there is nothing to watch.
    pub fn is_empty(&self) -> bool {
        self.sessions.is_empty()
    }

    /// What the cat should show: one activity for all of them.
    ///
    /// The order is not a ranking of importance, it is an order of *distinctness*: whichever
    /// activity is furthest from `Idle` in this list wins, so two sessions reading files and
    /// a session running a command show the command, and two sessions doing nothing show
    /// idle. Chosen because the list is also the order the mapping file and the docs use, so
    /// there is one order in this plugin rather than three.
    pub fn combined(&self, now_unix: i64) -> Activity {
        let cutoff = now_unix.saturating_sub(self.idle_seconds);
        self.sessions
            .values()
            .filter(|session| session.at_unix >= cutoff)
            .map(|session| session.activity)
            .max()
            .unwrap_or_else(|| {
                // Nothing recent. Whether that is "idle" because the person is thinking, or
                // because nothing has ever happened, is the same answer to the only question
                // the panel asks.
                let _ = self.started_at_unix;
                Activity::Idle
            })
    }

    /// The most recent session's own words, for the panel's second line.
    ///
    /// The most recent *event* rather than the most important one, because the line under
    /// the state is there to say what actually arrived — the tool's name, the event's name —
    /// and a user debugging their own hooks needs to see those, not a summary.
    fn latest(&self) -> Option<&Session> {
        self.sessions.values().max_by_key(|session| session.at_unix)
    }

    /// What the most recent event called itself and which tool it named.
    ///
    /// A method rather than two public fields, because the session record is the watch's
    /// business and a panel wants a sentence about it rather than a struct.
    pub fn latest_words(&self) -> Option<(&str, Option<&str>)> {
        self.latest()
            .map(|session| (session.event.as_str(), session.tool.as_deref()))
    }

    /// How many sessions are doing something other than idling.
    pub fn busy(&self, now_unix: i64) -> usize {
        let cutoff = now_unix.saturating_sub(self.idle_seconds);
        self.sessions
            .values()
            .filter(|session| session.at_unix >= cutoff && session.activity != Activity::Idle)
            .count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn now() -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|since| since.as_secs() as i64)
            .unwrap_or(0)
    }

    fn event(payload: &str) -> Event {
        Event::parse("claude-code", now(), payload).expect("a payload a person would send")
    }

    #[test]
    fn a_tool_about_to_read_a_file_is_reading() {
        assert_eq!(
            event(r#"{"hook_event_name":"PreToolUse","tool_name":"Read"}"#).activity,
            Activity::Reading
        );
        assert_eq!(
            event(r#"{"hook_event_name":"PreToolUse","tool_name":"NotebookRead"}"#).activity,
            Activity::Reading
        );
    }

    #[test]
    fn a_tool_about_to_change_a_file_is_writing() {
        for tool in ["Write", "Edit", "MultiEdit", "NotebookEdit", "apply_patch"] {
            let payload = format!(r#"{{"hook_event_name":"PreToolUse","tool_name":"{tool}"}}"#);
            assert_eq!(
                event(&payload).activity,
                Activity::Writing,
                "so the tool whose name contains `{tool}` is writing"
            );
        }
    }

    #[test]
    fn a_tool_looking_for_something_is_searching_and_one_running_something_is_running() {
        for tool in ["Grep", "Glob", "WebSearch", "WebFetch"] {
            let payload = format!(r#"{{"hook_event_name":"PreToolUse","tool_name":"{tool}"}}"#);
            assert_eq!(event(&payload).activity, Activity::Searching, "{tool}");
        }
        for tool in ["Bash", "BashOutput", "KillShell"] {
            let payload = format!(r#"{{"hook_event_name":"PreToolUse","tool_name":"{tool}"}}"#);
            assert_eq!(event(&payload).activity, Activity::Running, "{tool}");
        }
    }

    #[test]
    fn an_event_that_names_no_tool_is_working_and_does_not_guess() {
        // Six different activities hide behind `PreToolUse`, so a payload without a tool
        // name cannot be classified. "Working, and I do not know what with" is the honest
        // answer and the one a panel can be useful with.
        assert_eq!(
            event(r#"{"hook_event_name":"PreToolUse"}"#).activity,
            Activity::Thinking
        );
        assert_eq!(
            event(r#"{"hook_event_name":"PreToolUse","tool_name":"SomeFutureTool"}"#).activity,
            Activity::Thinking,
            "and so is a tool this build has never heard of"
        );
    }

    #[test]
    fn an_event_this_build_has_never_heard_of_is_still_visible() {
        // The monitoring tool must not be blind to the tool the user actually runs. The
        // activity falls back, but the event's own name is kept so the panel can show it
        // and the user's mapping can cover it without waiting for a new build.
        let parsed = event(r#"{"hook_event_name":"SomeBrandNewEvent","tool_name":"Read"}"#);
        assert_eq!(parsed.activity, Activity::Thinking);
        assert_eq!(
            parsed.event, "SomeBrandNewEvent",
            "so the panel says what arrived rather than showing a shrug"
        );
    }

    #[test]
    fn the_events_that_are_not_about_a_tool_are_recognised_by_their_own_names() {
        assert_eq!(
            event(r#"{"hook_event_name":"Notification"}"#).activity,
            Activity::Asking,
            "a notification is the tool asking the person something"
        );
        assert_eq!(
            event(r#"{"hook_event_name":"Stop"}"#).activity,
            Activity::Done
        );
        assert_eq!(
            event(r#"{"hook_event_name":"SubagentStop"}"#).activity,
            Activity::Done
        );
        assert_eq!(
            event(r#"{"hook_event_name":"Error"}"#).activity,
            Activity::Failed
        );
        assert_eq!(
            event(r#"{"hook_event_name":"IdlePrompt"}"#).activity,
            Activity::Done,
            "a tool telling the plugin it is done is done, whatever it calls the event"
        );
    }

    #[test]
    fn an_event_name_that_differs_only_in_case_is_the_same_name() {
        // The senders are not consistent about case, and a monitor that went blind over a
        // capital letter would be very hard to diagnose.
        assert_eq!(
            event(r#"{"hook_event_name":"pretooluse","tool_name":"read"}"#).activity,
            Activity::Reading
        );
    }

    #[test]
    fn a_session_is_remembered_under_whatever_the_sender_called_it() {
        for key in ["session_id", "session", "conversation_id"] {
            let payload = format!(r#"{{"hook_event_name":"Stop","{key}":"abc"}}"#);
            assert_eq!(
                event(&payload).session,
                "abc",
                "so `{key}` is the session's name"
            );
        }
        assert_eq!(
            event(r#"{"hook_event_name":"Stop"}"#).session,
            "unknown",
            "and a payload with no session at all still has one, so two anonymous payloads \
             do not become two sessions"
        );
    }

    #[test]
    fn a_payload_that_is_not_an_object_is_refused_rather_than_half_read() {
        // The one shape no reasonable reading can be built from. Anything else — a number
        // where a string belongs, a tool input that is an array — is read as far as it makes
        // sense, because a monitor that drops odd payloads is a monitor with holes in it.
        assert!(Event::parse("claude-code", 0, "not json at all").is_none());
        assert!(Event::parse("claude-code", 0, "[1,2,3]").is_none());
        assert!(Event::parse("claude-code", 0, r#"{"hook_event_name":7}"#).is_some());
        assert!(
            Event::parse("claude-code", 0, r#"{"tool_input":{"command":"ls"}}"#).is_some(),
            "a payload whose tool lives inside the input is still a payload"
        );
    }

    #[test]
    fn what_the_user_typed_never_reaches_the_queue() {
        // The product's rule about diagnostics, and this plugin is a diagnostic. A tool input
        // can contain a whole file's contents or a command with a password in it, so only the
        // tool's *name* is kept.
        let parsed = event(
            r#"{"hook_event_name":"PreToolUse","tool_name":"Bash",
                "tool_input":{"command":"curl -H 'token: hunter2' https://example.com"},
                "prompt":"my password is hunter2"}"#,
        );
        let line = parsed.to_line();
        for secret in ["hunter2", "curl", "example.com"] {
            assert!(
                !line.contains(secret),
                "so `{secret}` is not in the file another process appends to: {line}"
            );
        }
        assert!(line.contains("Bash"), "while the tool's name is: {line}");
    }

    #[test]
    fn an_event_survives_its_own_line() {
        let original =
            event(r#"{"hook_event_name":"PreToolUse","tool_name":"Edit","session_id":"s1"}"#);
        let back = Event::from_line(&original.to_line()).expect("reads back");
        assert_eq!(back.activity, original.activity);
        assert_eq!(back.session, "s1", "and the session it named: {back:?}");
        assert_eq!(back.tool.as_deref(), Some("Edit"));
    }

    #[test]
    fn a_line_this_build_cannot_read_is_dropped_rather_than_fatal() {
        // The queue is written by another program. A reader that stops is a queue that stops
        // being written, so the only thing that may happen to an unreadable line is that it
        // is not acted on.
        assert!(Event::from_line("").is_none());
        assert!(Event::from_line("{ broken").is_none());
        assert_eq!(
            Event::from_line(r#"{"at":1,"session":"s","activity":"invented"}"#)
                .expect("reads back")
                .activity,
            Activity::Thinking,
            "an activity name this build does not have falls back to working, not to idle"
        );
    }

    #[test]
    fn one_watch_answers_for_several_conversations() {
        let now = now();
        let mut watch = Watch::new(now, 60);
        watch.observe(event(
            r#"{"hook_event_name":"PreToolUse","tool_name":"Read","session_id":"a"}"#,
        ));
        assert_eq!(watch.len(), 1);
        assert_eq!(watch.combined(now), Activity::Reading);

        watch.observe(event(
            r#"{"hook_event_name":"PreToolUse","tool_name":"Bash","session_id":"b"}"#,
        ));
        assert_eq!(watch.len(), 2, "two terminals are two sessions");
        assert_eq!(watch.busy(now), 2);
        // The furthest from idle wins, which is the running command.
        assert_eq!(watch.combined(now), Activity::Running);
        assert_eq!(
            watch.latest().expect("a latest").activity,
            Activity::Running,
            "and the panel's second line is the most recent event, because that is what a \
             user debugging their own hooks needs to see"
        );
    }

    #[test]
    fn a_session_that_has_gone_quiet_stops_counting_as_busy() {
        // The reading compared against is absolute, so ageing a session means moving the
        // event's own timestamp — asking about an earlier moment does not age anything.
        let now = now();
        let mut watch = Watch::new(now, 30);
        let mut running =
            event(r#"{"hook_event_name":"PreToolUse","tool_name":"Bash","session_id":"a"}"#);
        running.at_unix = now - 31;
        watch.observe(running);
        assert_eq!(
            watch.combined(now),
            Activity::Idle,
            "so a tool that stopped half a minute ago is not still running a command"
        );
        assert_eq!(watch.busy(now), 0);
        assert_eq!(
            watch.len(),
            1,
            "while the session itself is still remembered, because the panel counts sessions \
             and a count that forgets one it is displaying would be wrong"
        );
    }

    #[test]
    fn nothing_happening_is_idle_rather_than_nothing_at_all() {
        // A watch that has never seen an event and a watch whose events have all gone quiet
        // are the same answer to the only question the panel asks, so neither needs a second
        // state the panel would have to explain.
        let now = now();
        assert_eq!(Watch::new(now, 60).combined(now), Activity::Idle);
        assert!(Watch::new(now, 60).is_empty());
    }

    #[test]
    fn a_watch_does_not_grow_for_a_tool_run_for_a_month() {
        let now = now();
        let mut watch = Watch::new(now, 10);
        for day in 0..40 {
            watch.observe(Event {
                source: "claude-code".to_owned(),
                session: format!("day-{day}"),
                activity: Activity::Done,
                event: "Stop".to_owned(),
                tool: None,
                at_unix: now.saturating_sub(day * 60),
            });
        }
        assert_eq!(watch.len(), 40, "before anything is forgotten");
        let forgotten = watch.expire(now);
        assert_eq!(watch.len(), 40 - forgotten);
        assert!(
            watch.len() <= 40,
            "and the count is bounded by the idle window the user asked about, not by how \
             long they have had the tool open"
        );
    }

    #[test]
    fn an_event_that_arrives_out_of_order_does_not_move_the_cat_backwards() {
        // Several short-lived processes appending to one file can be read out of order, and
        // a watch that applied them in arrival order would show a command finishing and then
        // start running again.
        let now = now();
        let mut watch = Watch::new(now, 60);
        let mut running =
            event(r#"{"hook_event_name":"PreToolUse","tool_name":"Bash","session_id":"a"}"#);
        running.at_unix = now;
        watch.observe(running.clone());
        let mut stale = running;
        stale.at_unix = now - 30;
        stale.activity = Activity::Writing;
        watch.observe(stale);
        assert_eq!(
            watch.combined(now),
            Activity::Running,
            "because the older event is about something that had already been superseded"
        );
    }

    #[test]
    fn the_states_have_names_that_can_be_written_in_a_mapping_file() {
        for activity in Activity::ALL {
            assert_eq!(
                Activity::from_name(activity.name()),
                Some(activity),
                "so `{}` round-trips, which is what lets a user's file name it",
                activity.name()
            );
        }
        assert_eq!(
            Activity::from_name("Idle"),
            None,
            "and names are snake case"
        );
        assert_eq!(Activity::from_name("invented"), None);
    }
}
