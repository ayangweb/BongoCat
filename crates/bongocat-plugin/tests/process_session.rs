//! The whole path, from a directory on disk to a running panel, as one test file.
//!
//! Every other test in this crate checks a piece: that a manifest parses, that a queue
//! drops, that a press is checked against what was drawn. None of them starts a process,
//! and a process is the thing this architecture is actually about — a plugin is a
//! program, so "the host can talk to a program" is a claim only a test which runs one can
//! support.
//!
//! # Why this file has its own `main`
//!
//! The plugin is **this binary**, asked to behave as one: the host sets
//! `BONGOCAT_PLUGIN_ID` in the child's environment, and a `main` that finds it speaks the
//! protocol instead of running the tests. That needs a plain `main` rather than
//! `libtest`, for one concrete reason: `libtest` captures the output of a running test and
//! prints it when the test ends, so a child that wrote protocol lines from inside a
//! `#[test]` would have had them swallowed and the host would have seen nothing at all.
//! A plugin's stdout is the wire, and a wire cannot be captured.
//!
//! So this target is `harness = false` and runs its own handful of cases. What that costs
//! is parallelism and libtest's reporting, both of which are a fair price for a test that
//! proves the architecture works, and what it buys is that the plugin half is a real
//! program writing real messages through the real encoder — on both platforms, with no
//! shell, and with no way to drift from the protocol without breaking this file.
//!
//! # What it does not prove
//!
//! It does not prove that *the pomodoro* works. That plugin's own crate tests its logic
//! through the SDK's session. This file proves the other half: that a process saying
//! those things is believed, started, drawn, pressed and stopped.

use bongocat_plugin::{
    CatalogMode, Incoming, LocalTimeCache, PluginCommand, PluginEntry, PluginId, PluginSnapshot,
    PluginStore, PluginVersion, PluginWorkerReader, Session, SessionOutcome, SessionState,
};
use bongocat_plugin_protocol::{
    ButtonNode, ConfigDocument, ConfigValue, HostMessage, PanelPlacement, PanelUpdate,
    PluginAnchor, PluginMessage, SceneNode, StackNode, Subscription,
};
use bongocat_plugin_render::TextMeasurer;
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::panic::AssertUnwindSafe;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// The id this binary's plugin half answers to, and the directory it is unpacked into.
const ID: &str = "probe";

/// The environment variable the host sets in every child, and the only thing that tells
/// this binary it was started as a plugin rather than run as the test suite.
const SPAWNED_AS: &str = "BONGOCAT_PLUGIN_ID";

/// The button the plugin declares, so a press has something to land on.
const BUTTON: &str = "probe-button";

/// The panel's own size, which is also the space a press is tested in.
const PANEL_SIZE: [u32; 2] = [200, 100];

/// How long a test waits for a process to do the next thing.
///
/// Long enough for a cold start on a loaded machine, and short enough that a plugin which
/// never answers fails the test rather than stalling the suite. The production bound is
/// thirty seconds because a real plugin may load a font and open a socket before it
/// speaks; this one does nothing but print.
const PATIENCE: Duration = Duration::from_secs(20);

/// Every case this file runs, in the order a reader would want them.
const CASES: &[(&str, fn())] = &[
    (
        "an archive this file wrote becomes an installed plugin the store will start",
        an_archive_becomes_an_installed_plugin,
    ),
    (
        "a process that announces itself is believed and named by its own words",
        a_process_is_believed,
    ),
    (
        "a panel a process drew is rasterized and its button is pressable",
        a_panel_is_pressable,
    ),
    (
        "a press outside every button is not a press",
        a_press_outside_is_not_a_press,
    ),
    (
        "a plugin reports its own settings and the host believes them",
        a_plugin_reports_its_settings,
    ),
    (
        "a panel with two buttons of one id is refused whole",
        an_ambiguous_panel_is_refused,
    ),
    (
        "a process that is asked to stop ends on its own and loses its panel",
        a_process_stops_on_its_own,
    ),
    (
        "a switch the settings window draws reaches the plugin center",
        a_switch_turned_off_is_published,
    ),
    (
        "an unpacked executable is made runnable even when the archive carried no mode",
        an_unpacked_executable_is_made_runnable,
    ),
    (
        "a directory with no executable is a refusal naming the file that is missing",
        a_missing_executable_is_a_refusal,
    ),
];

