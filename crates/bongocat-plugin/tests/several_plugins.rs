//! Several plugins, at once, on a real worker.
//!
//! `process_session.rs` proves one plugin can talk to this host. It cannot prove the
//! claim that matters most about a *plugin system*: that three of them run at the same
//! time without touching each other. That claim lives in the worker — one thread owning
//! several sessions, their feeds, their panels, their settings and their places — and the
//! worker had no test at all, so "several plugins run concurrently" was supported by
//! nothing but a number having been raised from four to nine.
//!
//! So this file starts the real worker over a real store and lets it manage real
//! processes, and checks the four things "independently and simultaneously" means:
//!
//! * every enabled plugin is running at the same time, each with its own descriptor,
//! * each plugin's settings are its own — its own keys, its own values, its own file,
//! * each panel is drawn in its own place, and a place another plugin holds is not even
//!   on the menu,
//! * one plugin going away takes nothing with it.
//!
//! # Why this file has its own `main`
//!
//! Same reason as `process_session.rs`: the plugin is **this binary** asked to behave as
//! one, and a plugin's stdout is the wire. `libtest` captures a running test's output, so
//! a child writing protocol lines from inside a `#[test]` would have had them swallowed
//! and the host would have seen nothing. `harness = false`, and the plugin half is a
//! plain function.
//!
//! Each case installs this binary under a different plugin id, and the child tells which
//! one it is from the directory it was unpacked into — the host sets
//! `BONGOCAT_PLUGIN_ID` to that, and hands a plugin the host's own environment
//! otherwise unmodified.

use bongocat_audio::MotionAudioClient;
use bongocat_plugin::{
    CatalogMode, LocalTimeCache, PluginCommand, PluginId, PluginSnapshot, PluginStore,
    PluginVersion, WorkerStopper,
};
use bongocat_plugin_protocol::{
    Color, ConfigDocument, ConfigValue, HostMessage, PanelPlacement, PanelUpdate, PluginAnchor,
    PluginMessage, SceneNode, StackNode, Subscription, TextNode,
};
use bongocat_render::overlay_layer_channel;
use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// The ids this binary answers to as a plugin, and what each one does.
///
/// The interesting part is that they *differ*: each prefers a different corner, declares a
/// differently-named setting, and draws a differently-sized panel. A worker that mixed
/// them up — one session's panel published as another's, one plugin's settings sent to
/// another, one plugin's place given to a second — would show up here as a size, a key or
/// a corner that is not this plugin's.
const ALPHA: &str = "tally-alpha";
const BETA: &str = "tally-beta";
const GAMMA: &str = "tally-gamma";
/// Announces itself, draws, and then exits — a plugin that dies mid-session.
const DELTA: &str = "tally-delta";

/// How long a case waits for the worker to publish something.
const PATIENCE: Duration = Duration::from_secs(30);

/// Each plugin's own shape, so a worker that confused two of them could not pass.
fn character_of(id: &str) -> (PluginAnchor, [u32; 2], &'static str, &'static str) {
    match id {
        ALPHA => (PluginAnchor::TopLeft, [200, 100], "Alpha", "alpha_flag"),
        BETA => (PluginAnchor::TopCenter, [240, 120], "Beta", "beta_flag"),
        GAMMA => (PluginAnchor::TopRight, [280, 140], "Gamma", "gamma_flag"),
        // The one that does not survive the session, so the case can watch the others
        // carry on without it.
        DELTA => (PluginAnchor::CenterLeft, [220, 110], "Delta", "delta_flag"),
        other => panic!("{other} is not a plugin this file knows how to be"),
    }
}

/// The cases, and the plain claim each one makes.
///
/// The same shape as `process_session.rs`'s: `harness = false`, a `main` that dispatches,
/// and a case that reports by panicking. A panic is the only report available here, because
/// the repository denies printing — and the rule is right for the same reason the plugin
/// half is a plain function: on the plugin side stdout *is* the wire.
const CASES: [(&str, fn()); 4] = [
    (
        "three plugins run at once, each with its own settings, panel and place",
        several_run_at_once,
    ),
    (
        "a plugin that dies takes nothing else with it",
        one_dying_takes_nothing,
    ),
    (
        "a place is offered to one plugin at a time, and switching one off frees it",
        places_are_exclusive,
    ),
    (
        "a plugin that cannot start says so on its own card",
        a_plugin_that_cannot_start_says_so,
    ),
];

