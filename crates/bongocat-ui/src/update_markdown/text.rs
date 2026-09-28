//! What two of the refusals need and neither owns.
//!
//! The URL test decides whether a link is clickable, and the text walker resolves
//! a node's markup away so a refused link can keep its label. Neither belongs to
//! the image or the link refusal: both are asked for by a decision made in one
//! place and used in another.

use super::*;

// --- The shared decisions ---

/// The URL to make clickable, or `None` to render the link as plain text.
///
/// HTTPS only, matching the update transport's own policy: the platform opener refuses
/// anything else, and a release manifest must not be able to hand the user a control that
/// opens a `javascript:`, `file:` or `data:` target. The whitespace check matters because a
/// URL with a newline in it is how a scheme can be smuggled past a naive prefix test.
pub(crate) fn clickable_link(url: &str) -> Option<String> {
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
pub(crate) fn plain_text(node: &markdown_ast::Node) -> String {
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
