//! The settings-window-wide model drop affordance.

use super::*;
use gpui_kit::assets::IconName;
use gpui_kit::base::TestSupportExt as _;
use gpui_kit::component::{Sizable as _, Size};

/// What the drop overlay can currently offer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ModelDragOverlayState {
    /// One path was dropped over the window and import may start.
    Ready,
    /// Another source operation owns the model import flow.
    Busy,
    /// Zero or multiple paths cannot form the one-folder import contract.
    InvalidSelection,
}

struct ModelDragOverlayCopy {
    title: &'static str,
    icon: IconName,
    actionable: bool,
}

impl ModelDragOverlayCopy {
    fn for_state(state: ModelDragOverlayState) -> Self {
        match state {
            ModelDragOverlayState::Ready => Self {
                title: "models.import.drop.ready.title",
                icon: IconName::FolderInput,
                actionable: true,
            },
            ModelDragOverlayState::Busy => Self {
                title: "models.import.drop.busy.title",
                icon: IconName::Clock,
                actionable: false,
            },
            ModelDragOverlayState::InvalidSelection => Self {
                title: "models.import.drop.invalid.title",
                icon: IconName::FolderX,
                actionable: false,
            },
        }
    }
}

/// Render the drop state above the entire settings surface.
///
/// The backdrop is deliberately outside the models page: file drops are a
/// window-level gesture, and requiring the pointer to find the upload card
/// would make the same action depend on the page currently selected. The panel
/// is visual-only; accepting or rejecting the drop remains an event concern.
pub(super) fn render(
    state: ModelDragOverlayState,
    language: SettingsLanguage,
    tokens: Tokens,
) -> gpui_kit::base::ObservedElement<Stateful<Div>> {
    let copy = ModelDragOverlayCopy::for_state(state);
    let accent = if copy.actionable {
        tokens.accent
    } else {
        tokens.muted
    };
    let locale = language.catalog_locale();
    let title = bongocat_i18n::text(locale, copy.title);

    div()
        .id("model-drop-overlay")
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .p_8()
        .bg(tokens.overlay)
        .test_support()
        .child(
            div()
                .id("model-drop-panel")
                .w(px(420.0))
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap_3()
                .p_8()
                .text_color(tokens.text)
                .text_center()
                .child(
                    div()
                        .size_12()
                        .rounded_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .bg(accent.opacity(0.12))
                        .child(
                            Icon::new(copy.icon)
                                .with_size(Size::Large)
                                .text_color(accent),
                        ),
                )
                .child(div().text_lg().child(title))
                .test_support(),
        )
}
