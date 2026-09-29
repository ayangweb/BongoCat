//! The plugin worker's snapshot, projected onto the settings protocol.
//!
//! The window's vocabulary is deliberately thinner than the host's: a row, a state,
//! and a code. Everything the host knows and the window has no use for — a digest, a
//! directory, a signature, a `PluginId` that validates itself — stops here, which is
//! what makes the plugin centre unable to grow a dependency on the plugin protocol.

use super::*;

use bongocat_plugin::{PluginEntry, PluginPhase, PluginSnapshot};
use bongocat_plugin::{PluginErrorCode, PluginVersion};

/// Project one worker snapshot.
pub(super) fn project_plugins(snapshot: &PluginSnapshot) -> SettingsPlugins {
    SettingsPlugins {
        revision: snapshot.revision,
        available: true,
        busy: snapshot.phase.as_ref().is_some_and(PluginPhase::is_busy),
        entries: snapshot.entries.iter().map(project_entry).collect(),
        active: snapshot.active.len(),
        maximum_active: bongocat_plugin::MAXIMUM_ENABLED_PLUGINS,
        last_error: snapshot.last_error.as_ref().map(project_error),
    }
}

fn project_entry(entry: &PluginEntry) -> SettingsPluginEntry {
    SettingsPluginEntry {
        id: entry.manifest.id.as_str().to_owned(),
        name: entry.manifest.name.clone(),
        description: entry.manifest.description.clone(),
        author: entry.manifest.author.clone(),
        installed_version: entry
            .installed
            .then(|| version_text(&entry.manifest.version)),
        available_version: entry.available_version.as_ref().map(version_text),
        installed: entry.installed,
        enabled: entry.enabled,
        update_available: entry.update_available,
        refusal: entry.refusal.as_ref().map(|error| SettingsPluginRefusal {
            code: project_error_code(error.code),
            detail: error.detail.clone(),
        }),
    }
}

fn version_text(version: &PluginVersion) -> String {
    version.to_string()
}

fn project_error(error: &bongocat_plugin::PluginError) -> SettingsPluginError {
    SettingsPluginError {
        code: project_error_code(error.code),
        detail: error.detail.clone(),
    }
}

