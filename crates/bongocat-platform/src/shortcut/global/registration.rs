//! Turning the shared shortcut table into real OS registrations.
//!
//! The table is a value the configuration owns; the operating system is not
//! changed by rewriting it. So the plan is computed first — what this process
//! wants registered, what it already holds, and what the platform refused — and
//! only then applied. A binding the platform will not take is reported as a
//! registration failure and left out, rather than blocking the bindings behind
//! it: a model whose only unregistrable chord is ScrollLock should still have
//! its other twelve shortcuts work.

use super::*;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Registration {
    pub(crate) hotkey: HotKey,
    pub(crate) target: ShortcutTarget,
}

/// The OS registration surface the mirror drives. Production uses the real
/// manager; tests drive it with a fake so that reconciling a changed table
/// can be exercised without taking global hotkeys away from the machine
/// running the tests.
pub(crate) trait HotkeyRegistrar {
    fn register(&self, hotkey: HotKey) -> Result<(), String>;
    fn unregister(&self, hotkey: HotKey) -> Result<(), String>;
}

impl HotkeyRegistrar for GlobalHotKeyManager {
    fn register(&self, hotkey: HotKey) -> Result<(), String> {
        GlobalHotKeyManager::register(self, hotkey).map_err(|error| error.to_string())
    }

    fn unregister(&self, hotkey: HotKey) -> Result<(), String> {
        GlobalHotKeyManager::unregister(self, hotkey).map_err(|error| error.to_string())
    }
}

/// What a table change requires of the live OS registrations.
#[derive(Debug, Default)]
pub(crate) struct RegistrationPlan {
    /// Chords the table dropped: unregister them and forget them.
    pub(crate) removed: Vec<Registration>,
    /// Chords the table added: register them and remember them.
    pub(crate) added: Vec<Registration>,
    /// Chords the table still binds, behind a different target.
    pub(crate) retargeted: Vec<(u32, ShortcutTarget)>,
}

/// Compares the live registrations with the desired ones.
///
/// A registration's identity is its hotkey id — the chord — and not the
/// whole binding, because two models may legitimately bind the same chord:
/// each model counts its own default behavior chords from the primary
/// modifier's first digit, and only one model is live at a time. A model
/// switch therefore leaves most chords registered while the target behind
/// every one of them moves to the incoming model. A mirror that answered
/// "already registered" for such a chord would keep the platform pointing at
/// the outgoing model, and the dispatcher drops those targets as an inactive
/// model — the incoming model's behavior shortcuts would silently do nothing.
pub(crate) fn plan_registrations(
    registered: &HashMap<u32, Registration>,
    desired: &[Registration],
) -> RegistrationPlan {
    let desired_ids: BTreeSet<u32> = desired.iter().map(|entry| entry.hotkey.id).collect();
    let mut plan = RegistrationPlan::default();
    for (id, entry) in registered {
        if !desired_ids.contains(id) {
            plan.removed.push(entry.clone());
        }
    }
    for entry in desired {
        match registered.get(&entry.hotkey.id) {
            None => plan.added.push(entry.clone()),
            Some(registered) if registered.target != entry.target => {
                plan.retargeted
                    .push((entry.hotkey.id, entry.target.clone()));
            }
            Some(_) => {}
        }
    }
    plan
}

/// Applies the plan to the OS registrations, keeping `registered` as the
/// single record of what the platform currently holds.
pub(crate) fn mirror_registrations<M: HotkeyRegistrar>(
    manager: &M,
    desired: &[Registration],
    registered: &mut HashMap<u32, Registration>,
    pressed: &mut PressEdges,
    registration_failures: &Mutex<Vec<String>>,
    counters: &GlobalShortcutCounters,
) {
    let plan = plan_registrations(registered, desired);
    // Bindings that just left the table never report a release, so a
    // retained held id would swallow the first press after the same chord
    // is bound again.
    let bound: BTreeSet<u32> = desired.iter().map(|entry| entry.hotkey.id).collect();
    pressed.retain(&bound);

    for entry in plan.removed {
        if let Err(error) = manager.unregister(entry.hotkey) {
            counters
                .registration_failures
                .fetch_add(1, Ordering::Relaxed);
            record_failure(registration_failures, error);
        }
        // Forgetting a chord the platform no longer holds is what lets it
        // be registered again once it re-enters the table.
        registered.remove(&entry.hotkey.id);
    }

    // A chord that stayed registered is already held by the OS; only the
    // target behind it can change. Re-registering it would be refused (or
    // duplicate the registration) instead of retargeting it.
    for (id, target) in plan.retargeted {
        if let Some(registered) = registered.get_mut(&id) {
            registered.target = target;
        }
    }

    for entry in plan.added {
        match manager.register(entry.hotkey) {
            Ok(()) => {
                registered.insert(entry.hotkey.id, entry);
            }
            Err(error) => {
                counters
                    .registration_failures
                    .fetch_add(1, Ordering::Relaxed);
                record_failure(registration_failures, format!("{}: {error}", entry.hotkey));
            }
        }
    }
}
