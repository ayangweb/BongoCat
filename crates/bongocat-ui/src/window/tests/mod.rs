//! The settings window's tests, split by the page or control each one covers.
//!
//! The fixtures every module reaches live here; each subject module holds only
//! the tests for one surface.

use super::shortcuts_page::ShortcutScope;
use super::*;
use crate::{SettingsModelBehaviorBinding, SettingsShortcutBinding};
use gpui_kit::base::TestSupportExt as _;
use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{
    ElementId, FileDropEvent, InputEvent as _, Keystroke, Modifiers, TestAppContext,
    VisualTestContext,
};

mod copy;
mod model_actions;
mod model_drag_overlay;
mod model_editor_render;
mod model_import;
mod models_page_render;
mod settings_commands;
mod settings_page;
mod shortcuts;
mod startup_item;

/// A catalog entry for tests that do not care where the model lives.
///
/// Only the settings page reads `directory` and `cover`; every other assertion
/// in this module is about identity, availability or actions, so those two stay
/// unset unless the test is specifically about opening a location or showing a
/// cover.
fn settings_model_key(id: &str, origin: SettingsModelOrigin) -> SettingsModelKey {
    SettingsModelKey {
        id: id.to_owned(),
        origin,
    }
}

fn model_entry(
    id: &str,
    origin: SettingsModelOrigin,
    availability: SettingsModelAvailability,
) -> SettingsModelEntry {
    SettingsModelEntry {
        id: id.to_owned(),
        title: "untitled".to_owned(),
        input_mode: Some(SettingsModelMode::Standard),
        origin,
        availability,
        directory: None,
        cover: None,
    }
}

fn model_entry_in_mode(
    id: &str,
    origin: SettingsModelOrigin,
    mode: SettingsModelMode,
    availability: SettingsModelAvailability,
) -> SettingsModelEntry {
    SettingsModelEntry {
        input_mode: Some(mode),
        ..model_entry(id, origin, availability)
    }
}

fn key(key: &str, key_char: Option<&str>) -> KeyDownEvent {
    KeyDownEvent {
        keystroke: Keystroke {
            modifiers: Modifiers::default(),
            key: key.to_owned(),
            key_char: key_char.map(str::to_owned),
        },
        is_held: false,
        prefer_character_input: false,
    }
}

fn captured_shortcut(key: &str, modifiers: Modifiers) -> Option<String> {
    let keys = capture_key(key).into_iter().collect();
    shortcut_from_capture(&modifiers, &keys)
}

fn settings_view_with_endpoint(
    cx: &mut TestAppContext,
) -> (
    Entity<SettingsView>,
    &mut VisualTestContext,
    crate::SettingsServiceEndpoint,
) {
    cx.update(gpui_kit::init);
    let (client, endpoint) = crate::SettingsClient::bounded(4);
    let built = Rc::new(RefCell::new(None));
    let capture = Rc::clone(&built);
    let (_, visual) = cx.add_window_view(move |window, cx| {
        let view = cx.new(|cx| {
            SettingsView::new(
                client,
                SettingsWindowSeed {
                    language: SettingsLanguage::EnglishUnitedStates,
                    appearance_theme: SettingsTheme::System,
                },
                Rc::new(|_| {}),
                Rc::new(|_| {}),
                window,
                cx,
            )
        });
        capture.borrow_mut().replace(view.clone());
        Root::new(view, window, cx)
    });
    let view = built
        .borrow_mut()
        .take()
        .expect("the window builder must hand the page out");
    (view, visual, endpoint)
}

fn settings_view(cx: &mut TestAppContext) -> (Entity<SettingsView>, &mut VisualTestContext) {
    let (view, visual, _endpoint) = settings_view_with_endpoint(cx);
    (view, visual)
}

/// The models page's own content, with the import card and the model cards in it.
///
/// The page renders through `SettingItem::render`, which only runs for the page
/// the settings component has selected, so this harness calls
/// `models::content` the same way that closure does rather than trying to make
/// the component select a page.
struct ModelsPageHarness {
    view: Entity<SettingsView>,
    snapshot: Option<SettingsSnapshot>,
}

impl Render for ModelsPageHarness {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let snapshot = self.snapshot.clone();
        let tokens = Tokens::from_theme(cx);
        self.view
            .clone()
            .update(cx, move |view, cx| {
                super::models::content(view, window, cx, snapshot.as_ref(), tokens)
            })
            .into_any_element()
    }
}

/// Reproduce the real `Settings -> SettingGroup -> SettingItem` wrapper around
/// the model catalog. The catalog's own harness bypasses this list layout, which
/// is where a child cannot influence a container-query element's height.
struct WrappedModelsPageHarness {
    view: Entity<SettingsView>,
    snapshot: Option<SettingsSnapshot>,
}

impl Render for WrappedModelsPageHarness {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let snapshot = self.snapshot.clone();
        let view = self.view.clone();
        let page = SettingPage::new("Models").group(
            SettingGroup::new()
                .variant(GroupBoxVariant::Normal)
                .item(SettingItem::render(move |_, window, app| {
                    let tokens = Tokens::from_theme(app);
                    let snapshot = snapshot.clone();
                    view.clone().update(app, move |view, cx| {
                        super::models::content(view, window, cx, snapshot.as_ref(), tokens)
                            .into_any_element()
                    })
                })),
        );

        div().size_full().child(
            Settings::new("wrapped-models-page")
                .sidebar_width(px(220.0))
                .with_group_variant(GroupBoxVariant::Outline)
                .page(page),
        )
    }
}

/// The bounds an element was painted at.
fn rendered_bounds(visual: &mut VisualTestContext, id: ElementId) -> Bounds<Pixels> {
    visual.update(|window, _| {
        let drawn = id.clone();
        window
            .try_find(id)
            .unwrap_or_else(|| panic!("{drawn:?} must be drawn"))
            .bounds()
    })
}
