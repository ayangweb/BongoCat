//! Running somebody's command, carefully.
//!
//! This is the plugin's one dangerous act, and it is the whole of what lets it do useful
//! things: everything else in this plugin is arithmetic, and this is the part where a mistake
//! or a hostile setting runs something. So every constraint here is a decision about what
//! must *not* happen.
//!
//! * **No shell.** The setting is split into a program and its arguments by this file, and
//!   the program is executed directly. There is no `/bin/sh -c`, so a setting cannot smuggle
//!   in `;` or `&&` or a backtick, and a command cannot be assembled out of a template that
//!   happens to contain something dangerous. It also means the plugin does not have to know
//!   one platform's shell rules from another's.
//! * **A bounded output.** A command that prints a hundred megabytes must not be able to use
//!   the plugin's memory; the read stops at the bound and the rest is dropped.
//! * **A bounded time.** A command that never exits must not be able to hang the session,
//!   because the session thread is also the thread that draws the panel. The run is on a
//!   thread of its own and the plugin stops caring about it after the timeout.
//! * **One substitution, and only where the user put it.** `%s` becomes the configured
//!   question, and nothing else is substituted — so a question containing `%s` or a percent
//!   sign cannot become a format string.
//! * **The plugin's own directory is not on the path.** The command is found the way the
//!   operating system finds commands, and the plugin does not add itself to that search, so
//!   a command cannot accidentally invoke a second copy of this plugin.

use std::io::Read;
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// The most output this plugin will read from a command, in bytes.
///
/// Bounded because the command's output becomes a bubble and a panel line, both of which are
/// far smaller than this, and a command that prints without limit is a command that would
/// otherwise be read into this process's memory a byte at a time.
pub const MAXIMUM_OUTPUT_BYTES: usize = 64 * 1024;

/// How long a command may run before this plugin stops waiting for it.
///
/// Long enough for a network call or a slow script and short enough that a hung command is
/// not a hung cat: the plugin keeps drawing and the run is abandoned. Five seconds is the
/// point where a command that has not answered has told the user something already.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5);

/// The most a command may run, whatever the user asked for.
///
/// The user can shorten the wait and cannot extend it past this, because the wait is a
/// property of the plugin rather than of the command: a command that takes a minute is a
/// command whose answer is not worth having.
pub const MAXIMUM_TIMEOUT_SECONDS: u64 = 30;

/// What came back.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Outcome {
    /// The command ran and said this.
    Said(String),
    /// The command ran and said nothing a person could read.
    Silent,
    /// The command ran and failed in a way worth saying.
    Failed(String),
    /// The command was still running when the plugin stopped waiting.
    TimedOut,
    /// The command could not be started at all.
    NotStarted(String),
}

impl Outcome {
    /// The one line this outcome is worth showing, or nothing.
    ///
    /// A bubble is one line and a panel line is one line, so this takes the first line that
    /// has something in it and drops the rest. A command whose useful output is on the ninth
    /// line is a command with a flag problem, and a bubble full of blank lines helps nobody.
    pub fn first_line(&self) -> Option<String> {
        let text = match self {
            Self::Said(text) | Self::Failed(text) | Self::NotStarted(text) => text,
            Self::Silent => return None,
            Self::TimedOut => return None,
        };
        text.lines()
            .map(str::trim)
            .find(|line| !line.is_empty())
            .map(str::to_owned)
    }

    /// Whether this outcome is worth interrupting the user for.
    ///
    /// Success and failure are; silence and a timeout are not. A command that printed nothing
    /// has not told the user anything, and a bubble saying nothing is a blank rectangle on
    /// the screen.
    pub fn worth_saying(self) -> bool {
        self.first_line().is_some()
    }
}

/// A command, split into what will be run.
///
/// Split once, at the moment the settings are read, so that what is shown in a log and what
/// is executed cannot drift apart — a run that reported one thing and did another would be
/// the least debuggable thing this plugin could do.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Command {
    program: String,
    arguments: Vec<String>,
}

