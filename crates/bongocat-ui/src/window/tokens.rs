//! The colours the pages draw with, read from the active theme.
//!
//! Semantic colours are the component library's, read through its active theme
//! rather than hardcoded here, so a light/dark switch needs no branch in a page.
//! `danger` is the one colour with no page chrome: it belongs to the
//! confirmation surfaces, which are the only place the window says "this cannot
//! be undone".

use super::*;

#[derive(Clone, Copy)]
pub(crate) struct Tokens {
    pub(crate) canvas: Hsla,
    pub(crate) overlay: Hsla,
    pub(crate) border: Hsla,
    pub(crate) text: Hsla,
    pub(crate) muted: Hsla,
    pub(crate) accent: Hsla,
    /// The colour of an action that destroys something. Nothing in the page
    /// chrome uses it; it is here for the confirmation surfaces, which are the
    /// only place the page says "this cannot be undone".
    pub(crate) danger: Hsla,
}

impl Tokens {
    pub(crate) fn from_theme(cx: &App) -> Self {
        let theme = cx.theme();
        Self {
            canvas: theme.background,
            overlay: theme.overlay,
            border: theme.border,
            text: theme.foreground,
            muted: theme.muted_foreground,
            accent: theme.primary,
            danger: theme.danger,
        }
    }
}
