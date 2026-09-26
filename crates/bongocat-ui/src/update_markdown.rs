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

/// The element id `TextView` keys its parsed document under.
///
/// It has to be stable across frames: `TextView` caches the parse against this id, so a
/// changing id would re-parse the whole changelog on every frame.
const NOTES_VIEW_ID: &str = "update-release-notes";

/// A fixed parser revision, so rebuilding the plugin handles each frame is not mistaken
/// for a changed parser configuration.
const PARSER_REVISION: u64 = 1;

/// Upper bound on the changelog this module will hand to the parser.
///
/// `bongocat-packaging` already bounds the announced notes, but that is the publisher's
/// promise, not this process's guarantee: the manifest arrives over the network, and an
/// oversized notes field would otherwise become an unbounded number of elements to lay
/// out. Truncation happens at a character boundary, so the worst case is a changelog that
/// stops mid-sentence with a visible marker.
const MAXIMUM_MARKDOWN_BYTES: usize = 32 * 1024;

/// Upper bound on the block containers a changelog may open.
///
/// Every level of CommonMark block nesting is opened by at least one container marker in
/// the source, so a document with at most this many markers cannot nest deeper than this
/// many levels. That is what makes the bound sound rather than a guess about how deeply
/// the source *looks* nested. A real changelog spends a handful; a document written to
/// exhaust the stack spends two bytes per level and runs into this within a kilobyte.
const MAXIMUM_BLOCK_MARKERS: usize = 512;

/// What a cut changelog ends with, so a truncated document never reads as complete.
const TRUNCATION_MARKER: &str = "\n\n…";

/// Longest link this module will turn into a control.
const MAXIMUM_LINK_BYTES: usize = 2048;

/// Plugin names, which are also how a claimed node is told apart from another one.
const NO_REMOTE_IMAGE: &str = "bongocat-notes-image-alt-text";
const REFUSED_LINK: &str = "bongocat-notes-refused-link";
const LITERAL_HTML_INLINE: &str = "bongocat-notes-literal-html-inline";
const LITERAL_HTML_BLOCK: &str = "bongocat-notes-literal-html-block";

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

/// The Markdown extensions that keep a release manifest inert.
///
/// Built once. `MarkdownExtensions` stamps every registration with a process-wide
/// revision, so rebuilding this per frame would hand `TextView` a new configuration on
/// every frame and leave it one change of that comparison away from re-parsing the whole
/// changelog sixty times a second. A single registry makes the configuration provably
/// stable instead.
fn extensions() -> MarkdownExtensions {
    static EXTENSIONS: OnceLock<MarkdownExtensions> = OnceLock::new();
    EXTENSIONS
        .get_or_init(|| {
            MarkdownExtensions::default()
                // A fixed parser revision as well, so the registry reads as the same
                // parser even if a future `gpui-kit` starts comparing it.
                .parser_revision(PARSER_REVISION)
                .plugin(NoRemoteImage)
                .plugin(RefusedLink)
                .plugin(LiteralHtml::<false>)
                .plugin(LiteralHtml::<true>)
        })
        .clone()
}

// --- The input bounds, applied before anything parses it ---

/// The changelog, bounded, ready for the parser.
///
/// Both bounds are applied to the raw text. The parser builds its tree with an explicit
/// stack, so it is not the parsing that has to be bounded —it is the shape of the tree
/// that comes out, because the renderer walks that by recursion.
pub(crate) fn prepare(notes: &str) -> String {
    let Some(cut) = cut_offset(notes) else {
        return notes.to_owned();
    };
    let mut boundary = cut;
    while boundary > 0 && !notes.is_char_boundary(boundary) {
        boundary -= 1;
    }
    format!("{}{TRUNCATION_MARKER}", &notes[..boundary])
}

/// Where the notes have to stop being rendered, if anywhere.
fn cut_offset(notes: &str) -> Option<usize> {
    let bytes = notes.as_bytes();
    let mut markers = 0usize;
    for (index, byte) in bytes.iter().enumerate() {
        if index >= MAXIMUM_MARKDOWN_BYTES {
            return Some(index);
        }
        if *byte == b'>' || is_list_marker_start(bytes, index) {
            markers += 1;
            if markers > MAXIMUM_BLOCK_MARKERS {
                return Some(index);
            }
        }
    }
    None
}