fn main() -> std::process::ExitCode {
    if spawned_as_plugin() {
        return serve();
    }
    // Each case on its own, and a failure does not stop the next one: they are independent,
    // and a run that reported only the first failure would be a run that has to be repeated
    // after every fix.
    let mut failures: Vec<(String, String)> = Vec::new();
    for (name, case) in CASES {
        if let Err(payload) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(case)) {
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
fn describe(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(message) = payload.downcast_ref::<String>() {
        return message.clone();
    }
    if let Some(message) = payload.downcast_ref::<&str>() {
        return (*message).to_owned();
    }
    "a case panicked without a message".to_owned()
}

/// A worker over a store holding these plugins, and the reader that watches it.
///
/// The store is written directly rather than through `unpack`, for the reason written on
/// `process_session.rs`'s equivalent: a debug test binary is tens of megabytes and the
/// store refuses a member over four, which is a bound a release plugin is nowhere near.
/// The mode comes from `std::fs::copy`, which keeps it — so the executable is runnable
/// without the store's marking, and the store's own unpack path is exercised where it is
/// exercised.
struct Several {
    root: tempfile::TempDir,
    store: PluginStore,
    stopper: Option<WorkerStopper>,
    reader: Option<bongocat_plugin::PluginWorkerReader>,
    endpoint: bongocat_plugin::PluginWorkerEndpoint,
    layers: bongocat_render::OverlayLayerConsumer,
}

impl Several {
    /// A worker over a store holding one installed plugin per id, none of them started.
    fn with(ids: &[&str]) -> Self {
        let root = tempfile::tempdir().expect("a temporary directory");
        let store = PluginStore::new(root.path().join("store"));
        store.create().expect("a store");
        for id in ids {
            install(&store, id);
        }

        let (layers, layer_consumer) = overlay_layer_channel();
        let data = root.path().join("plugin-data");
        std::fs::create_dir_all(&data).expect("a data directory");
        let (handle, endpoint) = bongocat_plugin::start(
            store.clone(),
            root.path().join("catalog"),
            CatalogMode::Directory,
            layers,
            Arc::new(LocalTimeCache::new()),
            None,
            data,
            "2.0.1".to_string(),
            "en-US".to_string(),
            MotionAudioClient::unavailable(),
        )
        .expect("a worker");
        Self {
            root,
            store,
            stopper: Some(handle.stopper(&endpoint)),
            reader: Some(handle.reader()),
            endpoint,
            layers: layer_consumer,
        }
    }

    fn id(value: &str) -> PluginId {
        PluginId::new(value).expect("a valid id")
    }

    /// Turn each plugin on and wait for the worker to say every one of them is up.
    ///
    /// "Up" means running *and* announced — a session whose process exists has not yet
    /// said anything, and a snapshot taken in between would show a plugin that is running
    /// with no name, no settings and no panel. Waiting for less would make every assertion
    /// below a race against the handshake rather than a statement about the result.
    fn enable_all(&self, ids: &[&str]) -> PluginSnapshot {
        for id in ids {
            assert!(
                self.endpoint.send(PluginCommand::SetEnabled {
                    id: Self::id(id),
                    enabled: true,
                }),
                "the switch for {id} was queued"
            );
        }
        self.await_running(ids)
    }

    fn await_running(&self, ids: &[&str]) -> PluginSnapshot {
        let deadline = Instant::now() + PATIENCE;
        loop {
            let snapshot = self.snapshot();
            if ids.iter().all(|id| {
                snapshot.entry(&Self::id(id)).is_some_and(|entry| {
                    entry.running
                        && entry.descriptor.is_some()
                        && entry.failure.is_none()
                        // Its own settings, under its own key: the config arrives in a
                        // message of its own after the announcement, so "announced" and
                        // "reported what it has" are two moments apart.
                        && entry
                            .config
                            .get(character_of(id).3)
                            .is_some_and(|value| *value == ConfigValue::Bool(true))
                })
            }) {
                return snapshot;
            }
            assert!(
                Instant::now() < deadline,
                "every one of {ids:?} up and announced within {PATIENCE:?}; the last snapshot at \
                 revision {} was {:?}, phase {:?}, last error {:?}",
                snapshot.revision,
                state_of(&snapshot, ids),
                snapshot.phase,
                snapshot
                    .last_error
                    .as_ref()
                    .map(|error| (error.code().as_str(), error.detail.clone())),
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn snapshot(&self) -> PluginSnapshot {
        self.reader.as_ref().expect("a reader").snapshot()
    }

    fn send(&self, command: PluginCommand) {
        assert!(self.endpoint.send(command), "a command was queued");
    }

    /// The published layers, waited for until there are `count` of them.
    fn layers(&self, count: usize) -> Vec<bongocat_render::OverlayLayer> {
        let deadline = Instant::now() + PATIENCE;
        loop {
            let layers = self.layers.take_latest();
            if layers.len() >= count {
                return layers;
            }
            assert!(
                Instant::now() < deadline,
                "{count} panels on the model window within {PATIENCE:?}; the last publish \
                 carried {}",
                layers.len()
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

impl Drop for Several {
    fn drop(&mut self) {
        // An explicit shutdown rather than a drop, so the worker's thread is joined
        // before the temporary directory it reads from goes away. A worker still running
        // over a deleted store would fail a *later* case rather than this one.
        if let Some(mut stopper) = self.stopper.take() {
            let _ = stopper.stop();
        }
        let _ = &self.store;
        let _ = &self.root;
    }
}

/// What a snapshot was holding, for a failure message that has to be readable.
fn state_of(snapshot: &PluginSnapshot, ids: &[&str]) -> Vec<(String, bool, Option<String>)> {
    ids.iter()
        .map(|id| {
            let entry = snapshot.entry(&PluginId::new((*id).to_string()).expect("a valid id"));
            (
                id.to_string(),
                entry.is_some_and(|entry| entry.running),
                entry
                    .and_then(|entry| entry.failure.as_ref())
                    .map(|failure| {
                        format!(
                            "{} ({})",
                            failure.code().as_str(),
                            failure.detail.as_deref().unwrap_or("")
                        )
                    }),
            )
        })
        .collect()
}

/// Write one installed plugin into the store: its directory, its executable, its manifest.
fn install(store: &PluginStore, id: &str) {
    let id_value = PluginId::new(id.to_string()).expect("a valid id");
    let version = PluginVersion::new(1, 0, 0);
    // The store's own layout — `root/<id>/<version>/` — written directly rather than asked
    // for. `version_directory` only answers about a directory that already holds a
    // manifest, so it cannot say where one should go; asking it that way would mean a
    // second copy of the layout here, and the layout is the store's fact.
    let directory = store.root().join(id).join(version.to_string());
    std::fs::create_dir_all(&directory).expect("a plugin directory");
    let (_, _, label, _) = character_of(id);
    std::fs::write(
        directory.join(bongocat_plugin_protocol::PLUGIN_MANIFEST_FILE_NAME),
        manifest_for(id, label).as_bytes(),
    )
    .expect("writes the manifest");
    std::fs::copy(current_executable(), directory.join(binary_name()))
        .expect("puts the executable beside it");
    store
        .set_current(&id_value, &version)
        .expect("marks the version live");
}

fn manifest_for(id: &str, name: &str) -> String {
    format!(
        r#"{{"schema_version":1,"api_version":1,"id":"{id}","name":"{name}","version":"1.0.0","executable":"{executable}","icon":{{"emoji":"🧮"}}}}"#,
        executable = binary_name()
    )
}

fn binary_name() -> String {
    current_executable()
        .file_name()
        .expect("a file name")
        .to_string_lossy()
        .into_owned()
}

fn current_executable() -> PathBuf {
    std::env::current_exe().expect("this test binary")
}

// ---------------------------------------------------------------------------
// The cases.
// ---------------------------------------------------------------------------

fn several_run_at_once() {
    let ids = [ALPHA, BETA, GAMMA];
    let worker = Several::with(&ids);
    let snapshot = worker.enable_all(&ids);

    // Every one is running, at the same time, on one worker thread.
    for id in ids {
        let entry = snapshot.entry(&Several::id(id)).expect("an entry");
        assert!(
            entry.running && entry.enabled,
            "{id} is running and enabled: {:?}",
            entry.failure
        );
        assert_eq!(
            entry
                .descriptor
                .as_ref()
                .map(|descriptor| descriptor.id.as_str()),
            Some(id),
            "and it is running under its own id, so one plugin cannot answer for another"
        );
    }

    // Each plugin's settings are its own — its own keys, and its own values back.
    for id in ids {
        let entry = snapshot.entry(&Several::id(id)).expect("an entry");
        let (_, _, _, key) = character_of(id);
        let descriptor = entry.descriptor.as_ref().expect("a descriptor");
        let keys: Vec<&str> = descriptor
            .config
            .fields
            .iter()
            .map(|field| field.key.as_str())
            .collect();
        assert_eq!(
            keys,
            vec![key],
            "{id} declared one setting, and the host is showing {id}'s own: a shared form \
             would have put another plugin's key in this plugin's settings"
        );
        assert_eq!(
            entry.config.get(key),
            Some(&ConfigValue::Bool(true)),
            "and the value the plugin reported for its own key is what the host holds, not \
             a default the host invented"
        );
    }

    // Each panel has its own place, and nobody is offered a place another one holds.
    let places: BTreeSet<PluginAnchor> = ids
        .iter()
        .filter_map(|id| {
            snapshot
                .entry(&Several::id(id))
                .and_then(|entry| entry.position)
                .map(|placed| placed.anchor)
        })
        .collect();
    assert_eq!(
        places.len(),
        ids.len(),
        "three plugins, three places: {:?}",
        describe_places(&snapshot, &ids)
    );
    for id in ids {
        let entry = snapshot.entry(&Several::id(id)).expect("an entry");
        let mine = entry.position.as_ref().expect("a place").anchor;
        assert_eq!(
            mine,
            character_of(id).0,
            "{id} keeps its own preferred corner while it is free, which is the answer for \
             a fresh install and the reason a plugin's author still has a say"
        );
        for other in ids.iter().filter(|other| **other != id) {
            let theirs = snapshot
                .entry(&Several::id(other))
                .and_then(|entry| entry.position)
                .expect("a place");
            assert!(
                !entry.positions.contains(&theirs.anchor),
                "{id} is offered {theirs:?}, which {other} is already in: a position another \
                 plugin holds must be absent from the menu, not marked unavailable"
            );
        }
    }

    // And three panels are on the model window, each its own size, so no two of them can be
    // the same layer published twice.
    let layers = worker.layers(ids.len());
    let sizes: BTreeSet<(u32, u32)> = layers
        .iter()
        .map(|layer| (layer.raster.width, layer.raster.height))
        .collect();
    for id in ids {
        assert!(
            sizes.contains(&(character_of(id).1[0], character_of(id).1[1])),
            "{id}'s panel is on the window at its own size: {sizes:?}"
        );
    }
    assert_eq!(
        sizes.len(),
        ids.len(),
        "three panels, three sizes: one size would mean two plugins are being drawn from \
         one session, which is the failure this whole file exists to catch"
    );
}

fn one_dying_takes_nothing() {
    let ids = [ALPHA, BETA, DELTA];
    let worker = Several::with(&ids);
    for id in ids {
        worker.send(PluginCommand::SetEnabled {
            id: Several::id(id),
            enabled: true,
        });
    }
    // The two that are meant to survive, waited for. Delta is not waited for and cannot be:
    // it exists here to exit, so "up and reported its settings" is a moment it is never in.
    worker.await_running(&[ALPHA, BETA]);

    // Delta exits by itself the moment it is started. The worker has to notice on its own;
    // nothing is told to it.
    let deadline = Instant::now() + PATIENCE;
    let snapshot = loop {
        let snapshot = worker.snapshot();
        let delta = snapshot.entry(&Several::id(DELTA)).expect("an entry");
        // Not "running" is the observable: either it is stopped or it has failed, and both
        // are the host's business rather than this case's.
        if !delta.running {
            break snapshot;
        }
        // The message reports the snapshot rather than the layer channel: `take_latest`
        // drains, so asking it for a count here would both destroy what a later assertion
        // needs and wait for something that may never arrive.
        assert!(
            Instant::now() < deadline,
            "delta to stop within {PATIENCE:?}; it is still running and the snapshot lists \
             {:?} as active",
            snapshot.active,
        );
        std::thread::sleep(Duration::from_millis(10));
    };

    // The other two are untouched: still running, still in their own places.
    for id in [ALPHA, BETA] {
        let entry = snapshot.entry(&Several::id(id)).expect("an entry");
        assert!(
            entry.running,
            "{id} is still running after delta exited: {:?}",
            entry.failure
        );
        assert!(
            entry.failure.is_none(),
            "and has not been given delta's failure, because a plugin that dies is its own \
             card's news and not everybody else's"
        );
        assert_eq!(
            entry.position.as_ref().map(|placed| placed.anchor),
            Some(character_of(id).0),
            "{id} is still in its own place: a plugin's death must not reshuffle the window"
        );
    }

    // And they keep drawing, which is the part "a crash does not take the application
    // down" actually means for a panel: the two survivors are on the model window at their
    // own sizes, and delta's is not among them.
    let layers = worker.layers(2);
    for id in [ALPHA, BETA] {
        assert!(
            layers.iter().any(|layer| {
                (layer.raster.width, layer.raster.height)
                    == (character_of(id).1[0], character_of(id).1[1])
            }),
            "{id} is still being published after delta's process ended: {:?}",
            layers
                .iter()
                .map(|layer| (layer.raster.width, layer.raster.height))
                .collect::<Vec<_>>(),
        );
    }
}

fn places_are_exclusive() {
    let ids = [ALPHA, BETA];
    let worker = Several::with(&ids);
    let snapshot = worker.enable_all(&ids);
    let alpha_anchor = snapshot
        .entry(&Several::id(ALPHA))
        .and_then(|entry| entry.position)
        .expect("a place")
        .anchor;
    let beta_anchor = snapshot
        .entry(&Several::id(BETA))
        .and_then(|entry| entry.position)
        .expect("a place")
        .anchor;

    // Beta may be moved to any free place.
    let free = snapshot
        .entry(&Several::id(BETA))
        .expect("an entry")
        .positions
        .iter()
        .find(|anchor| **anchor != beta_anchor)
        .copied()
        .expect("eight other places to move to");
    worker.send(PluginCommand::SetPosition {
        id: Several::id(BETA),
        anchor: free,
    });
    let snapshot = await_snapshot(
        &worker,
        |snapshot| {
            snapshot
                .entry(&Several::id(BETA))
                .and_then(|entry| entry.position)
                .is_some_and(|placed| placed.anchor == free)
        },
        "beta at the place the user moved it to",
    );
    assert_eq!(
        snapshot
            .entry(&Several::id(BETA))
            .and_then(|entry| entry.position)
            .map(|placed| placed.chosen),
        Some(true),
        "and it is recorded as the user's choice rather than as a default, so a plugin that \\
         arrives later cannot take it"
    );
    assert_eq!(
        snapshot
            .entry(&Several::id(ALPHA))
            .and_then(|entry| entry.position)
            .map(|placed| placed.anchor),
        Some(alpha_anchor),
        "while alpha keeps its own: moving one plugin must not move another"
    );

    // Switching alpha off frees its place, and the place comes back rather than the file
    // being rewritten.
    worker.send(PluginCommand::SetEnabled {
        id: Several::id(ALPHA),
        enabled: false,
    });
    let snapshot = await_snapshot(
        &worker,
        |snapshot| {
            snapshot
                .entry(&Several::id(ALPHA))
                .is_some_and(|entry| !entry.running)
                && snapshot
                    .entry(&Several::id(BETA))
                    .is_some_and(|entry| entry.positions.contains(&alpha_anchor))
        },
        "alpha off and its place offered to beta",
    );
    assert!(
        snapshot
            .entry(&Several::id(ALPHA))
            .and_then(|entry| entry.position)
            .is_none(),
        "a plugin that is drawing nothing has no place, so its corner is free without \
         anything being edited"
    );
    assert!(
        snapshot
            .entry(&Several::id(BETA))
            .is_some_and(|entry| entry.running),
        "and the plugin that was still on is still on, which is the other half of a switch \
         that only switched off one thing"
    );
    let _ = beta_anchor;
}

/// A plugin that cannot start has no session, and the card still has to say why.
///
/// The case is here because it was not true, and the way it was not true was invisible:
/// a start that fails is published as one page-level error, so a machine with two broken
/// plugins showed one message naming neither, and each card claimed nothing was wrong. A
/// session is where a card's reason comes from, and a plugin that never got one had nothing.
///
/// So the reason is recorded against the plugin and shown on its card, and it is dropped
/// again when the plugin is switched off — a start failure is a fact about being asked to
/// start, not a permanent property of the plugin.
fn a_plugin_that_cannot_start_says_so() {
    let ids = [ALPHA, BETA];
    let worker = Several::with(&ids);

    // Beta's executable is taken away after it was installed, which is the case a user hits
    // when an antivirus quarantines a plugin or a directory is half-copied.
    //
    // A *manifest* that will not parse would not be this case: the host does not list a
    // record whose manifest it cannot read, so such a plugin has no card at all rather than
    // a card with no reason. This one parses, so the plugin is listed and can be asked to
    // start — and that is the moment the reason has somewhere to go.
    let beta = worker.store.root().join(BETA).join("1.0.0");
    std::fs::remove_file(beta.join(binary_name())).expect("takes the executable away");

    worker.send(PluginCommand::SetEnabled {
        id: Several::id(ALPHA),
        enabled: true,
    });
    worker.send(PluginCommand::SetEnabled {
        id: Several::id(BETA),
        enabled: true,
    });
    // Alpha is waited for as usual; beta is waited for by the thing this case is about.
    let _ = worker.await_running(&[ALPHA]);
    let snapshot = await_snapshot(
        &worker,
        |snapshot| {
            snapshot
                .entry(&Several::id(BETA))
                .is_some_and(|entry| entry.failure.is_some())
        },
        "beta's card to carry the reason",
    );

    let beta_entry = snapshot.entry(&Several::id(BETA)).expect("an entry");
    let failure = beta_entry
        .failure
        .as_ref()
        .expect("a reason on beta's own card");
    assert_eq!(
        failure.code(),
        bongocat_plugin_protocol::PluginErrorCode::PluginDirectoryUnreadable,
        "and it names what is wrong rather than saying the plugin failed: {:?}",
        failure.detail
    );
    assert!(
        !beta_entry.running,
        "while a plugin that could not start is plainly not running"
    );

    // The other plugin is untouched: one broken manifest is one card's news.
    let alpha = snapshot.entry(&Several::id(ALPHA)).expect("an entry");
    assert!(
        alpha.running && alpha.failure.is_none(),
        "and alpha neither refused nor blamed: {:?}",
        alpha.failure
    );

    // Switching the broken plugin off drops the reason, because there is no longer
    // anything to report about a plugin nobody asked to run.
    worker.send(PluginCommand::SetEnabled {
        id: Several::id(BETA),
        enabled: false,
    });
    let snapshot = await_snapshot(
        &worker,
        |snapshot| {
            snapshot
                .entry(&Several::id(BETA))
                .is_some_and(|entry| entry.failure.is_none())
        },
        "beta's card clean once it is switched off",
    );
    let beta = snapshot.entry(&Several::id(BETA)).expect("an entry");
    assert!(
        !beta.running && !beta.enabled,
        "and it is plainly off: a plugin that was never running cannot be enabled by \
         refusing to run"
    );
}

fn describe_places(snapshot: &PluginSnapshot, ids: &[&str]) -> Vec<(String, Option<PluginAnchor>)> {
    ids.iter()
        .map(|id| {
            (
                id.to_string(),
                snapshot
                    .entry(&Several::id(id))
                    .and_then(|entry| entry.position)
                    .map(|placed| placed.anchor),
            )
        })
        .collect()
}

/// Wait for the worker to publish something matching `until`.
fn await_snapshot(
    worker: &Several,
    until: impl Fn(&PluginSnapshot) -> bool,
    what: &str,
) -> PluginSnapshot {
    let deadline = Instant::now() + PATIENCE;
    loop {
        let snapshot = worker.snapshot();
        if until(&snapshot) {
            return snapshot;
        }
        assert!(
            Instant::now() < deadline,
            "{what} within {PATIENCE:?}; the last snapshot at revision {} was {:?}",
            snapshot.revision,
            snapshot
                .entries
                .iter()
                .map(|entry| (
                    entry.manifest.id.as_str().to_string(),
                    entry.running,
                    entry.position.map(|placed| placed.anchor),
                ))
                .collect::<Vec<_>>(),
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn spawned_as_plugin() -> bool {
    std::env::var_os("BONGOCAT_PLUGIN_ID").is_some()
}

// ---------------------------------------------------------------------------
// The plugin half: the same binary, behaving as whichever plugin it was started as.
// ---------------------------------------------------------------------------

/// Announce, draw, and serve until told to stop — or, for the one plugin that is here to
/// die, exit as soon as it has drawn.
fn serve() -> std::process::ExitCode {
    let id = std::env::var("BONGOCAT_PLUGIN_ID").unwrap_or_default();
    let (anchor, size, label, key) = character_of(&id);

    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    let mut reader = BufReader::new(stdin.lock());

    let mut line = String::new();
    if reader.read_line(&mut line).unwrap_or(0) == 0 {
        return std::process::ExitCode::SUCCESS;
    }
    let hello = match serde_json::from_str::<HostMessage>(line.trim()) {
        Ok(HostMessage::Hello(hello)) => hello,
        _ => return std::process::ExitCode::SUCCESS,
    };

    send(
        &mut stdout,
        &PluginMessage::Ready {
            descriptor: Box::new(bongocat_plugin_protocol::PluginDescriptor {
                id: hello.id.clone(),
                name: label.into(),
                version: hello.version,
                author: "BongoCat".to_string(),
                description: format!("The {label} plugin.").into(),
                icon: Default::default(),
                config: bongocat_plugin_protocol::ConfigSchema {
                    schema_version: 1,
                    fields: vec![bongocat_plugin_protocol::ConfigField {
                        key: key.to_string(),
                        label: format!("{label} flag").into(),
                        description: None,
                        control: bongocat_plugin_protocol::ConfigControl::Toggle { default: true },
                    }],
                },
                draws_panel: true,
                subscriptions: vec![Subscription::HostState],
            }),
        },
    );
    send(
        &mut stdout,
        &PluginMessage::Panel(Box::new(panel(anchor, size, label))),
    );
    send(
        &mut stdout,
        &PluginMessage::ConfigChanged {
            config: ConfigDocument::new(BTreeMap::from([(
                key.to_string(),
                ConfigValue::Bool(true),
            )])),
        },
    );

    if id == DELTA {
        // Draw, then die. The panels are flushed above, so what the host has already
        // received is a plugin that worked; what follows is a process that is gone, which
        // is the thing the other two must survive.
        return std::process::ExitCode::FAILURE;
    }

    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) | Err(_) => return std::process::ExitCode::SUCCESS,
            Ok(_) => {}
        }
        match serde_json::from_str::<HostMessage>(line.trim()) {
            Ok(HostMessage::Shutdown) | Err(_) => return std::process::ExitCode::SUCCESS,
            Ok(_) => {}
        }
    }
}

/// This plugin's own panel: a filled surface at this plugin's own size.
///
/// A surface rather than a line of text, because the case checks the *sizes*: the worker's
/// text measurer loads system fonts, but a shape's size is its own regardless of what the
/// machine has, so a difference between two plugins' panels is a difference the worker made
/// rather than a difference in the fonts.
fn panel(anchor: PluginAnchor, size: [u32; 2], label: &str) -> PanelUpdate {
    PanelUpdate {
        placement: PanelPlacement {
            anchor,
            margin: [0.02, 0.02],
            width_fraction: 0.5,
            opacity: 0.9,
            size,
        },
        scene: SceneNode::Stack(StackNode {
            padding: [8.0, 8.0],
            background: Some(Color::rgba(0x20, 0x20, 0x20, 0xff)),
            radius: 8.0,
            children: vec![SceneNode::Text(TextNode {
                value: label.to_string(),
                ..TextNode::default()
            })],
            ..StackNode::default()
        }),
    }
}

/// One message, one line, flushed — because the host reads a pipe.
fn send(output: &mut impl Write, message: &PluginMessage) {
    let line = bongocat_plugin_protocol::write_message(message).expect("a message serializes");
    let _ = output.write_all(&line);
    let _ = output.write_all(b"\n");
    let _ = output.flush();
}