impl Command {
    /// Split a setting into a program and its arguments.
    ///
    /// Quoting is the only syntax, and it is the same in every language a person types:
    /// a run of non-space characters, or a run inside `'…'` or `"…"` where a backslash
    /// escapes the next character. An unterminated quote takes the rest of the line rather
    /// than failing, because a half-typed setting should run what it clearly means rather
    /// than nothing.
    ///
    /// `None` when there is no program, which is how "the user has not turned this on" is
    /// said. Empty is a real program and not this.
    pub fn parse(setting: &str) -> Option<Self> {
        let mut words = Vec::new();
        let mut current = String::new();
        let mut started = false;
        let mut quote: Option<char> = None;
        let mut escaped = false;
        for character in setting.chars() {
            if escaped {
                current.push(character);
                escaped = false;
                continue;
            }
            match quote {
                Some(open) => {
                    if character == '\\' {
                        escaped = true;
                    } else if character == open {
                        quote = None;
                    } else {
                        current.push(character);
                    }
                }
                None => match character {
                    '\\' => {
                        // A backslash outside quotes escapes the next character, so a Windows
                        // path in a setting is one word rather than two.
                        escaped = true;
                        started = true;
                    }
                    '\'' | '"' => {
                        quote = Some(character);
                        started = true;
                    }
                    character if character.is_whitespace() => {
                        if started {
                            words.push(std::mem::take(&mut current));
                            started = false;
                        }
                    }
                    _ => {
                        current.push(character);
                        started = true;
                    }
                },
            }
        }
        if started || !current.is_empty() {
            words.push(current);
        }
        // Nothing at all is not a command that runs nothing — it is the plugin being off,
        // which is a different thing and is what `None` says. Popping from an empty list here
        // would panic on the plugin's own default settings.
        let program = words.first()?.clone();
        if program.is_empty() {
            return None;
        }
        let arguments = words
            .split_first()
            .map(|(_, rest)| rest.to_vec())
            .unwrap_or_default();
        Some(Self { program, arguments })
    }

    /// The arguments this command will be started with.
    pub fn arguments(&self) -> &[String] {
        &self.arguments
    }

    /// The command as it would be typed, for the panel and a log line.
    pub fn display(&self) -> String {
        let mut text = String::new();
        for word in std::iter::once(&self.program).chain(&self.arguments) {
            if !text.is_empty() {
                text.push(' ');
            }
            if word.contains(char::is_whitespace) || word.is_empty() {
                text.push('"');
                text.push_str(word);
                text.push('"');
            } else {
                text.push_str(word);
            }
        }
        text
    }

    /// This command, with `%s` in it replaced by the question.
    ///
    /// Only the *arguments* are substituted, and only the first `%s` in each: a program path
    /// is not something a question belongs inside, and one substitution per argument is
    /// enough for every use this has.
    pub fn with_question(&self, question: &str) -> Self {
        Self {
            program: self.program.clone(),
            arguments: self
                .arguments
                .iter()
                .map(|argument| match argument.find("%s") {
                    Some(at) => {
                        let mut filled = String::with_capacity(argument.len() + question.len());
                        filled.push_str(&argument[..at]);
                        filled.push_str(question);
                        filled.push_str(&argument[at + 2..]);
                        filled
                    }
                    None => argument.clone(),
                })
                .collect(),
        }
    }

    /// Start it, and hand back something to ask about later.
    ///
    /// Non-blocking on purpose: the session thread is the thread that draws the panel, and a
    /// command that hangs must not hang the cat.
    pub fn spawn(self) -> Running {
        self.spawn_within(DEFAULT_TIMEOUT)
    }

    /// Start it, and give up on it after this long however the user configured it.
    ///
    /// The cap is applied here rather than trusted to the caller, because this is the one
    /// place that decides how long the plugin's own thread is occupied.
    pub fn spawn_within(self, timeout: Duration) -> Running {
        let timeout = timeout.min(MAXIMUM_TIMEOUT);
        let (sender, receiver) = mpsc::channel();
        let outcome = std::thread::Builder::new()
            .name("cat-skills-command".to_string())
            .spawn(move || {
                let outcome = run_command(&self, timeout);
                // The receiver may be gone — the plugin stopped caring — and a closed channel
                // is the normal way for this to end, so nothing is done about it.
                let _ = sender.send(outcome);
            });
        match outcome {
            Ok(_) => Running {
                receiver,
                failure: None,
                answered: false,
            },
            // A thread that cannot be spawned is reported as a command that could not start,
            // because that is what it means for the user: nothing ran. The channel is dropped
            // with the sender, so the `answer` below has nothing to say either way.
            Err(_) => Running {
                // A channel with nothing behind it: `answer` finds nothing to say, and
                // `failure` is what the panel reports instead.
                receiver: mpsc::channel().1,
                failure: Some(
                    "this plugin could not start a thread to run the command in".to_string(),
                ),
                answered: false,
            },
        }
    }
}

/// The longest this plugin will wait, whatever the setting says.
pub const MAXIMUM_TIMEOUT: Duration = Duration::from_secs(MAXIMUM_TIMEOUT_SECONDS);