/// Whether a list marker starts at `index`.
///
/// Deliberately over-eager: a bullet counts wherever it follows whitespace or starts the
/// text, and so does a digit run closed by `.` or `)`. Over-counting only makes the bound
/// trip earlier, which is the direction a guard has to fail in —the alternative is a
/// document that nests deeper than its marker count suggests.
fn is_list_marker_start(bytes: &[u8], index: usize) -> bool {
    if index > 0 && !bytes[index - 1].is_ascii_whitespace() {
        return false;
    }
    if matches!(bytes[index], b'-' | b'*' | b'+') {
        return true;
    }
    let digits = bytes[index..]
        .iter()
        .take_while(|byte| byte.is_ascii_digit())
        .count();
    digits > 0
        && bytes
            .get(index + digits)
            .is_some_and(|byte| matches!(byte, b'.' | b')'))
}

// --- What a refused image shows ---

/// The alt text to show in place of an image, or `None` to leave the node alone.
///
/// An image with no alt text shows nothing at all, which is the same as what a reader
/// would get from a failed image and does not advertise that one was there.
fn image_alt_text(node: &markdown_ast::Node) -> Option<String> {
    match node {
        markdown_ast::Node::Image(image) => Some(image.alt.clone()),
        // `[alt][ref]` resolves to the same fetch, so it gets the same answer.
        markdown_ast::Node::ImageReference(image) => Some(image.alt.clone()),
        _ => None,
    }
}

/// Claims every image node so its URL is never turned into a request.
///
/// `gpui-kit` renders `![alt](url)` by handing `url` to GPUI's resource loader, which
/// fetches it. A manifest must not be able to make the application issue a request of its
/// choosing, so the node is claimed before the built-in handler builds an `ImageSource`
/// and there is nothing left to fetch.
struct NoRemoteImage;

impl MarkdownPlugin for NoRemoteImage {
    fn name(&self) -> &str {
        NO_REMOTE_IMAGE
    }

    fn parse(
        &self,
        node: &markdown_ast::Node,
        _context: &MarkdownParseContext<'_>,
    ) -> Option<MarkdownNode> {
        // An image with empty alt text still resolves to `Some("")` and is still claimed:
        // declining it would hand the node, and its URL, back to the built-in renderer.
        // A node that is not an image at all has to fall through to the next resolver, so
        // this cannot be written as `unwrap_or_default` —that would swallow the rest of
        // the document's inline content.
        let alt = image_alt_text(node)?;
        Some(MarkdownNode::new(NO_REMOTE_IMAGE, ()).text(alt))
    }
}

// --- What a refused link shows ---

/// The text to show in place of a link this product will not open, or `None` to let
/// `gpui-kit` render the link itself.
fn refused_link_text(node: &markdown_ast::Node) -> Option<String> {
    let markdown_ast::Node::Link(link) = node else {
        return None;
    };
    if clickable_link(&link.url).is_some() {
        // An accepted target is declined here, on purpose: the built-in renderer is
        // better at links than this module is, and it routes clicks through the handler
        // in `render`.
        return None;
    }
    Some(plain_text(node))
}

/// Claims a link whose target [`clickable_link`] refuses, and shows its label as text.
///
/// The point is presentational as much as protective: a `javascript:` or `file:` target
/// must not arrive underlined and accent-coloured, because that is what tells a reader it
/// is something to press. The refusal that matters is still the opener's.
struct RefusedLink;

impl MarkdownPlugin for RefusedLink {
    fn name(&self) -> &str {
        REFUSED_LINK
    }

    fn parse(
        &self,
        node: &markdown_ast::Node,
        _context: &MarkdownParseContext<'_>,
    ) -> Option<MarkdownNode> {
        let text = refused_link_text(node)?;
        Some(MarkdownNode::new(REFUSED_LINK, ()).text(text))
    }
}

// --- What raw HTML shows ---