fn main() -> std::process::ExitCode {
    if spawned_as_plugin() {
        speak();
        return std::process::ExitCode::SUCCESS;
    }
    // Nothing is printed. The repository denies both `clippy::print_stdout` and
    // `clippy::print_stderr`, and the rule is right: on the product side a line on either
    // stream is a line something else owns, and on the plugin side stdout *is* the wire.
    // So a case reports by panicking, which the default hook already prints, and the run
    // reports by its exit code.
    let mut failures: Vec<(String, String)> = Vec::new();
    for (name, case) in CASES {
        // Each case on its own, and a failure does not stop the next one: the cases are
        // independent, and a run that reported only the first failure would be a run that
        // has to be repeated after every fix.
        if let Err(payload) = std::panic::catch_unwind(AssertUnwindSafe(case)) {
            failures.push(((*name).to_owned(), describe(payload)));
        }
    }
    if failures.is_empty() {
        return std::process::ExitCode::SUCCESS;
    }
    panic!(
        "{} of {} cases failed:\n{}",
        failures.len(),
        CASES.len(),
        failures
            .iter()
            .map(|(name, detail)| format!("  {name}\n    {detail}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

/// What a caught panic said, as a string.
///
/// A panic payload is whatever the panicking code boxed, so the two cases worth handling
/// are the two `panic!` actually produces; anything else is reported as itself having
/// panicked rather than as a message nobody wrote.
fn describe(payload: Box<dyn std::any::Any + Send>) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|message| (*message).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "panicked without a message".to_owned())
}

// ---------------------------------------------------------------------------
// The host half.
// ---------------------------------------------------------------------------

/// One plugin on disk and the session talking to it.
struct Probe {
    /// Kept alive because the directories below live inside it.
    _root: tempfile::TempDir,
    data: PathBuf,
    directory: PathBuf,
    session: Option<Session>,
    /// What the child wrote to its own stderr, for a failure message.
    stderr: Vec<String>,
}

impl Probe {
    /// A plugin directory holding this binary and a manifest that names it.
    ///
    /// Written directly rather than through the store's `unpack`, and the reason is the
    /// store's own member bound: a debug test binary is tens of megabytes and the store
    /// refuses a member over four, which is a bound a release plugin is nowhere near and
    /// a test double is. The store's own path is exercised by the first case, with an
    /// archive that is a manifest and nothing else.
    ///
    /// Also where the recursion stops. A child is this same binary, so without the guard
    /// the child would run the suite, reach a case that starts a plugin, and start its own
    /// child — and the guard costs one line here rather than one in each of the six cases
    /// that follow.
    fn on_disk() -> Self {
        assert!(
            !spawned_as_plugin(),
            "a plugin process must not start another one: the recursion guard in `main` is what \\
             makes this binary's own cases stop, and a case that reached here anyway would spawn \\
             without bound"
        );
        let root = tempfile::tempdir().expect("a temporary directory");
        let directory = root.path().join(ID);
        std::fs::create_dir_all(&directory).expect("a plugin directory");
        std::fs::write(directory.join("plugin.json"), manifest().as_bytes())
            .expect("writes the manifest");
        std::fs::copy(current_executable(), directory.join(binary_name()))
            .expect("puts the executable beside it");
        Self {
            data: root.path().join("data"),
            _root: root,
            directory,
            session: None,
            stderr: Vec::new(),
        }
    }

    /// Start the process, and wait for it to announce itself.
    ///
    /// A `Session` hands its messages over one at a time and never blocks, so this loop is
    /// the shape the worker's own evaluation has: take one thing, act on it, look again.
    /// Standing in for the worker is the point — a test that reached past the public API
    /// would not be testing the path the product runs.
    fn started(&mut self) -> &mut Self {
        let session = Session::start(
            PluginId::new(ID).expect("a valid id"),
            self.directory.clone(),
            self.data.clone(),
            "2.0.1".to_string(),
            "en-US".to_string(),
            7,
        )
        .expect("the executable this file put there is one the host can start");
        self.session = Some(session);
        let mut fonts = TextMeasurer::empty();
        let deadline = Instant::now() + PATIENCE;
        while Instant::now() < deadline {
            match self.step(&mut fonts) {
                Step::Said(id) => {
                    assert_eq!(id, ID, "the plugin announced the id it was started as");
                    return self;
                }
                Step::Exited(code) => panic!(
                    "the plugin's process exited with {code:?} before announcing itself\n{}",
                    self.said(20)
                ),
                Step::Acted | Step::Quiet => std::thread::sleep(Duration::from_millis(5)),
            }
        }
        panic!(
            "the plugin announced itself within {PATIENCE:?}\n{}",
            self.said(20)
        );
    }

    /// The live session, once started.
    fn session(&mut self) -> &mut Session {
        self.session.as_mut().expect("a started session")
    }

    /// Wait until the plugin's panel has been rasterized, and hand it back.
    fn await_panel(&mut self) -> bongocat_plugin_render::RenderedPanel {
        let mut fonts = TextMeasurer::empty();
        let deadline = Instant::now() + PATIENCE;
        while Instant::now() < deadline {
            if let Some(panel) = self.session().rendered() {
                return panel.clone();
            }
            self.step(&mut fonts);
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!(
            "the plugin drew a panel within {PATIENCE:?}\n{}",
            self.said(20)
        );
    }

    /// Act on the next thing the process said, if there is one.
    fn step(&mut self, fonts: &mut TextMeasurer) -> Step {
        match self.session().take_message() {
            Some(Incoming::Message(message)) => {
                // Noted before it is handed over, and then handed over: a `Ready` is the
                // message that records the descriptor, so a loop that returned early on it
                // would be a loop that saw the plugin arrive and then never learned its
                // name.
                let announced = match &message {
                    PluginMessage::Ready { descriptor } => Some(descriptor.id.as_str().to_owned()),
                    _ => None,
                };
                self.session().on_child_message(message, fonts, 1.0);
                match announced {
                    Some(id) => Step::Said(id),
                    None => Step::Acted,
                }
            }
            Some(Incoming::Exited(code)) => Step::Exited(code),
            // The child's own output, kept because it is the only explanation a test can
            // give for a process that did not do what it was supposed to: a panic inside
            // the plugin half shows up here and nowhere else. A refused line is the host
            // refusing something, which a test that is not about it has no opinion on.
            Some(Incoming::Stderr(line)) => {
                self.stderr.push(line);
                Step::Acted
            }
            Some(_) => Step::Acted,
            None => Step::Quiet,
        }
    }

    /// The last few lines the child wrote to its own stderr.
    ///
    /// In a failure message rather than printed, so a passing case says nothing about a
    /// process that worked and a failing one says what the process said.
    fn said(&self, tail: usize) -> String {
        let start = self.stderr.len().saturating_sub(tail);
        self.stderr[start..].join("\n")
    }

    /// Where inside the panel a press lands on the plugin's own button.
    ///
    /// Asked by scanning rather than by computing: the button's rectangle depends on the
    /// renderer's text metrics, and a test that hard-coded one would break whenever the
    /// product's own measurement moved. The scan is a step of four pixels over a 200×100
    /// panel — a few thousand hit tests, and no font files.
    fn press_the_button(&mut self) -> String {
        (0..PANEL_SIZE[1])
            .step_by(4)
            .flat_map(|y| {
                (0..PANEL_SIZE[0])
                    .step_by(4)
                    .map(move |x| (x as f32, y as f32))
            })
            .find_map(|(x, y)| self.session().press(x, y))
            .unwrap_or_else(|| panic!("somewhere in the panel is the button it declared"))
    }
}

/// What one turn of the host's loop produced.
enum Step {
    /// The plugin announced itself, with this id.
    Said(String),
    /// Something else was acted on.
    Acted,
    /// The process ended, with this exit code.
    Exited(Option<i32>),
    /// Nothing to do.
    Quiet,
}

/// Whether this binary was started as a plugin rather than run as the test suite.
///
/// The host sets [`SPAWNED_AS`] in every child it starts, and its **presence** is the
/// whole of the test. The value is the name of the directory the plugin was unpacked
/// into, which is the plugin's id only for a plugin this file put there itself: the
/// store's own layout names that directory after the version, so a case that installs
/// this binary the way the product does hands its child `1.0.0` rather than `probe`.
/// Comparing the value would therefore let that child run this suite — which starts a
/// plugin, whose child would do the same, without end.
fn spawned_as_plugin() -> bool {
    std::env::var_os(SPAWNED_AS).is_some()
}

/// The manifest a probe plugin ships: the smallest one the store accepts.
///
/// The executable is named after this binary's own file, which is the whole of what a
/// plugin's manifest has to get right about its own build — the name is a plain relative
/// path into the plugin's own directory, and the host runs exactly what it says.
fn manifest() -> String {
    format!(
        r#"{{"schema_version":1,"api_version":1,"id":"{ID}","name":"Probe","version":"1.0.0","executable":"{executable}"}}"#,
        executable = binary_name()
    )
}

/// A zip holding that manifest and nothing else.
///
/// Written here rather than through the packaging tool, because a test in this crate must
/// not depend on another crate's build: the point is to prove the store's unpack with
/// something the test itself made.
fn archive() -> Vec<u8> {
    let mut buffer = std::io::Cursor::new(Vec::new());
    {
        let mut writer = zip::ZipWriter::new(&mut buffer);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        writer.start_file("plugin.json", options).expect("a member");
        writer.write_all(manifest().as_bytes()).expect("writes");
        writer.finish().expect("a zip");
    }
    buffer.into_inner()
}

/// This binary's own file name, which the manifest has to name exactly.
fn binary_name() -> String {
    current_executable()
        .file_name()
        .and_then(|name| name.to_str())
        .map(str::to_owned)
        .expect("a file name")
}

fn current_executable() -> PathBuf {
    std::env::current_exe().expect("this binary's own path")
}

fn an_archive_becomes_an_installed_plugin() {
    // A manifest and nothing else: the store's member bound is four megabytes and a debug
    // test binary is far past it, so the executable is put in place by `Probe::on_disk`
    // and what this covers is the other half — that an archive unpacks under the id the
    // catalog named, and that `current` is what makes it live rather than a directory scan.
    let root = tempfile::tempdir().expect("a temporary directory");
    let store = PluginStore::new(root.path().join("store"));
    let id = PluginId::new(ID).expect("a valid id");
    let version = PluginVersion::new(1, 0, 0);
    let directory = store
        .unpack(&id, &version, &archive())
        .expect("the archive this file wrote unpacks");
    assert!(
        directory.join("plugin.json").is_file(),
        "and the manifest arrived beside where the executable goes"
    );
    assert!(
        store.installed().is_empty(),
        "an unpacked version is not a live one until `current` says so, so a download that was \
         cut off leaves nothing the product would start"
    );
    store
        .set_current(&id, &version)
        .expect("and becomes the live version");
    let installed = store.installed();
    assert_eq!(
        installed
            .iter()
            .map(|plugin| format!("{} {}", plugin.id, plugin.version))
            .collect::<Vec<_>>(),
        [format!("{ID} 1.0.0")],
        "the store lists what it unpacked, from `current` rather than by scanning"
    );
    assert_eq!(
        store
            .manifest(&installed[0])
            .expect("the manifest parses")
            .id
            .as_str(),
        ID,
        "and it is the manifest the store checked against the id it unpacked under"
    );
}

fn a_process_is_believed() {
    // The end of the road for a plugin: a real child process, over two real pipes,
    // answering the handshake. Everything after this is the host believing it.
    let mut probe = Probe::on_disk();
    probe.started();
    let facts = probe.session().facts();
    assert_eq!(facts.id.as_str(), ID);
    assert!(facts.running, "a process that said hello is running");
    assert_eq!(
        facts
            .descriptor
            .as_ref()
            .map(|descriptor| descriptor.name.resolve("en-US")),
        Some("Probe"),
        "and the card is named by the running process, not by the archive"
    );
    assert!(
        facts.wants(Subscription::HostState),
        "and the feeds it asked for are the feeds it is recorded as wanting"
    );
    assert!(facts.failure.is_none(), "{:?}", facts.failure);
}

fn a_panel_is_pressable() {
    let mut probe = Probe::on_disk();
    probe.started();
    let panel = probe.await_panel();
    assert!(
        !panel.pixels.is_empty(),
        "the host drew the plugin's scene rather than holding an empty texture"
    );
    assert_eq!(
        (panel.width, panel.height),
        (PANEL_SIZE[0], PANEL_SIZE[1]),
        "at the size the plugin's own placement asked for"
    );
    assert_eq!(
        panel.hit_regions.len(),
        1,
        "and the host's hit test found exactly the one pressable the scene declared"
    );
    assert_eq!(
        probe.press_the_button(),
        BUTTON,
        "a press inside it comes back as the id the plugin's own button declared"
    );
}

fn a_press_outside_is_not_a_press() {
    // The half of the hit test that keeps a click on the cat from being a click on a
    // plugin's button, and the reason the host tests against what it drew rather than
    // against the panel a plugin would draw next.
    let mut probe = Probe::on_disk();
    probe.started();
    probe.await_panel();
    assert_eq!(
        probe.session().press(-1_000.0, -1_000.0),
        None,
        "a press nowhere near the panel is no button at all"
    );
    assert_eq!(
        probe.session().press(f32::NAN, f32::NAN),
        None,
        "and a press with no coordinates in it is refused rather than matched"
    );
}

fn a_plugin_reports_its_settings() {
    let mut probe = Probe::on_disk();
    probe.started();
    let mut fonts = TextMeasurer::empty();
    let deadline = Instant::now() + PATIENCE;
    while Instant::now() < deadline {
        let config = probe.session().facts().config;
        if config.get("answered").is_some() {
            assert_eq!(
                config.get("answered"),
                Some(&ConfigValue::Bool(true)),
                "and the host read the value as itself rather than as a string"
            );
            return;
        }
        probe.step(&mut fonts);
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!(
        "the plugin's settings reached the host within {PATIENCE:?}\n{}",
        probe.said(20)
    );
}

fn an_ambiguous_panel_is_refused() {
    // Two buttons with one id make a press unanswerable, so the whole panel has to be
    // refused — and refused *before* the hit test, because a host that kept half of it
    // would be guessing which button the user meant.
    let mut probe = Probe::on_disk();
    probe.started();
    probe.await_panel();
    let mut fonts = TextMeasurer::empty();
    let mut duplicated = panel();
    let SceneNode::Stack(root) = &mut duplicated.scene else {
        panic!("the panel's root is a stack");
    };
    root.children.push(SceneNode::Button(ButtonNode {
        id: BUTTON.to_string(),
        label: "Also probe".to_string(),
        ..ButtonNode::default()
    }));
    assert_eq!(
        probe.session().on_child_message(
            PluginMessage::Panel(Box::new(duplicated)),
            &mut fonts,
            1.0
        ),
        SessionOutcome::Ignored,
        "so the incoming panel is turned away"
    );
    assert_eq!(
        probe.session().diagnostics().panels_rejected,
        1,
        "and counted, because a plugin that keeps sending one is a plugin the diagnostics export \\
         has to be able to explain"
    );
}

fn a_process_stops_on_its_own() {
    // A plugin left running after the host stops is a plugin that has to be killed, and
    // this is the step that makes sure it is not.
    let mut probe = Probe::on_disk();
    probe.started();
    probe.await_panel();
    probe.session().stop();
    let mut fonts = TextMeasurer::empty();
    let deadline = Instant::now() + PATIENCE;
    let mut ended = false;
    while Instant::now() < deadline {
        if matches!(probe.step(&mut fonts), Step::Exited(_)) {
            ended = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(
        ended,
        "the process ended by itself once the host closed its end of the pipe, so nothing had to \
         be killed\n{}",
        probe.said(20)
    );
    // The worker turns the exit into the session's own record; a test standing in for the
    // worker does the same one step, which is the whole of what it stands in for.
    probe.session().on_exit(Some(0));
    assert_eq!(probe.session().state(), SessionState::Exited);
    assert!(
        probe.session().panel().is_none(),
        "and its panel is off the model window rather than a stale texture nobody can explain"
    );
}

/// The switch on a plugin's card reaches the plugin center.
///
/// The published snapshot is the only thing the plugin page reads, and an entry's
/// `enabled` is read out of the worker's own session map when that snapshot is built.
/// So a stop that did not publish left the card's switch on for a plugin whose process
/// was already gone, and left the revision where it was — which is the one thing the
/// settings window's poll watches. The press then looked like it had done nothing at
/// all, and the page only corrected itself the next time an unrelated command happened
/// to publish.
///
/// A real [`bongocat_plugin::start`]ed worker rather than a stand-in, because the
/// publish is the worker's own and nothing above it can be asked about it: a double
/// running the loop itself would assert that the double publishes, which is the one
/// thing in question.
fn a_switch_turned_off_is_published() {
    let root = tempfile::tempdir().expect("a temporary directory");
    let store = PluginStore::new(root.path().join("store"));
    let id = PluginId::new(ID).expect("a valid id");
    let version = PluginVersion::new(1, 0, 0);
    // The manifest through the store's own path, and the program beside it afterwards:
    // the store refuses a member over four megabytes and this binary is far past it,
    // which is the same reason `Probe::on_disk` puts the executable in place by hand.
    let directory = store
        .unpack(&id, &version, &archive())
        .expect("the archive this file wrote unpacks");
    std::fs::copy(current_executable(), directory.join(binary_name()))
        .expect("puts the executable the manifest names beside it");
    store
        .set_current(&id, &version)
        .expect("and makes it the installed version");

    // A directory with no catalog in it is an empty catalog, so this worker never
    // reaches the network: what is under test is a switch, not a download.
    let (producer, _consumer) = bongocat_render::overlay_layer_channel();
    let (handle, endpoint) = bongocat_plugin::start(
        store,
        root.path().join("catalog"),
        CatalogMode::Directory,
        producer,
        Arc::new(LocalTimeCache::new()),
        None,
        root.path().join("data"),
        "0.0.0-test".to_string(),
        "en-US".to_string(),
        // No audio service behind this test, which is the same shape as a build whose
        // output device would not open: a plugin's sound request is answered rather than
        // dropped, and nothing here is about sound.
        bongocat_audio::MotionAudioClient::unavailable(),
    )
    .expect("a worker starts over a store with one plugin in it");
    let reader = handle.reader();

    // The handshake first, so the press below is about a plugin the center really is
    // showing as on rather than about one whose process is still starting.
    let on = await_published(&reader, &id, "the plugin announced itself", |entry| {
        entry.enabled && entry.running && entry.descriptor.is_some()
    });

    assert!(
        endpoint.send(PluginCommand::SetEnabled {
            id: id.clone(),
            enabled: false,
        }),
        "the switch's command is queued"
    );

    let off = await_published(&reader, &id, "the plugin was switched off", |entry| {
        !entry.enabled
    });
    assert!(
        off.revision > on.revision,
        "and the published revision moved with it: a snapshot that changed without moving \
         the revision is a page that never redraws, which is what the switch looked like"
    );
    assert!(
        !off.active.contains(&id),
        "and the panel is off the model window, which is what the switch says"
    );

    let mut stopper = handle.stopper(&endpoint);
    stopper.stop();
    handle
        .stop_and_join(Duration::from_secs(5))
        .expect("the worker stops rather than running on for the rest of the suite");
}

/// Wait for one plugin's published entry to satisfy a predicate, and hand back the
/// snapshot it was read from.
///
/// The snapshot rather than the entry, because the revision is half of what a case here
/// asserts — it is the only signal the settings window polls — and a deadline rather
/// than a sleep, because a worker that never publishes has to fail the case instead of
/// stalling the suite.
fn await_published(
    reader: &PluginWorkerReader,
    id: &PluginId,
    what: &str,
    until: impl Fn(&PluginEntry) -> bool,
) -> PluginSnapshot {
    let deadline = Instant::now() + PATIENCE;
    loop {
        // A snapshot read before the worker's first publish is an empty one, so a
        // plugin that is not on it yet is a question rather than a failure.
        let snapshot = reader.snapshot();
        if let Some(entry) = snapshot.entry(id)
            && until(entry)
        {
            return snapshot;
        }
        assert!(
            Instant::now() < deadline,
            "{what} within {PATIENCE:?}; the last published snapshot was revision {} listing \
             {:?}",
            snapshot.revision,
            snapshot
                .entries
                .iter()
                .map(|entry| entry.manifest.id.as_str())
                .collect::<Vec<_>>(),
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// The store has to make the file it is about to run runnable.
///
/// Every file an unpack writes is private — `0600`, no execute bit — and a zip does not
/// reliably carry a Unix mode, so an archive built on another platform arrives with
/// nothing to restore. Without the store marking it, the session would start, the
/// handshake would never happen, and the only symptom would be a card saying a plugin
/// could not be started.
fn an_unpacked_executable_is_made_runnable() {
    let root = tempfile::tempdir().expect("a temporary directory");
    let store = PluginStore::new(root.path().join("store"));
    let id = PluginId::new(ID).expect("a valid id");
    let version = PluginVersion::new(1, 0, 0);
    let directory = store
        .unpack(&id, &version, &archive_with_a_program())
        .expect("unpacks");
    let executable = directory.join(binary_name());
    assert!(executable.is_file(), "the executable arrived");
    assert_eq!(
        mode(&executable),
        Some(0o700),
        "so the store marked the file it is about to run, rather than leaving a private file \
         that only the store's own user could read and nobody could start"
    );
}

/// An archive holding a manifest and a member that is not a program, with no mode on it.
///
/// Not this binary: it is tens of megabytes and the store refuses a member over four. Not
/// a real program either, because this case is about the file's permissions and a program
/// that says nothing is a start failure the case above already covers.
fn archive_with_a_program() -> Vec<u8> {
    let mut buffer = std::io::Cursor::new(Vec::new());
    {
        let mut writer = zip::ZipWriter::new(&mut buffer);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        writer.start_file("plugin.json", options).expect("a member");
        writer.write_all(manifest().as_bytes()).expect("writes");
        writer.start_file(binary_name(), options).expect("a member");
        writer.write_all(b"not really a program").expect("writes");
        writer.finish().expect("a zip");
    }
    buffer.into_inner()
}

/// A file's mode, where a platform has modes.
#[cfg(unix)]
fn mode(path: &Path) -> Option<u32> {
    use std::os::unix::fs::PermissionsExt;
    // Masked to the permission bits: `mode()` is the whole `st_mode`, which carries the
    // file-type bits too, and this is about who may run it.
    std::fs::metadata(path)
        .ok()
        .map(|meta| meta.permissions().mode() & 0o7777)
}

/// A file's mode, where a platform has modes.
#[cfg(windows)]
fn mode(_path: &Path) -> Option<u32> {
    // Windows has no executable bit: a file is runnable or it is not, and the store's
    // marking is a no-op there, so there is no mode to assert.
    None
}

fn a_missing_executable_is_a_refusal() {
    // The same failure an author would hit, and a refusal rather than a hang because it
    // happens before any process is started.
    let root = tempfile::tempdir().expect("a temporary directory");
    let directory = root.path().join("pomodoro");
    std::fs::create_dir_all(&directory).expect("a plugin directory");
    std::fs::write(
        directory.join("plugin.json"),
        br#"{"schema_version":1,"api_version":1,"id":"pomodoro","name":"Pomodoro",
            "version":"1.0.0","executable":"pomodoro"}"#,
    )
    .expect("a manifest");
    let error = Session::start(
        PluginId::new("pomodoro").expect("valid"),
        directory,
        root.path().join("data"),
        "2.0.1".to_string(),
        "en-US".to_string(),
        1,
    )
    .err()
    .expect("there is nothing to run");
    assert_eq!(
        error.code(),
        bongocat_plugin_protocol::PluginErrorCode::PluginDirectoryUnreadable
    );
    assert!(
        error
            .detail
            .as_deref()
            .is_some_and(|detail| detail.contains("pomodoro")),
        "and the detail names the executable that is not there: {:?}",
        error.detail
    );
}

// ---------------------------------------------------------------------------
// The plugin half: the same binary, behaving as a plugin when the host spawns it.
// ---------------------------------------------------------------------------

/// Read the host's messages and answer them, until the host stops.
///
/// The smallest thing that satisfies the protocol: read the handshake, announce a
/// descriptor, draw one panel with one button, report the settings a declared field would
/// produce, and stop when told to. It is a plain function rather than a test because the
/// wire is stdout, and a harness that captured stdout would be a harness that swallowed
/// every message.
fn speak() {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    let mut reader = BufReader::new(stdin.lock());

    // The handshake: one line, and it has to be the hello, because the descriptor a plugin
    // announces is checked against the id and version the store installed.
    let mut line = String::new();
    if reader.read_line(&mut line).unwrap_or(0) == 0 {
        return;
    }
    let hello = match serde_json::from_str::<HostMessage>(line.trim()) {
        Ok(HostMessage::Hello(hello)) => hello,
        // Not a panic: a plugin whose first line was not a hello has no session, and the
        // host is the side that decides what to do about that. Returning is the same end
        // the host would arrive at, and it does not put a line on the wire.
        _ => return,
    };
    assert_eq!(hello.id.as_str(), ID, "the host said hello for this plugin");
    assert_eq!(hello.locale, "en-US", "and told it the user's language");
    assert_eq!(
        hello.protocol_version,
        bongocat_plugin_protocol::PROTOCOL_VERSION
    );
    assert_eq!(
        hello.data_dir,
        std::env::var("BONGOCAT_PLUGIN_DATA").unwrap_or_default(),
        "and the data directory it names is the one the environment names, because that is the \\
         only way a plugin finds its own files"
    );
    assert!(
        Path::new(&hello.data_dir).is_dir(),
        "and the host created it, because the host is the side that knows where storage is"
    );

    let descriptor = bongocat_plugin_protocol::PluginDescriptor {
        id: hello.id.clone(),
        name: "Probe".into(),
        version: hello.version,
        author: "BongoCat".to_string(),
        description: "A test double.".into(),
        icon: Default::default(),
        config: Default::default(),
        subscriptions: vec![Subscription::HostState],
    };
    send(
        &mut stdout,
        &PluginMessage::Ready {
            descriptor: Box::new(descriptor),
        },
    );
    send(&mut stdout, &PluginMessage::Panel(Box::new(panel())));
    send(
        &mut stdout,
        &PluginMessage::ConfigChanged {
            config: ConfigDocument::new(settings()),
        },
    );

    // Then serve, so a press and a stop both arrive and this process ends by itself rather
    // than being killed.
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) | Err(_) => return,
            Ok(_) => {}
        }
        match serde_json::from_str::<HostMessage>(line.trim()) {
            Ok(HostMessage::Shutdown) | Err(_) => return,
            Ok(_) => {}
        }
    }
}

/// The one panel this plugin draws: a single button, so there is a press target.
fn panel() -> PanelUpdate {
    PanelUpdate {
        placement: PanelPlacement {
            anchor: PluginAnchor::TopLeft,
            margin: [0.02, 0.02],
            width_fraction: 0.5,
            opacity: 0.9,
            size: PANEL_SIZE,
        },
        scene: SceneNode::Stack(StackNode {
            children: vec![SceneNode::Button(ButtonNode {
                id: BUTTON.to_string(),
                label: "Probe".to_string(),
                ..ButtonNode::default()
            })],
            ..StackNode::default()
        }),
    }
}

/// The settings this plugin reports, as a plugin that declared a field would.
fn settings() -> BTreeMap<String, ConfigValue> {
    BTreeMap::from([("answered".to_string(), ConfigValue::Bool(true))])
}

/// One message, one line, flushed — because the host reads a pipe.
fn send(output: &mut impl Write, message: &PluginMessage) {
    let line = bongocat_plugin_protocol::write_message(message).expect("a message serializes");
    let _ = output.write_all(&line);
    let _ = output.write_all(b"\n");
    let _ = output.flush();
}