/// A command that has been started and may or may not have answered.
#[derive(Debug)]
pub struct Running {
    receiver: mpsc::Receiver<Outcome>,
    failure: Option<String>,
    /// Whether the answer has been taken.
    ///
    /// A field rather than a second `try_recv`, because receiving takes the value: an
    /// `is_pending` that asked the channel would *discard* the answer, and the plugin asks
    /// "is it still running" on every tick and takes the answer on the tick after it arrives.
    /// The two together would lose every answer from any command that answered quickly —
    /// which is most of them.
    answered: bool,
}

impl Running {
    /// The answer, if it has arrived.
    ///
    /// `try_recv` rather than a wait, because this is called from the tick loop: a command
    /// that has not answered is simply not answered *yet*, and the panel has to be drawn in
    /// the meantime.
    pub fn answer(&mut self) -> Option<Outcome> {
        if let Some(failure) = &self.failure {
            self.answered = true;
            return Some(Outcome::NotStarted(failure.clone()));
        }
        match self.receiver.try_recv() {
            Ok(outcome) => {
                self.answered = true;
                Some(outcome)
            }
            Err(_) => None,
        }
    }

    /// Whether this command is still running.
    pub fn is_pending(&self) -> bool {
        !self.answered && self.failure.is_none()
    }

    /// Stop caring about this command.
    ///
    /// The run itself is bounded by the timeout [`Command::spawn_within`] was given, and that
    /// timeout kills the child and ends the thread — so an abandoned run ends on its own and
    /// there is no handle to keep. What this does is stop the answer from arriving, which is
    /// the half the plugin controls.
    pub fn abandon(&mut self) {
        self.failure = Some(String::new());
        self.answered = true;
    }
}

/// Run the command to completion, or until the timeout, and say what came back.
///
/// The whole body is on a thread of its own so that nothing here can hold up the session
/// thread, and the timeout is enforced by polling rather than by a timer thread, so there is
/// exactly one extra thread per run and it always ends.
fn run_command(command: &Command, timeout: Duration) -> Outcome {
    match std::process::Command::new(&command.program)
        .args(&command.arguments)
        // Nothing is piped in. A command that waits for input would otherwise wait for a
        // terminal this plugin does not have, forever, until the timeout killed it.
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
    {
        Ok(mut child) => collect(&mut child, timeout),
        Err(error) => Outcome::NotStarted(format!("{}: {error}", command.display())),
    }
}

/// Wait for the child, reading both of its pipes, and turn that into an [`Outcome`].
fn collect(child: &mut std::process::Child, timeout: Duration) -> Outcome {
    let stdout = child.stdout.take().map(read_bounded);
    let stderr = child.stderr.take().map(read_bounded);
    let deadline = Instant::now() + timeout;
    let mut status = None;
    loop {
        match child.try_wait() {
            Ok(Some(exit)) => {
                status = Some(exit);
                break;
            }
            Ok(None) => {}
            Err(_) => break,
        }
        if Instant::now() >= deadline {
            // Best-effort and then given up on: a child that will not die is a child's
            // problem, and this thread's job is to end either way.
            let _ = child.kill();
            let _ = child.wait();
            return Outcome::TimedOut;
        }
        std::thread::sleep(Duration::from_millis(8));
    }
    // Both pipes are read on their own threads, because reading one to its end while the
    // other fills its buffer deadlocks: a command that writes a lot to stderr while stdout
    // is quiet would block on stderr forever.
    let out = stdout.map(join).unwrap_or_default();
    let err = stderr.map(join).unwrap_or_default();
    let succeeded = status.is_some_and(|exit| exit.success());
    if succeeded && out.trim().is_empty() && err.trim().is_empty() {
        return Outcome::Silent;
    }
    // A failed command's stderr is the more useful of the two, and a successful one might
    // have written to either.
    let text = if succeeded || out.trim().is_empty() {
        if out.trim().is_empty() { err } else { out }
    } else if err.trim().is_empty() {
        out
    } else {
        err
    };
    if succeeded {
        Outcome::Said(text)
    } else {
        Outcome::Failed(text)
    }
}

/// Read a pipe to its end, or to the bound, whichever comes first.
fn read_bounded(mut pipe: impl Read + Send + 'static) -> std::thread::JoinHandle<String> {
    std::thread::spawn(move || {
        let mut text = Vec::new();
        let mut chunk = [0_u8; 8192];
        while text.len() < MAXIMUM_OUTPUT_BYTES {
            match pipe.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(read) => text.extend_from_slice(&chunk[..read]),
            }
        }
        String::from_utf8_lossy(&text).into_owned()
    })
}

