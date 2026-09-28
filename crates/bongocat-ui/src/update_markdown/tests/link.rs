//! A link is clickable only if the platform would open it.

use super::*;

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