/// Map one host failure onto the code the window can say a sentence about.
///
/// The arms are written out rather than grouped by guesswork, and the match is
/// deliberately exhaustive: the host's code set is a closed list this crate is
/// rebuilt against, so a code the window has no sentence for is a compile error here
/// rather than a blank notification at runtime. The two codes that are not the host's
/// own — `HostUnavailable` and `HostStopped` — are produced above this function.
fn project_error_code(code: PluginErrorCode) -> SettingsPluginErrorCode {
    use PluginErrorCode as Code;
    match code {
        Code::PluginNotPublished => SettingsPluginErrorCode::NotPublished,
        Code::AlreadyInstalled | Code::AlreadyUpToDate => SettingsPluginErrorCode::AlreadyInstalled,
        Code::NotInstalled => SettingsPluginErrorCode::NotPublished,
        Code::CatalogInvalid => SettingsPluginErrorCode::CatalogUnavailable,
        Code::DownloadFailed => SettingsPluginErrorCode::NetworkUnavailable,
        Code::ChecksumMismatch => SettingsPluginErrorCode::ChecksumMismatch,
        Code::SignatureInvalid | Code::SignatureKeyMissing => {
            SettingsPluginErrorCode::SignatureInvalid
        }
        Code::ArchiveInvalid => SettingsPluginErrorCode::InvalidManifest,
        Code::StoreWriteFailed => SettingsPluginErrorCode::StoreWriteFailed,
        Code::PluginDirectoryUnreadable => SettingsPluginErrorCode::StoreWriteFailed,
        Code::TooManyEnabled => SettingsPluginErrorCode::TooManyEnabled,
        Code::RenderFailed | Code::FontUnavailable => SettingsPluginErrorCode::RenderFailed,
        Code::ManifestInvalid
        | Code::UnsupportedSchemaVersion
        | Code::UnsupportedApiVersion
        | Code::InvalidPluginId
        | Code::InvalidPluginName
        | Code::InvalidPluginDescription
        | Code::InvalidAssetPath
        | Code::CapabilityNotGranted
        | Code::TooManyBehaviors
        | Code::InvalidBehaviorId
        | Code::DuplicateBehaviorId
        | Code::InvalidBehaviorSpec
        | Code::SceneTooLarge
        | Code::SceneTooDeep
        | Code::InvalidBinding
        | Code::UnknownBinding
        | Code::InvalidButtonId
        | Code::DuplicateButtonId
        | Code::InvalidTimeFormat
        | Code::InvalidPanelSize
        | Code::InvalidPanelPlacement => SettingsPluginErrorCode::InvalidManifest,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bongocat_plugin::{PluginManifest, PluginVersion};

    fn manifest(id: &str) -> PluginManifest {
        PluginManifest {
            schema_version: 1,
            api_version: 1,
            id: bongocat_plugin::PluginId::new(id).expect("a valid id"),
            name: format!("{id} name"),
            version: PluginVersion::new(1, 0, 0),
            author: "someone".to_string(),
            description: format!("{id} description"),
            min_app_version: None,
            capabilities: Vec::new(),
            icon: None,
            overlay: bongocat_plugin::OverlayContribution {
                anchor: bongocat_plugin::PluginAnchor::TopLeft,
                margin: [0.0, 0.0],
                width_fraction: 0.5,
                opacity: 1.0,
                size: [1, 1],
                behaviors: Vec::new(),
                scene: bongocat_plugin::SceneNode::Spacer(bongocat_plugin::SpacerNode {
                    grow: 1.0,
                }),
            },
        }
    }

    #[test]
    fn an_installed_plugin_shows_its_version_and_can_be_updated() {
        let snapshot = PluginSnapshot {
            revision: 4,
            phase: Some(PluginPhase::Idle),
            entries: vec![PluginEntry {
                manifest: manifest("pomodoro"),
                installed: true,
                enabled: true,
                available_version: Some(PluginVersion::new(2, 0, 0)),
                update_available: true,
                refusal: None,
            }],
            active: vec![bongocat_plugin::PluginId::new("pomodoro").expect("id")],
            last_error: None,
        };

        let plugins = project_plugins(&snapshot);

        assert_eq!(plugins.revision, 4);
        assert!(plugins.available);
        assert!(!plugins.busy);
        assert_eq!(plugins.active, 1);
        assert_eq!(plugins.entries.len(), 1);
        let entry = &plugins.entries[0];
        assert_eq!(entry.id, "pomodoro");
        assert_eq!(entry.installed_version.as_deref(), Some("1.0.0"));
        assert_eq!(entry.available_version.as_deref(), Some("2.0.0"));
        assert!(entry.update_available);
        assert!(entry.enabled);
    }

    #[test]
    fn a_catalog_only_plugin_has_no_installed_version_and_keeps_its_refusal() {
        // A plugin the catalog offers but not for this platform is refused with
        // `PluginNotPublished` and a detail naming the platform, so the row survives
        // and explains itself rather than disappearing from the list.
        let refusal = bongocat_plugin::PluginError::with_detail(
            PluginErrorCode::PluginNotPublished,
            "no download for platform x86_64-unknown-linux-gnu",
        );
        let snapshot = PluginSnapshot {
            revision: 1,
            phase: None,
            entries: vec![PluginEntry {
                manifest: manifest("linux-only"),
                installed: false,
                enabled: false,
                available_version: None,
                update_available: false,
                refusal: Some(refusal),
            }],
            active: Vec::new(),
            last_error: None,
        };

        let entry = &project_plugins(&snapshot).entries[0];

        assert!(!entry.installed);
        assert_eq!(entry.installed_version, None);
        assert_eq!(
            entry.refusal.as_ref().map(|refusal| refusal.code),
            Some(SettingsPluginErrorCode::NotPublished),
            "a plugin this host cannot install still has to be listed, with a reason"
        );
    }

    #[test]
    fn a_busy_phase_is_reported_and_an_idle_one_is_not() {
        let mut snapshot = PluginSnapshot {
            revision: 0,
            phase: Some(PluginPhase::Idle),
            ..PluginSnapshot::default()
        };
        assert!(!project_plugins(&snapshot).busy);

        snapshot.phase = Some(PluginPhase::RefreshingCatalog);
        assert!(project_plugins(&snapshot).busy);

        snapshot.phase = Some(PluginPhase::Installing(
            bongocat_plugin::PluginId::new("pomodoro").expect("id"),
        ));
        assert!(project_plugins(&snapshot).busy);
    }

    #[test]
    fn every_host_code_maps_to_something_the_window_can_say() {
        // The point is totality: `project_error_code` matches the host's whole code
        // set with no catch-all, so a code added to the host without a sentence here
        // fails to compile rather than producing an unreachable page.
        for code in PluginErrorCode::ALL {
            let mapped = project_error_code(code);
            assert_ne!(
                mapped,
                SettingsPluginErrorCode::Other,
                "{code:?} has a code of its own and should not fall through"
            );
        }
    }
}
