//! The settings window's own state: placement, size and visibility.
//!
//! Driven through the real window rather than through the store, because what is
//! under test is the window's response to a placement — a store that reported the
//! right value while the window opened somewhere else would pass a store test
//! and fail this one.

#[cfg(feature = "storage-test-injection")]
use super::SmokeRoot;
#[cfg(feature = "storage-test-injection")]
use crate::gpui_application;
#[cfg(feature = "storage-test-injection")]
use crate::preset_root;
#[cfg(feature = "storage-test-injection")]
use crate::write_smoke_status;
#[cfg(feature = "storage-test-injection")]
use async_io::Timer;
#[cfg(feature = "storage-test-injection")]
use bongocat_ui::SettingsNavigationMemory;
#[cfg(feature = "storage-test-injection")]
use bongocat_ui::SettingsWindowSeed;
#[cfg(feature = "storage-test-injection")]
use bongocat_ui::open_settings_window;
#[cfg(feature = "storage-test-injection")]
use gpui_kit::App;
#[cfg(feature = "storage-test-injection")]
use gpui_kit::assets::AllAssets;
#[cfg(feature = "storage-test-injection")]
use gpui_kit::px;
#[cfg(feature = "storage-test-injection")]
use gpui_kit::size;
#[cfg(feature = "storage-test-injection")]
use std::env;
#[cfg(feature = "storage-test-injection")]
use std::io;
#[cfg(feature = "storage-test-injection")]
use std::io::Write;
#[cfg(feature = "storage-test-injection")]
use std::time::Duration;

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
