//! The standalone automated-verification modes of the product binary.
//!
//! Each function here is a whole process: the entry point recognizes its flag
//! and returns before the product assembles a single window, because what it
//! checks is a subsystem on its own: the login-startup registration, the settings
//! window's persisted bounds, the diagnostics export, or what a crash leaves
//! behind. Every one of them either verifies an assertion and exits or reports
//! what it observed and fails; none of them starts the product.
//!
//! The scenarios that need the running product (the settings window pages, the
//! system menu, the overlay frame loop) stay in `main.rs`, because they drive the
//! coordinator and the run loop this binary exists to host.

use crate::write_smoke_status;
#[cfg(feature = "storage-test-injection")]
use crate::{gpui_application, preset_root};
#[cfg(feature = "storage-test-injection")]
use async_io::Timer;
#[cfg(feature = "storage-test-injection")]
use bongocat_log::{LogLevel, LogStream, is_log_file_name, parse_log_line};
#[cfg(feature = "storage-test-injection")]
use bongocat_ui::{SettingsNavigationMemory, SettingsWindowSeed, open_settings_window};
#[cfg(feature = "storage-test-injection")]
use gpui_kit::assets::AllAssets;
#[cfg(feature = "storage-test-injection")]
use gpui_kit::{App, px, size};
#[cfg(feature = "storage-test-injection")]
use std::{
    env,
    io::{self, Write},
    path::{Path, PathBuf},
    time::Duration,
};
#[cfg(feature = "storage-test-injection")]
use zip::ZipArchive;

/// Reports the startup permission state the product would act on, without showing any prompt.
///
/// This is the repeatable acceptance path for both platforms: it is run once while the capability
/// is missing and once while it is granted, and it never writes product state.
pub(crate) fn run_startup_permission_smoke() -> Result<(), Box<dyn std::error::Error>> {
    let state = if bongocat_platform::startup_permission_available() {
        "available"
    } else {
        "missing"
    };
    write_smoke_status(&format!(
        "startup permission {} is {state}",
        bongocat_platform::STARTUP_PERMISSION_CAPABILITY
    ))?;
    Ok(())
}

#[cfg(target_os = "macos")]
pub(crate) fn run_startup_item_smoke() -> Result<(), Box<dyn std::error::Error>> {
    use bongocat_platform::{
        StartupItemEnvironment, StartupItemState, set_startup_item_enabled, startup_item_state,
    };

    if bongocat_app::BUILD_ENVIRONMENT != bongocat_config::BuildEnvironment::Production {
        return Err("startup-item mutation smoke requires a Production build".into());
    }
    let environment = StartupItemEnvironment::Production;
    let original = startup_item_state(environment)?;
    write_smoke_status(&format!("startup-item original state {original:?}"))?;

    let exercise: Result<(), String> = (|| match original {
        StartupItemState::Disabled | StartupItemState::NotFound => {
            let enabled =
                set_startup_item_enabled(environment, true).map_err(|error| error.to_string())?;
            if !matches!(
                enabled,
                StartupItemState::Enabled | StartupItemState::RequiresApproval
            ) {
                Err(format!(
                    "startup-item enable returned an unexpected state: {enabled:?}"
                ))
            } else {
                Ok(())
            }
        }
        StartupItemState::Enabled | StartupItemState::RequiresApproval => {
            let disabled =
                set_startup_item_enabled(environment, false).map_err(|error| error.to_string())?;
            if disabled != StartupItemState::Disabled {
                Err(format!(
                    "startup-item disable returned an unexpected state: {disabled:?}"
                ))
            } else {
                Ok(())
            }
        }
        StartupItemState::Unsupported(reason) => Err(format!(
            "startup-item capability is unsupported: {reason:?}"
        )),
        StartupItemState::Stale => Err(format!(
            "startup-item bundle produced an invalid initial state: {original:?}"
        )),
    })();

    let restoration = match original {
        StartupItemState::Disabled | StartupItemState::NotFound => {
            set_startup_item_enabled(environment, false)
        }
        StartupItemState::Enabled | StartupItemState::RequiresApproval => {
            set_startup_item_enabled(environment, true)
        }
        state => Ok(state),
    };
    exercise.map_err(io::Error::other)?;
    let restored = restoration?;
    let restored_matches = restored == original
        || (original == StartupItemState::NotFound && restored == StartupItemState::Disabled);
    if !restored_matches {
        return Err(format!(
            "startup-item state was not restored: expected {original:?}, got {restored:?}"
        )
        .into());
    }
    write_smoke_status(&format!("startup-item restored state {restored:?}"))?;
    Ok(())
}