/// The source to show in place of a raw HTML node, or `None` to leave the node alone.
fn literal_html_text(node: &markdown_ast::Node) -> Option<String> {
    let markdown_ast::Node::Html(html) = node else {
        return None;
    };
    Some(html.value.clone())
}

/// Claims raw HTML and shows the source the author wrote.
///
/// Left alone, `gpui-kit` parses a Markdown HTML node with its HTML parser and lays the
/// result out, which is how a `<strong>` in a manifest becomes bold text and an
/// `<img src>` becomes a request. Claiming the node keeps the markup inert and visible as
/// written: it cannot execute, and dropping it would hide content the author wrote.
///
/// `BLOCK` selects which of the two dispatchers registers the plugin. `gpui-kit` has one
/// for inline content and one for block content, and a plugin belongs to exactly one of
/// them, so a raw HTML node has to be claimed in both positions —a block-level
/// `<div>…</div>` never reaches the inline one.
struct LiteralHtml<const BLOCK: bool>;

impl<const BLOCK: bool> MarkdownPlugin for LiteralHtml<BLOCK> {
    fn name(&self) -> &str {
        if BLOCK {
            LITERAL_HTML_BLOCK
        } else {
            LITERAL_HTML_INLINE
        }
    }

    fn is_block(&self) -> bool {
        BLOCK
    }

    fn parse(
        &self,
        node: &markdown_ast::Node,
        _context: &MarkdownParseContext<'_>,
    ) -> Option<MarkdownNode> {
        let name = self.name();
        let text = literal_html_text(node)?;
        Some(MarkdownNode::new(name, ()).text(text))
    }

    fn render(&self, node: &MarkdownNode, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        // Monospace marks it as "this was literal", which is the whole difference between
        // showing the source and quietly honouring it.
        let mono = cx.theme().mono_font_family.clone();
        div()
            .font_family(mono)
            .text_xs()
            .child(node.as_text().to_string())
    }
}

// --- The shared decisions ---

/// The URL to make clickable, or `None` to render the link as plain text.
///
/// HTTPS only, matching the update transport's own policy: the platform opener refuses
/// anything else, and a release manifest must not be able to hand the user a control that
/// opens a `javascript:`, `file:` or `data:` target. The whitespace check matters because a
/// URL with a newline in it is how a scheme can be smuggled past a naive prefix test.
fn clickable_link(url: &str) -> Option<String> {
    let trimmed = url.trim();
    let acceptable = trimmed.len() <= MAXIMUM_LINK_BYTES
        && trimmed.starts_with("https://")
        && trimmed.len() > "https://".len()
        && !trimmed.chars().any(char::is_whitespace)
        && !trimmed.chars().any(char::is_control);
    acceptable.then(|| trimmed.to_owned())
}

