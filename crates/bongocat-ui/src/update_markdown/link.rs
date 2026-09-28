//! A link is clickable only if the platform would open it.
//!
//! HTTPS and nothing else, matching the update transport's own policy. The
//! whitespace check is not tidiness: a URL with a newline in it is how a scheme
//! gets smuggled past a parser that only looks at the first five characters. The
//! label survives a refused target, because dropping it would lose content over a
//! URL the product was never going to open anyway — the notes are text first and
//! links second.

use super::*;

/// Longest link this module will turn into a control.
pub(crate) const MAXIMUM_LINK_BYTES: usize = 2048;

pub(crate) const REFUSED_LINK: &str = "bongocat-notes-refused-link";

// --- What a refused link shows ---

/// The text to show in place of a link this product will not open, or `None` to let
/// `gpui-kit` render the link itself.
pub(crate) fn refused_link_text(node: &markdown_ast::Node) -> Option<String> {
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
pub(crate) struct RefusedLink;

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
