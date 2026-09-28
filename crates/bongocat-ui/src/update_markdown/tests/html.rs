//! Raw HTML is shown as its source.

use super::*;

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
