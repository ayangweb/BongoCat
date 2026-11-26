//! The global shortcuts' tests, split by the module they cover.
//!
//! The fixtures live here rather than in one of the files because two of them
//! need the same one: a registration is built to be registered *and* to be
//! mirrored away, and a test that does the second needs the first.

use super::*;

use bongocat_config::{
    ModelBehaviorAction, ModelBehaviorBinding, ModelIdentity, ModelSource, ShortcutBinding,
    ShortcutCommand, ShortcutConfig,
};

fn hotkey(chord: &str) -> HotKey {
    shortcut_hotkey(&ShortcutChord::parse(chord).expect("valid chord")).expect("mapped hotkey")
}

/// A model behavior binding of one model, with the expression name
/// shared by every model so that only the model identity distinguishes them.
fn behavior(chord: &str, model_id: &str) -> Registration {
    Registration {
        hotkey: hotkey(chord),
        target: ShortcutTarget::ModelBehavior {
            model: ModelIdentity {
                id: model_id.to_owned(),
                source: ModelSource::BuiltIn,
            },
            action: ModelBehaviorAction::Expression {
                name: "happy".to_owned(),
            },
        },
    }
}

fn command(chord: &str, command: ShortcutCommand) -> Registration {
    Registration {
        hotkey: hotkey(chord),
        target: ShortcutTarget::Application(command),
    }
}

/// Records the chords the mirror holds registered, so a test can assert
/// both the calls and the state the mirror leaves behind. It rejects a
/// re-registration of a chord it already holds, like the real manager.
#[derive(Default)]
struct FakeRegistrar {
    bound: std::cell::RefCell<BTreeSet<u32>>,
}

impl HotkeyRegistrar for FakeRegistrar {
    fn register(&self, hotkey: HotKey) -> Result<(), String> {
        assert!(
            self.bound.borrow_mut().insert(hotkey.id),
            "the OS refuses a chord it already holds"
        );
        Ok(())
    }

    fn unregister(&self, hotkey: HotKey) -> Result<(), String> {
        assert!(
            self.bound.borrow_mut().remove(&hotkey.id),
            "the OS cannot release a chord it does not hold"
        );
        Ok(())
    }
}

fn mirror(
    registrar: &FakeRegistrar,
    registered: &mut HashMap<u32, Registration>,
    desired: &[Registration],
) {
    mirror_registrations(
        registrar,
        desired,
        registered,
        &mut PressEdges::default(),
        &Mutex::new(Vec::new()),
        &GlobalShortcutCounters::default(),
    );
}

mod hotkey;
mod owner;
mod registration;
#[cfg(not(target_os = "linux"))]
mod service;