/// The text a node contributes, with every bit of markup resolved away.
///
/// Used to keep the label of a refused link. The label is what the author wrote, and
/// dropping it because the *target* was refused would lose content over a target the
/// product was never going to open anyway.
fn plain_text(node: &markdown_ast::Node) -> String {
    match node {
        markdown_ast::Node::Text(text) => text.value.clone(),
        markdown_ast::Node::InlineCode(code) => code.value.clone(),
        markdown_ast::Node::InlineMath(math) => math.value.clone(),
        markdown_ast::Node::Math(math) => math.value.clone(),
        // CommonMark calls the break inside a paragraph a space, and a link label is a
        // paragraph.
        markdown_ast::Node::Break(_) => " ".to_owned(),
        markdown_ast::Node::Image(image) => image.alt.clone(),
        markdown_ast::Node::ImageReference(image) => image.alt.clone(),
        markdown_ast::Node::FootnoteReference(footnote) => {
            format!(
                "[^{}]",
                footnote.label.as_deref().unwrap_or(&footnote.identifier)
            )
        }
        // Reached only if a caller declined the HTML node; showing the source keeps the
        // text complete either way.
        markdown_ast::Node::Html(html) => html.value.clone(),
        _ => node
            .children()
            .map(|children| children.iter().map(plain_text).collect())
            .unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        LITERAL_HTML_BLOCK, LITERAL_HTML_INLINE, MAXIMUM_BLOCK_MARKERS, MAXIMUM_LINK_BYTES,
        MAXIMUM_MARKDOWN_BYTES, NO_REMOTE_IMAGE, REFUSED_LINK, TRUNCATION_MARKER, clickable_link,
        cut_offset, image_alt_text, is_list_marker_start, literal_html_text, plain_text, prepare,
        refused_link_text,
    };
    use gpui_kit::base::markdown_ast;
    use gpui_kit::{TestAppContext, Window};

    fn image(alt: &str, url: &str) -> markdown_ast::Node {
        markdown_ast::Node::Image(markdown_ast::Image {
            position: None,
            alt: alt.to_owned(),
            url: url.to_owned(),
            title: None,
        })
    }

    fn image_reference(alt: &str) -> markdown_ast::Node {
        markdown_ast::Node::ImageReference(markdown_ast::ImageReference {
            position: None,
            alt: alt.to_owned(),
            reference_kind: markdown_ast::ReferenceKind::Full,
            identifier: "shared".to_owned(),
            label: None,
        })
    }

    fn link(url: &str, label: &str) -> markdown_ast::Node {
        markdown_ast::Node::Link(markdown_ast::Link {
            children: vec![markdown_ast::Node::Text(markdown_ast::Text {
                value: label.to_owned(),
                position: None,
            })],
            position: None,
            url: url.to_owned(),
            title: None,
        })
    }

    fn html(value: &str) -> markdown_ast::Node {
        markdown_ast::Node::Html(markdown_ast::Html {
            value: value.to_owned(),
            position: None,
        })
    }

    fn emphasis(text: &str) -> markdown_ast::Node {
        markdown_ast::Node::Emphasis(markdown_ast::Emphasis {
            children: vec![markdown_ast::Node::Text(markdown_ast::Text {
                value: text.to_owned(),
                position: None,
            })],
            position: None,
        })
    }

    // --- The input bounds ---

    #[test]
    fn an_ordinary_changelog_is_not_cut() {
        let notes = "# BongoCat 2.0.0\n\n## Added\n\n- the overlay stays up\n- `just build` works\n\n> thanks\n";
        assert_eq!(prepare(notes), notes);
        assert_eq!(cut_offset(notes), None);
    }

    #[test]
    fn an_oversized_changelog_is_cut_with_a_visible_marker() {
        let notes = "a".repeat(MAXIMUM_MARKDOWN_BYTES + 1);
        let prepared = prepare(&notes);
        assert!(prepared.ends_with(TRUNCATION_MARKER), "{prepared:?}");
        assert_eq!(
            prepared.len(),
            MAXIMUM_MARKDOWN_BYTES + TRUNCATION_MARKER.len(),
            "the cut has to land exactly on the byte budget"
        );
    }

    #[test]
    fn a_cut_lands_on_a_character_boundary() {
        // Each `é` is two bytes, so a budget that lands mid-character would panic on a
        // slice if the boundary were not walked back.
        let notes = "é".repeat(MAXIMUM_MARKDOWN_BYTES);
        let prepared = prepare(&notes);
        assert!(prepared.ends_with(TRUNCATION_MARKER), "{prepared:?}");
        assert!(prepared.starts_with('é'));
    }

    /// Deeply nested quotes are the cheapest way to make a renderer recurse.
    ///
    /// Two bytes buy one level, so a document that nests thousands deep fits in a
    /// kilobyte. The bound is on the number of containers rather than on the apparent
    /// depth, which is what makes it hold for a document written to look shallow.
    #[test]
    fn nesting_deeper_than_the_stack_allows_is_cut() {
        let notes = format!("{}deep\n", "> ".repeat(MAXIMUM_BLOCK_MARKERS + 1));
        let prepared = prepare(&notes);
        assert!(prepared.ends_with(TRUNCATION_MARKER), "{prepared:?}");
        assert!(
            prepared.len() < notes.len(),
            "a document this nested must be cut, not rendered"
        );
    }

    /// A real changelog has a long list in it, and none of that is nesting.
    #[test]
    fn a_long_list_is_not_mistaken_for_nesting() {
        let notes = (0..MAXIMUM_BLOCK_MARKERS)
            .map(|index| format!("- entry {index}\n"))
            .collect::<String>();
        assert_eq!(cut_offset(&notes), None);
    }

    #[test]
    fn a_bullet_is_recognised_wherever_one_could_start() {
        let bytes = b"- a * b 1. c 2) d";
        let found: Vec<usize> = (0..bytes.len())
            .filter(|index| is_list_marker_start(bytes, *index))
            .collect();
        // `-`, `*`, `1.`, `2)`. A `-` or `*` mid-word is not a marker.
        assert_eq!(found, vec![0, 4, 8, 13]);
    }

    // --- Images ---

    /// The refusal has to cover both spellings, because both fetch.
    #[test]
    fn every_image_spelling_yields_its_alt_text_and_nothing_else() {
        for node in [
            image("a diagram", "https://example.com/tracker.gif"),
            image_reference("a diagram"),
        ] {
            assert_eq!(
                image_alt_text(&node),
                Some("a diagram".to_owned()),
                "{node:?}"
            );
        }
    }

    /// An image with no alt text is still an image, and still must not be fetched.
    #[test]
    fn an_image_without_alt_text_is_still_claimed() {
        assert_eq!(
            image_alt_text(&image("", "https://example.com/x.png")),
            Some(String::new())
        );
    }

    #[test]
    fn a_node_that_is_not_an_image_is_left_to_the_built_in_renderer() {
        for node in [
            link("https://example.com", "x"),
            html("<b>x</b>"),
            emphasis("x"),
        ] {
            assert_eq!(image_alt_text(&node), None, "{node:?}");
        }
    }

    // --- Links ---

    /// HTTPS, and only HTTPS, becomes a control.
    #[test]
    fn only_https_links_are_clickable() {
        assert_eq!(
            clickable_link("https://example.com/issues/47"),
            Some("https://example.com/issues/47".to_owned())
        );
        for refused in [
            "javascript:alert(1)",
            "file:///etc/passwd",
            "data:text/html,<script>",
            "http://example.com",
            // A newline is how a scheme gets smuggled past a naive prefix test.
            "https://example.com/\njavascript:alert(1)",
            "https://example.com/ with a space",
            "https://",
            "",
        ] {
            assert_eq!(clickable_link(refused), None, "{refused:?}");
        }
    }

    #[test]
    fn an_over_long_link_target_is_refused() {
        let refused = format!("https://example.com/{}", "a".repeat(MAXIMUM_LINK_BYTES));
        assert_eq!(clickable_link(&refused), None);
    }

    /// The plugin declines an accepted link, so `gpui-kit` renders it.
    #[test]
    fn an_accepted_link_is_left_to_the_built_in_renderer() {
        assert_eq!(
            refused_link_text(&link("https://example.com", "the issue")),
            None
        );
    }

    #[test]
    fn a_refused_link_keeps_its_label() {
        assert_eq!(
            refused_link_text(&link("javascript:alert(1)", "click me")),
            Some("click me".to_owned())
        );
    }

    #[test]
    fn a_refused_links_label_keeps_its_own_markup_resolved_away() {
        let node = markdown_ast::Node::Link(markdown_ast::Link {
            children: vec![
                markdown_ast::Node::Text(markdown_ast::Text {
                    value: "see ".to_owned(),
                    position: None,
                }),
                emphasis("the issue"),
                markdown_ast::Node::InlineCode(markdown_ast::InlineCode {
                    value: "#47".to_owned(),
                    position: None,
                }),
            ],
            position: None,
            url: "javascript:alert(1)".to_owned(),
            title: None,
        });
        assert_eq!(
            refused_link_text(&node),
            Some("see the issue#47".to_owned())
        );
    }

    #[test]
    fn a_link_reference_is_left_to_the_built_in_renderer() {
        // A reference resolves to its target inside `gpui-kit`, so this module cannot judge
        // it here. What still holds is that the click goes through the opener, which
        // re-checks the scheme.
        let node = markdown_ast::Node::LinkReference(markdown_ast::LinkReference {
            children: vec![markdown_ast::Node::Text(markdown_ast::Text {
                value: "x".to_owned(),
                position: None,
            })],
            position: None,
            reference_kind: markdown_ast::ReferenceKind::Full,
            identifier: "shared".to_owned(),
            label: None,
        });
        assert_eq!(refused_link_text(&node), None);
    }

    #[test]
    fn plain_text_keeps_a_footnotes_label_and_an_images_alt() {
        let footnote = markdown_ast::Node::FootnoteReference(markdown_ast::FootnoteReference {
            position: None,
            identifier: "note".to_owned(),
            label: Some("Note".to_owned()),
        });
        assert_eq!(plain_text(&footnote), "[^Note]");
        assert_eq!(
            plain_text(&image("alt", "https://example.com/x.png")),
            "alt"
        );
    }

    // --- Raw HTML ---

    #[test]
    fn raw_html_yields_its_source() {
        assert_eq!(
            literal_html_text(&html("<script>alert(1)</script>")),
            Some("<script>alert(1)</script>".to_owned())
        );
    }

    #[test]
    fn a_node_that_is_not_html_is_left_alone() {
        for node in [
            image("a", "b"),
            link("https://example.com", "x"),
            emphasis("x"),
        ] {
            assert_eq!(literal_html_text(&node), None, "{node:?}");
        }
    }

    /// Both positions need a plugin, and a plugin belongs to exactly one of them.
    ///
    /// A block-level `<div>…</div>` never reaches the inline dispatcher, so registering
    /// only one of the two would leave the other reading raw HTML as markup.
    #[test]
    fn the_two_html_plugins_hold_the_two_positions_apart() {
        use gpui_kit::base::MarkdownPlugin;

        assert!(!super::LiteralHtml::<false>.is_block());
        assert!(super::LiteralHtml::<true>.is_block());
        assert_eq!(super::LiteralHtml::<false>.name(), LITERAL_HTML_INLINE);
        assert_eq!(super::LiteralHtml::<true>.name(), LITERAL_HTML_BLOCK);
        assert_ne!(LITERAL_HTML_INLINE, LITERAL_HTML_BLOCK);
        // Every plugin claims its nodes under its own name, so a claimed node can never be
        // rendered by the wrong plugin's renderer.
        assert_eq!(super::NoRemoteImage.name(), NO_REMOTE_IMAGE);
        assert_eq!(super::RefusedLink.name(), REFUSED_LINK);
    }

    // --- The claim the whole module rests on ---

    /// A refusal has to refuse *only* the thing it names.
    ///
    /// Two things are checked here, and the second is the one that catches the mistake
    /// this test was written after. A plugin that claims a node it was not meant to claim
    /// —by treating "not an image" as "an image with no alt text" —replaces the whole
    /// document's inline content with a single empty string, and the changelog window
    /// still lays out, still has height, and still passes a test that only asks whether
    /// anything rendered. So the rendered text is read back and compared.
    ///
    /// The three properties are: every URL the manifest named is gone from the rendered
    /// text, the alt text and link labels that replace them are still there, and a probe
    /// registered after the production plugins never saw an image or an HTML node —which
    /// is what shows they were claimed rather than merely not-rendered.
    #[gpui_kit::test]
    fn the_refusals_claim_their_nodes_before_the_built_in_renderer(cx: &mut TestAppContext) {
        use gpui_kit::base::{
            MarkdownExtensions, MarkdownNode, MarkdownParseContext, MarkdownPlugin, TextView,
            TextViewState,
        };
        use gpui_kit::test::TestWindowExt;
        use gpui_kit::{AppContext, Context, Entity, IntoElement, Render, px, size};
        use std::sync::{Arc, Mutex};

        /// Records the kind of every node `gpui-kit` still offers, claiming nothing.
        struct Probe<const BLOCK: bool>(Arc<Mutex<Vec<String>>>);

        impl<const BLOCK: bool> MarkdownPlugin for Probe<BLOCK> {
            fn name(&self) -> &str {
                "bongocat-notes-probe"
            }

            fn is_block(&self) -> bool {
                BLOCK
            }

            fn parse(
                &self,
                node: &markdown_ast::Node,
                _context: &MarkdownParseContext<'_>,
            ) -> Option<MarkdownNode> {
                self.0
                    .lock()
                    .expect("the probe recorder is not poisoned")
                    .push(format!(
                        "{}:{}",
                        if BLOCK { "BLOCK" } else { "INLINE" },
                        kind_of(node)
                    ));
                // Never claims: letting the next resolver have it is what makes this a
                // record of what the refusals left behind.
                None
            }
        }

        fn kind_of(node: &markdown_ast::Node) -> String {
            match node {
                markdown_ast::Node::Image(_) => "image".to_owned(),
                markdown_ast::Node::ImageReference(_) => "image-reference".to_owned(),
                markdown_ast::Node::Link(_) => "link".to_owned(),
                markdown_ast::Node::Html(_) => "html".to_owned(),
                markdown_ast::Node::Text(_) => "text".to_owned(),
                other => {
                    let rendered = format!("{other:?}");
                    format!("other:{}", &rendered[..rendered.len().min(48)])
                }
            }
        }

        struct Notes {
            state: Entity<TextViewState>,
            extensions: MarkdownExtensions,
        }

        impl Render for Notes {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                TextView::new(&self.state).markdown_extensions(self.extensions.clone())
            }
        }

        let source = "\
![a diagram](https://example.com/tracker.gif)

![a referenced diagram][shared]

<div><img src=\"https://example.com/pixel.png\"></div>

an [ok link](https://example.com/page) and a [bad one](javascript:alert(1))

[shared]: https://example.com/shared.png
";
        let seen: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let extensions = super::extensions()
            .plugin(Probe::<false>(Arc::clone(&seen)))
            .plugin(Probe::<true>(Arc::clone(&seen)));

        cx.update(gpui_kit::init);
        let state = cx.new(|cx| TextViewState::markdown(source, cx));
        let mounted = state.clone();
        let handle = cx.open_window(size(px(560.0), px(460.0)), move |_, _cx| Notes {
            state: mounted.clone(),
            extensions: extensions.clone(),
        });
        for _ in 0..2 {
            cx.update_window(handle.into(), |_, window, cx| {
                window.render_frame(cx);
            })
            .expect("the notes window stays open");
            cx.run_until_parked();
        }

        let offered = seen
            .lock()
            .expect("the probe recorder is not poisoned")
            .clone();
        // The control: the probe ran at all, and it ran inline as well as block.
        assert!(
            offered.iter().any(|kind| kind == "INLINE:text"),
            "the probe never saw inline content, so it proves nothing: {offered:?}"
        );
        assert!(
            offered.iter().any(|kind| kind == "INLINE:link"),
            "the probe never saw a link, so it proves nothing: {offered:?}"
        );
        for claimed in ["image", "image-reference", "html"] {
            assert!(
                !offered.iter().any(|kind| kind.ends_with(claimed)),
                "a {claimed} node reached the renderer behind the refusal that exists to \
                 stop it: {offered:?}"
            );
        }

        // What the window ended up showing. A refusal that also ate the surrounding
        // content would pass every check above.
        let rendered = cx
            .update_window(handle.into(), |_, _window, cx| {
                cx.update_entity(&state, |state, cx| state.select_all(cx));
                state.read_with(cx, |state, _| state.selected_text())
            })
            .expect("the notes window stays open");
        for shown in [
            "a diagram",
            "a referenced diagram",
            // Raw HTML is shown as written, which is what keeps it inert and visible.
            "<div><img src=\"https://example.com/pixel.png\"></div>",
            "ok link",
            "bad one",
        ] {
            assert!(
                rendered.contains(shown),
                "{shown:?} is missing from the rendered changelog: {rendered:?}"
            );
        }
        for gone in ["tracker.gif", "shared.png", "example.com/page"] {
            assert!(
                !rendered.contains(gone),
                "{gone:?} reached the rendered changelog: {rendered:?}"
            );
        }
        // A URL inside raw HTML is the one that legitimately survives, as text: showing
        // the author what they wrote is the whole point of refusing to interpret it. It is
        // inert either way —a string in the changelog is not a request —and the
        // assertion above is about the *image* URLs, which are dropped rather than shown.
        assert!(
            rendered.contains("<div><img src=\"https://example.com/pixel.png\"></div>"),
            "raw HTML must be shown as written: {rendered:?}"
        );
    }
}