#[cfg(feature = "storage-test-injection")]
struct SmokeRoot(PathBuf);

#[cfg(feature = "storage-test-injection")]
impl SmokeRoot {
    fn cleanup(mut self) -> io::Result<()> {
        let result = std::fs::remove_dir_all(&self.0);
        self.0 = PathBuf::new();
        result
    }
}

#[cfg(feature = "storage-test-injection")]
impl Drop for SmokeRoot {
    fn drop(&mut self) {
        if !self.0.as_os_str().is_empty() {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}

#[cfg(feature = "storage-test-injection")]
pub(crate) fn run_settings_window_state_smoke() -> Result<(), Box<dyn std::error::Error>> {
    use bongocat_config::{
        BuildEnvironment, ConfigStore, Language, StorageLayout, Theme, WindowPlacement,
        WindowState, WindowStateStore,
    };
    use bongocat_ui_protocol::{SettingsLanguage, SettingsTheme};

    const RESIZED_WIDTH: u32 = 700;
    const RESIZED_HEIGHT: u32 = 520;

    let root = env::temp_dir().join(format!(
        "bongocat-settings-window-state-smoke-{}",
        std::process::id()
    ));
    if root.exists() {
        std::fs::remove_dir_all(&root)?;
    }
    let root = SmokeRoot(root);
    let layout = StorageLayout::under(&root.0, BuildEnvironment::Development);
    let config_store = ConfigStore::new(layout.clone())?;
    let mut config = config_store.load_or_default()?.config;
    config.appearance.theme = Theme::Dark;
    config.appearance.language = Language::ChineseSimplified;
    config_store.commit(&config)?;
    drop(config_store);
    WindowStateStore::new(layout.clone()).commit(&WindowState::with_settings_window(Some(
        WindowPlacement::new(999_000, 999_000, 800, 600, false)?,
    )))?;
    let application =
        bongocat_app::Application::start_with_layout_for_smoke(layout.clone(), preset_root())?;
    let service = bongocat_app::ApplicationSettingsService::start(application)?;
    let client = service.client();
    let window_state = service.window_state();
    let gpui_application = gpui_application().with_assets(AllAssets);
    gpui_application.run(move |cx| {
        let window =
            match open_settings_window(
                client.clone(),
                window_state.clone(),
                // The seed the product derives from its startup snapshot, here matching
                // the configuration this smoke just committed.
                SettingsWindowSeed {
                    language: SettingsLanguage::ChineseSimplified,
                    appearance_theme: SettingsTheme::Dark,
                },
                SettingsNavigationMemory::new(),
                true,
                |cx| cx.quit(),
                |_: &mut App| {},
                cx,
            ) {
                Ok(window) => window,
                Err(error) => {
                    let _ = writeln!(
                        io::stderr().lock(),
                        "settings window state smoke failed: {error}"
                    );
                    let _ = std::fs::remove_dir_all(&root.0);
                    std::process::exit(1);
                }
            };
        cx.spawn(async move |cx| {
            let result = async {
                // The window is already on screen here, and the snapshot the loop below
                // waits for may still be in flight: this is the frame the seed answers,
                // and the one the user would otherwise watch redraw from English.
                let seeded = window
                    .update(cx, |view, _, _| view.display_language_for_smoke())
                    .map_err(|error| io::Error::other(format!("read seeded language: {error}")))?;
                if seeded != SettingsLanguage::ChineseSimplified {
                    return Err(io::Error::other(format!(
                        "settings window opened in {seeded:?}, expected the configured \
                         Simplified Chinese before its first snapshot"
                    ))
                    .into());
                }
                let mut appearance_verified = false;
                let mut last_appearance_error = None;
                for _ in 0..200 {
                    let appearance =
                        window.update(cx, |view, _, cx| {
                            view.show_appearance_page_for_smoke(cx)?;
                            if view.resolved_language_for_smoke()
                                != Some(SettingsLanguage::ChineseSimplified)
                            {
                                return Err(
                                    "settings snapshot did not resolve Simplified Chinese"
                                        .to_owned(),
                                );
                            }
                            Ok(())
                        });
                    match appearance {
                        Ok(Ok(())) => {
                            appearance_verified = true;
                            break;
                        }
                        Ok(Err(error)) => last_appearance_error = Some(error),
                        Err(error) => last_appearance_error = Some(error.to_string()),
                    }
                    Timer::after(Duration::from_millis(10)).await;
                }
                if !appearance_verified {
                    let detail = last_appearance_error
                        .unwrap_or_else(|| "settings view was unavailable".to_owned());
                    return Err(io::Error::other(format!(
                        "settings window did not apply the configured theme and localization: {detail}"
                    ))
                    .into());
                }
                write_smoke_status("Chinese Appearance and language localization verified")?;
                let mut shortcuts_verified = false;
                let mut last_shortcuts_error = None;
                for _ in 0..200 {
                    let shortcuts = window.update(cx, |view, _, cx| {
                        view.show_shortcuts_page_for_smoke(cx)
                    });
                    match shortcuts {
                        Ok(Ok(())) => {
                            shortcuts_verified = true;
                            break;
                        }
                        Ok(Err(error)) => last_shortcuts_error = Some(error),
                        Err(error) => last_shortcuts_error = Some(error.to_string()),
                    }
                    Timer::after(Duration::from_millis(10)).await;
                }
                if !shortcuts_verified {
                    let detail = last_shortcuts_error
                        .unwrap_or_else(|| "settings view was unavailable".to_owned());
                    return Err(io::Error::other(format!(
                        "settings window did not apply Shortcuts localization: {detail}"
                    ))
                    .into());
                }
                write_smoke_status("Chinese Shortcuts localization verified")?;
                let mut models_verified = false;
                let mut last_models_error = None;
                for _ in 0..200 {
                    let models =
                        window.update(cx, |view, _, cx| {
                            view.show_model_library_localization_for_smoke(cx)
                        });
                    match models {
                        Ok(Ok(())) => {
                            models_verified = true;
                            break;
                        }
                        Ok(Err(error)) => last_models_error = Some(error),
                        Err(error) => last_models_error = Some(error.to_string()),
                    }
                    Timer::after(Duration::from_millis(10)).await;
                }
                if !models_verified {
                    let detail = last_models_error
                        .unwrap_or_else(|| "settings view was unavailable".to_owned());
                    return Err(io::Error::other(format!(
                        "settings window did not apply Models localization: {detail}"
                    ))
                    .into());
                }
                write_smoke_status("Chinese Model library localization verified")?;
                let mut initial = None;
                let mut last_initial = window_state.placement();
                for _ in 0..200 {
                    let current = window_state.placement();
                    last_initial = current;
                    if current.is_some_and(|placement| {
                        placement.x != 999_000
                            && placement.y != 999_000
                            && (placement.width, placement.height) == (800, 600)
                    }) {
                        initial = current;
                        break;
                    }
                    Timer::after(Duration::from_millis(10)).await;
                }
                let initial = initial.ok_or_else(|| {
                    let size = last_initial
                        .map(|placement| format!("{}x{}", placement.width, placement.height))
                        .unwrap_or_else(|| "unavailable".to_owned());
                    io::Error::other(format!(
                        "default settings window content size was {size}, expected 800x600"
                    ))
                })?;
                window
                    .update(cx, |_, window, _| {
                        window.resize(size(
                            px(RESIZED_WIDTH as f32),
                            px(RESIZED_HEIGHT as f32),
                        ));
                    })
                    .map_err(|error| {
                        io::Error::other(format!("resize settings window: {error}"))
                    })?;
                let mut expected = None;
                let mut last_resized = window_state.placement();
                for _ in 0..200 {
                    let current = window_state.placement();
                    last_resized = current;
                    if current.is_some_and(|placement| {
                        (placement.width, placement.height) == (RESIZED_WIDTH, RESIZED_HEIGHT)
                    })
                    {
                        expected = current;
                        break;
                    }
                    Timer::after(Duration::from_millis(10)).await;
                }
                let expected = expected.ok_or_else(|| {
                    let observed = last_resized
                        .map(|placement| format!("{}x{}", placement.width, placement.height))
                        .unwrap_or_else(|| "unavailable".to_owned());
                    io::Error::other(format!(
                        "settings window bounds observer reported {observed}, expected {RESIZED_WIDTH}x{RESIZED_HEIGHT}"
                    ))
                })?;
                if (expected.x, expected.y, expected.maximized)
                    != (initial.x, initial.y, initial.maximized)
                {
                    return Err(io::Error::other(
                        "resizing settings window changed its position or maximized state",
                    )
                    .into());
                }
                client
                    .shutdown()
                    .await
                    .map_err(|error| io::Error::other(error.to_string()))?;
                service
                    .join()
                    .map_err(|error| io::Error::other(error.to_string()))?;
                let persisted = WindowStateStore::new(layout.clone()).load_or_default().state;
                let expected = WindowPlacement::new(
                    expected.x,
                    expected.y,
                    expected.width,
                    expected.height,
                    expected.maximized,
                )?;
                if persisted.settings_window != Some(expected) {
                    return Err(io::Error::other(
                        "settings window state did not match observed GPUI bounds",
                    )
                    .into());
                }
                let restarted = bongocat_app::Application::start_with_layout_for_smoke(
                    layout,
                    preset_root(),
                )?;
                if restarted.settings_window_placement() != Some(expected) {
                    return Err(io::Error::other(
                        "application restart did not restore settings window state",
                    )
                    .into());
                }
                restarted.shutdown()?;
                root.cleanup()?;
                write_smoke_status("settings window state restored after restart")?;
                Ok::<(), Box<dyn std::error::Error>>(())
            }
            .await;
            if let Err(error) = result {
                let mut stderr = io::stderr().lock();
                let _ = writeln!(stderr, "settings window state smoke failed: {error}");
                let _ = stderr.flush();
                std::process::exit(1);
            }
            cx.update(|cx| cx.quit());
        })
        .detach();
    });
    Ok(())
}

#[cfg(feature = "storage-test-injection")]
const PANIC_DIAGNOSTICS_SMOKE_ROOT_ENV: &str = "BONGOCAT_PANIC_DIAGNOSTICS_SMOKE_ROOT";

#[cfg(feature = "storage-test-injection")]
const PANIC_DIAGNOSTICS_SMOKE_PAYLOAD: &str = "panic-smoke-sensitive-payload";

#[cfg(feature = "storage-test-injection")]
pub(crate) fn run_panic_diagnostics_smoke_child() -> Result<(), Box<dyn std::error::Error>> {
    use bongocat_config::{BuildEnvironment, StorageLayout};

    let root = env::var_os(PANIC_DIAGNOSTICS_SMOKE_ROOT_ENV)
        .ok_or("panic diagnostics child is missing its isolated storage root")?;
    let root = PathBuf::from(root);
    if !root.is_absolute() {
        return Err("panic diagnostics child storage root must be absolute".into());
    }
    let layout = StorageLayout::under(&root, BuildEnvironment::Development);
    let mut application =
        bongocat_app::Application::start_with_layout_for_smoke(layout, preset_root())?;
    application.install_process_panic_hook();
    panic!("{PANIC_DIAGNOSTICS_SMOKE_PAYLOAD}: {}", root.display());
}

#[cfg(feature = "storage-test-injection")]
fn read_application_logs(directory: &Path) -> io::Result<String> {
    let mut paths = Vec::new();
    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let name = entry.file_name();
        if name
            .to_str()
            .is_some_and(|name| is_log_file_name(LogStream::Application, name))
        {
            paths.push(entry.path());
        }
    }
    paths.sort();
    let mut logs = String::new();
    for path in paths {
        logs.push_str(&std::fs::read_to_string(path)?);
    }
    Ok(logs)
}

#[cfg(feature = "storage-test-injection")]
fn contains_application_event(
    logs: &str,
    code: bongocat_app::ApplicationLogCode,
    level: LogLevel,
) -> bool {
    logs.lines().any(|line| {
        parse_log_line(line).is_some_and(|parsed| {
            parsed.code == code.as_str()
                && parsed.module == code.component().as_str()
                && parsed.level == level
        })
    })
}

#[cfg(feature = "storage-test-injection")]
pub(crate) fn run_diagnostics_export_smoke() -> Result<(), Box<dyn std::error::Error>> {
    use bongocat_config::{BuildEnvironment, StorageLayout};

    let root = env::temp_dir().join(format!(
        "bongocat-diagnostics-export-smoke-{}",
        std::process::id()
    ));
    if root.exists() {
        std::fs::remove_dir_all(&root)?;
    }
    let root = SmokeRoot(root);
    let layout = StorageLayout::under(&root.0, BuildEnvironment::Development);
    let application =
        bongocat_app::Application::start_with_layout_for_smoke(layout.clone(), preset_root())?;
    let service = bongocat_app::ApplicationSettingsService::start(application)?;
    let client = service.client();
    let exported = client.export_diagnostics_blocking()?;
    let status = exported
        .diagnostics_export
        .ok_or("diagnostics export did not return a typed result")?;
    if status.format_version != 1
        || status.preview_bundle_format_version != 1
        || status.preview_bundle_entry_count != 3
        || status.preview_bundle_skipped_source_files != 0
        || status.bytes_written == 0
        || status.preview_bundle_bytes_written == 0
    {
        return Err("diagnostics export returned an invalid typed result".into());
    }

    let diagnostics = layout.logs.join("diagnostics.json");
    let preview = layout.logs.join("diagnostics-preview.zip");
    if !diagnostics.is_file() || !preview.is_file() {
        return Err("diagnostics export did not create both private files".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if std::fs::metadata(&diagnostics)?.permissions().mode() & 0o777 != 0o600
            || std::fs::metadata(&preview)?.permissions().mode() & 0o777 != 0o600
        {
            return Err("diagnostics export did not preserve private file permissions".into());
        }
    }
    let mut archive = ZipArchive::new(std::fs::File::open(&preview)?)?;
    let mut entries = (0..archive.len())
        .map(|index| archive.by_index(index).map(|entry| entry.name().to_owned()))
        .collect::<Result<Vec<_>, _>>()?;
    entries.sort_unstable();
    if entries
        != [
            "application-events.log",
            "diagnostics.json",
            "manifest.json",
        ]
    {
        return Err("diagnostics preview archive entries diverged from the v1 contract".into());
    }
    let mut application_events = String::new();
    std::io::Read::read_to_string(
        &mut archive.by_name("application-events.log")?,
        &mut application_events,
    )?;
    if !application_events
        .lines()
        .any(|line| line == "INFO  [application] application/started")
    {
        return Err(
            "diagnostics preview did not contain the canonical application start event".into(),
        );
    }

    client.shutdown_blocking()?;
    service.join()?;
    root.cleanup()?;
    write_smoke_status("diagnostics export completed with a private preview bundle")?;
    Ok(())
}

#[cfg(feature = "storage-test-injection")]
pub(crate) fn run_diagnostics_export_failure_smoke() -> Result<(), Box<dyn std::error::Error>> {
    use bongocat_config::{BuildEnvironment, StorageLayout};

    let root = env::temp_dir().join(format!(
        "bongocat-diagnostics-export-failure-smoke-{}",
        std::process::id()
    ));
    if root.exists() {
        std::fs::remove_dir_all(&root)?;
    }
    let root = SmokeRoot(root);
    let layout = StorageLayout::under(&root.0, BuildEnvironment::Development);
    let application =
        bongocat_app::Application::start_with_layout_for_smoke(layout.clone(), preset_root())?;
    let service = bongocat_app::ApplicationSettingsService::start(application)?;
    let client = service.client();
    let first = client.export_diagnostics_blocking()?;
    let first_status = first
        .diagnostics_export
        .ok_or("initial diagnostics export did not return a typed result")?;
    let diagnostics = layout.logs.join("diagnostics.json");
    let preview = layout.logs.join("diagnostics-preview.zip");
    let previous_diagnostics = std::fs::read(&diagnostics)?;
    let previous_preview = std::fs::read(&preview)?;

    // A directory at the destination is an OS-level replace/open failure. The writer must
    // reject it before touching the existing diagnostics or preview bytes.
    std::fs::remove_file(&diagnostics)?;
    std::fs::create_dir(&diagnostics)?;
    let error = client
        .export_diagnostics_blocking()
        .expect_err("diagnostics export must reject a directory destination");
    if error.code() != bongocat_ui_protocol::SettingsErrorCode::DiagnosticsExportFailed {
        return Err("diagnostics export returned an unstable filesystem failure code".into());
    }
    if std::fs::read(&preview)? != previous_preview {
        return Err("failed diagnostics export changed the previous preview bundle".into());
    }
    std::fs::remove_dir(&diagnostics)?;
    std::fs::write(&diagnostics, &previous_diagnostics)?;

    #[cfg(target_os = "macos")]
    {
        // Marking the existing preview bundle immutable makes the atomic commit fail after the
        // staging file has been fully written. Unlike a directory or a read-only parent, this is
        // an OS-level failure the current process cannot bypass through `set_private_directory`,
        // so it deterministically exercises the commit-failure recovery path.
        let immutable = std::process::Command::new("/usr/bin/chflags")
            .arg("uchg")
            .arg(&preview)
            .status()?;
        if !immutable.success() {
            return Err("failed to mark the previous preview bundle immutable".into());
        }
        let result = client.export_diagnostics_blocking();
        let cleared = std::process::Command::new("/usr/bin/chflags")
            .arg("nouchg")
            .arg(&preview)
            .status()?;
        if !cleared.success() {
            return Err("failed to clear the immutable preview bundle flag".into());
        }
        let error =
            result.expect_err("diagnostics export must reject an immutable preview destination");
        if error.code() != bongocat_ui_protocol::SettingsErrorCode::DiagnosticsExportFailed {
            return Err("immutable diagnostics preview returned an unstable error code".into());
        }
        if std::fs::read(&preview)? != previous_preview {
            return Err("preview commit failure changed the previous preview bundle".into());
        }
    }

    let staging = std::fs::read_dir(&layout.logs)?
        .filter_map(Result::ok)
        .map(|entry| entry.file_name())
        .filter_map(|name| name.into_string().ok())
        .filter(|name| name.starts_with(".diagnostics-preview.zip."))
        .collect::<Vec<_>>();
    if !staging.is_empty() {
        return Err(format!("failed export left staging files: {staging:?}").into());
    }
    if first_status.preview_bundle_entry_count != 3 {
        return Err("initial diagnostics export returned an invalid preview status".into());
    }

    client.shutdown_blocking()?;
    service.join()?;
    root.cleanup()?;
    write_smoke_status("diagnostics export filesystem failures preserved the previous bundle")?;
    Ok(())
}

#[cfg(feature = "storage-test-injection")]
pub(crate) fn run_panic_diagnostics_smoke() -> Result<(), Box<dyn std::error::Error>> {
    use bongocat_config::{BuildEnvironment, StorageLayout};

    let root = env::temp_dir().join(format!(
        "bongocat-panic-diagnostics-smoke-{}",
        std::process::id()
    ));
    if root.exists() {
        std::fs::remove_dir_all(&root)?;
    }
    let root = SmokeRoot(root);
    let layout = StorageLayout::under(&root.0, BuildEnvironment::Development);
    let mut child = std::process::Command::new(env::current_exe()?)
        .arg("--panic-diagnostics-smoke-child")
        .env(PANIC_DIAGNOSTICS_SMOKE_ROOT_ENV, &root.0)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()?;
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    let timed_out = loop {
        if child.try_wait()?.is_some() {
            break false;
        }
        if std::time::Instant::now() >= deadline {
            child.kill()?;
            break true;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let child = child.wait_with_output()?;
    if timed_out {
        return Err("panic diagnostics child exceeded 10 seconds".into());
    }
    if child.status.success() {
        return Err("panic diagnostics child exited successfully".into());
    }
    let child_output = format!(
        "{}{}",
        String::from_utf8_lossy(&child.stdout),
        String::from_utf8_lossy(&child.stderr)
    );
    if child_output.contains(PANIC_DIAGNOSTICS_SMOKE_PAYLOAD)
        || child_output.contains(root.0.to_string_lossy().as_ref())
    {
        return Err("panic diagnostics child exposed its payload or storage path".into());
    }

    let run_marker = layout.logs.join("application-running.marker");
    if !run_marker.is_file() {
        return Err("panic diagnostics child did not preserve the unclean run marker".into());
    }
    let crashed_logs = read_application_logs(&layout.logs)?;
    if !contains_application_event(
        &crashed_logs,
        bongocat_app::ApplicationLogCode::Panicked,
        LogLevel::Error,
    ) {
        return Err("panic diagnostics child did not persist the stable panic record".into());
    }
    if crashed_logs.contains(PANIC_DIAGNOSTICS_SMOKE_PAYLOAD)
        || crashed_logs.contains(root.0.to_string_lossy().as_ref())
    {
        return Err("persistent panic diagnostics exposed their payload or storage path".into());
    }
    let config_after_crash = std::fs::read(&layout.config)?;

    let restarted =
        bongocat_app::Application::start_with_layout_for_smoke(layout.clone(), preset_root())?;
    let diagnostics = restarted.application_log_diagnostics();
    if diagnostics.events.previous_run_unclean != 1 || diagnostics.events.started != 1 {
        return Err("application restart did not classify the aborted run as unclean".into());
    }
    restarted.shutdown()?;
    if run_marker.exists() {
        return Err("clean restart shutdown did not remove the run marker".into());
    }
    if std::fs::read(&layout.config)? != config_after_crash {
        return Err("panic diagnostics or restart changed the current configuration".into());
    }
    let completed_logs = read_application_logs(&layout.logs)?;
    if !contains_application_event(
        &completed_logs,
        bongocat_app::ApplicationLogCode::ShutdownCompleted,
        LogLevel::Info,
    ) {
        return Err("clean restart did not persist its completed shutdown record".into());
    }

    write_smoke_status("panic diagnostics recovered after crash")?;
    root.cleanup()?;
    Ok(())
}
