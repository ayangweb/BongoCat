//! Markdown rendering for the update window's release notes.
//!
//! The changelog comes from the release manifest, so it is **untrusted input**: it travels
//! over the network, and whoever can replace the manifest controls every byte of it. It is
//! also the only place the product renders text it did not author.
//!
//! [`gpui_kit`]'s `TextView` does the parsing, the inline layout and the scrolling. What
//! this module owns is the policy that makes handing a manifest to it safe, and it is
//! deliberately three rules rather than a renderer:
//!
//! 1. **Bound the input before anything parses it.** [`prepare`] caps the notes by size and
//!    by how many block containers they may open. The size cap bounds the work; the marker
//!    cap bounds the *depth* of the tree `TextView` builds, because it walks that tree with
//!    one Rust call per level of nesting and the main thread's stack is not large enough to
//!    follow a document written to exhaust it.
//! 2. **Never fetch anything the manifest names.** [`NoRemoteImage`] claims every image
//!    node, so `gpui-kit` never builds an `ImageSource` and there is no URL to hand to
//!    GPUI's resource loader. The refusal happens before the URL is looked at, not after.
//! 3. **Never present a target this product will not open as a control.**
//!    [`RejectedLink`] claims a link whose target [`clickable_link`] refuses and renders
//!    its label as plain text. Accepted links are left to `TextView`, whose click handler
//!    still routes through the platform opener —which re-checks the scheme.
//!
//! Raw HTML is inert for the same reason: left alone, `gpui-kit` parses a Markdown HTML
//! node with its HTML parser and lays the result out, which is how a `<strong>` in a
//! manifest becomes bold text and an `<img src>` becomes a fetch. [`LiteralHtml`] claims
//! those nodes in both the inline and the block position and shows the source instead.
//!
//! # What the manifest therefore cannot do
//!
//! - Make the application issue a request. No image node is ever built, from either the
//!   inline `![alt](url)` form or the `[alt][ref]` form, and no raw HTML is interpreted.
//! - Offer a `javascript:`, `file:`, `data:` or whitespace-smuggled target as something
//!   that looks pressable.
//! - Choose how much stack the renderer spends, or how many elements the window is asked
//!   to lay out.
//!
//! # Why the plugins and not a hand-written renderer
//!
//! `gpui-kit` consults its Markdown plugins *before* its built-in node handling, in both
//! the inline and the block dispatcher, so a plugin that claims a node prevents the
//! built-in path from ever seeing it. That is what makes rules 2 and 3 enforceable here
//! rather than only describable. What is left for this module is the text each refusal
//! shows, and each of those is a plain function over the parsed node so it can be tested
//! without a window.

use gpui_kit::base::{
    MarkdownExtensions, MarkdownNode, MarkdownParseContext, MarkdownPlugin, TextView, markdown_ast,
};
use gpui_kit::component::ActiveTheme;
use gpui_kit::{App, IntoElement, Window, div, prelude::*};
use std::sync::OnceLock;

mod bounds;
mod html;
mod image;
mod link;
#[cfg(test)]
mod tests;
mod text;

// The five sections are one renderer: the registry in the root names every
// plugin, and a refusal in one section asks the shared decisions in another. They
// reach each other through this one prelude rather than naming two modules apiece.
use bounds::*;
use html::*;
use image::*;
use link::*;
use text::*;

/// The element id `TextView` keys its parsed document under.
///
/// It has to be stable across frames: `TextView` caches the parse against this id, so a
/// changing id would re-parse the whole changelog on every frame.
pub(crate) const NOTES_VIEW_ID: &str = "update-release-notes";

/// A fixed parser revision, so rebuilding the plugin handles each frame is not mistaken
/// for a changed parser configuration.
pub(crate) const PARSER_REVISION: u64 = 1;

/// Render the changelog.
///
/// `TextView` keys its parsed document on [`NOTES_VIEW_ID`] and compares the text it is
/// handed, so the notes are parsed once and re-parsed only when they change. The copy made
/// here per frame is a string copy, not a re-parse.
///
/// The scroll box around this is sized by its content up to a constant and no further;
/// `TextView` is therefore left unscrollable so that the two do not fight over the
/// height.
pub(crate) fn render(notes: &str) -> TextView {
    TextView::markdown(NOTES_VIEW_ID, prepare(notes))
        .markdown_extensions(extensions())
        .scrollable(false)
        .text_xs()
        .on_link_click(|url, _, _, _| {
            // The URL that reaches here is the one `gpui-kit` resolved, which for a link
            // *reference* is only knowable at this point. The opener re-checks the scheme
            // regardless, so this is the second of two independent refusals rather than
            // the only one.
            let _ = bongocat_platform::open_external_url(url.as_ref());
        })
}