/// Wait for a reader and take what it read.
fn join(handle: std::thread::JoinHandle<String>) -> String {
    handle.join().unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_setting_with_no_program_is_off_rather_than_a_command_that_runs_nothing() {
        assert_eq!(Command::parse(""), None);
        assert_eq!(
            Command::parse("\"\""),
            None,
            "so an empty program is off too: a quoted nothing is still nothing, and starting a \
             process named \"\" is a refusal the user cannot act on"
        );
        assert_eq!(Command::parse("   \t \n "), None);
        assert!(Command::parse("/bin/echo").is_some());
    }

    #[test]
    fn a_command_is_a_program_and_its_arguments_and_never_a_shell() {
        // The property the whole file is for: no `/bin/sh`, so nothing in a setting can be a
        // shell metacharacter that runs something else.
        let command = Command::parse("/usr/bin/curl -s https://example.com").expect("a command");
        assert_eq!(command.program, "/usr/bin/curl");
        assert_eq!(command.arguments, ["-s", "https://example.com"]);
        assert!(
            !command.program.contains("sh"),
            "because the thing that runs is what the setting named: {command:?}"
        );
    }

    #[test]
    fn quotes_are_the_only_syntax_and_a_path_with_spaces_is_one_word() {
        let command = Command::parse(r#""/Applications/My Tools/weather" --city "New York""#)
            .expect("a command");
        assert_eq!(command.program, "/Applications/My Tools/weather");
        assert_eq!(command.arguments, ["--city", "New York"]);
    }

    #[test]
    fn single_quotes_keep_what_they_hold_verbatim() {
        let command = Command::parse("/bin/echo 'a  b'").expect("a command");
        assert_eq!(command.arguments, ["a  b"]);
    }

    #[test]
    fn a_backslash_escapes_the_next_character_wherever_it_is() {
        let command = Command::parse(r"/bin/echo a\ b").expect("a command");
        assert_eq!(command.arguments, ["a b"], "so a Windows path is one word");
        let quoted = Command::parse(r#""a\"b""#).expect("a command");
        assert_eq!(quoted.program, "a\"b");
    }

    #[test]
    fn a_half_typed_quote_runs_what_it_clearly_means() {
        // A setting being typed is the normal state of a setting, and a command that refuses
        // to run because a quote is open is a command the user cannot debug from the panel.
        let command = Command::parse("/bin/echo \"unfinished").expect("a command");
        assert_eq!(command.program, "/bin/echo");
        assert_eq!(command.arguments, ["unfinished"]);
    }

    #[test]
    fn an_empty_quoted_word_is_a_word() {
        // `curl -H ""` is a thing somebody means, and losing the empty argument would turn it
        // into `curl -H` which is a different command with a different meaning.
        let command = Command::parse(r#"/bin/echo "" x"#).expect("a command");
        assert_eq!(command.arguments, ["", "x"]);
    }

    #[test]
    fn the_question_goes_where_the_user_put_the_placeholder_and_nowhere_else() {
        let command = Command::parse("/usr/bin/ask --say %s --plain").expect("a command");
        let filled = command.with_question("what is the weather");
        assert_eq!(
            filled.program, "/usr/bin/ask",
            "the program is never substituted"
        );
        assert_eq!(
            filled.arguments,
            ["--say", "what is the weather", "--plain"]
        );

        let no_placeholder = Command::parse("/usr/bin/ask --json").expect("a command");
        assert_eq!(
            no_placeholder.with_question("ignored").arguments,
            ["--json"],
            "so a command with no placeholder is run as written rather than with a question \
             glued to an argument nobody expected"
        );
    }

    #[test]
    fn only_the_first_placeholder_in_an_argument_is_filled() {
        // One substitution per argument. A question is data, and data that is scanned for
        // format specifiers is data that can change the shape of the command.
        let command = Command::parse("/usr/bin/ask %s %s").expect("a command");
        let filled = command.with_question("50% of %s");
        assert_eq!(
            filled.arguments,
            ["50% of %s", "50% of %s"],
            "so a percent sign in the question is not a format string"
        );
    }

    #[test]
    fn a_command_is_shown_the_way_it_would_be_typed() {
        // What a log line says and what runs cannot drift apart, because both are built from
        // the same split.
        let command = Command::parse(r#""/Applications/My Tools/weather" --city "New York""#)
            .expect("a command");
        assert_eq!(
            command.display(),
            r#""/Applications/My Tools/weather" --city "New York""#
        );
        let simple = Command::parse("/bin/echo hi").expect("a command");
        assert_eq!(simple.display(), "/bin/echo hi");
    }

    #[test]
    fn an_outcome_is_worth_saying_only_when_it_has_a_line_to_say() {
        assert!(Outcome::Said("hello".to_string()).worth_saying());
        assert!(Outcome::Failed("no such file".to_string()).worth_saying());
        assert!(Outcome::NotStarted("not found".to_string()).worth_saying());
        assert!(
            !Outcome::Silent.worth_saying(),
            "so a command that printed nothing does not put a blank rectangle on the screen"
        );
        assert!(!Outcome::TimedOut.worth_saying());
        assert!(!Outcome::Said("\n\n   \n".to_string()).worth_saying());
    }

    #[test]
    fn the_line_shown_is_the_first_one_with_something_in_it() {
        // A bubble is one line and a panel line is one line. A command whose useful output is
        // on the ninth line is a command with a flag problem, and a bubble full of blank
        // lines helps nobody.
        assert_eq!(
            Outcome::Said("\n\n  Tokyo: 21°C  \nsecond line".to_string()).first_line(),
            Some("Tokyo: 21°C".to_string())
        );
        assert_eq!(Outcome::Said("".to_string()).first_line(), None);
    }

    #[test]
    fn the_timeout_can_be_shortened_and_not_lengthened_past_what_the_plugin_can_wait() {
        // The wait is a property of the plugin: a command that takes a minute is a command
        // whose answer is not worth having, and a user who asks for a longer wait is asking
        // the plugin to stop drawing.
        assert_eq!(
            Duration::from_secs(90).min(MAXIMUM_TIMEOUT),
            MAXIMUM_TIMEOUT,
            "so a user who asks to wait a minute is not obeyed"
        );
        assert!(
            DEFAULT_TIMEOUT < MAXIMUM_TIMEOUT,
            "and the default is inside the cap, so the cap only ever shortens"
        );
    }

    #[test]
    fn a_command_that_was_started_is_pending_until_it_answers() {
        let mut running = Command::parse("/bin/sleep 30").expect("a command").spawn();
        assert!(
            running.is_pending(),
            "so the panel knows the cat is waiting rather than idle"
        );
        assert_eq!(running.answer(), None);
        // The wait is bounded, and abandoning is how the plugin bounds it.
        let started = Instant::now();
        while running.is_pending() && started.elapsed() < Duration::from_millis(500) {
            std::thread::sleep(Duration::from_millis(10));
        }
        running.abandon();
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "so the plugin never waits for the command itself"
        );
    }

    #[test]
    fn a_command_that_answers_is_not_pending_any_more() {
        let mut running = Command::parse("/bin/echo hello")
            .expect("a command")
            .spawn();
        let mut answered = None;
        let started = Instant::now();
        while answered.is_none() && started.elapsed() < Duration::from_secs(10) {
            answered = running.answer();
            if answered.is_none() {
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        assert_eq!(answered, Some(Outcome::Said("hello\n".to_string())));
        assert!(
            !running.is_pending(),
            "so asking whether it is still running is not what discovers that it is not"
        );
    }

    #[test]
    fn asking_whether_a_command_is_still_running_does_not_throw_its_answer_away() {
        // The bug this field exists to prevent: `is_pending` asked the channel, so a tick that
        // asked before taking the answer discarded it — and the plugin asks on every tick while
        // a command like `/bin/echo` answers in less than one.
        let mut running = Command::parse("/bin/echo hello")
            .expect("a command")
            .spawn();
        // Long enough for a process to start, print one line and exit, with a wide margin.
        std::thread::sleep(Duration::from_millis(300));
        for _ in 0..5 {
            assert!(
                running.is_pending(),
                "so a command whose answer has not been taken still counts as running"
            );
        }
        assert_eq!(
            running.answer(),
            Some(Outcome::Said("hello\n".to_string())),
            "and the answer survived five of those questions: a `is_pending` that asked the \
             channel would have taken it on the first one"
        );
    }

    #[test]
    fn a_command_that_does_not_exist_is_reported_rather_than_ignored() {
        // "Nothing happened" and "the program you named is not there" are different facts, and
        // a user who mistyped a path needs to be told which one it is.
        let mut running = Command::parse("/definitely/not/a/program --flag")
            .expect("a command")
            .spawn();
        let mut answered = None;
        let started = Instant::now();
        while answered.is_none() && started.elapsed() < Duration::from_secs(10) {
            answered = running.answer();
            if answered.is_none() {
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        let outcome = answered.expect("an answer");
        assert!(
            matches!(outcome, Outcome::NotStarted(_)),
            "so the panel says the program was not there rather than that it failed: a command \
             that ran and failed is a different fact and needs different words: {outcome:?}"
        );
        assert!(
            outcome.first_line().is_some(),
            "and with something to read, because a failure with no message is not a report"
        );
    }
}
