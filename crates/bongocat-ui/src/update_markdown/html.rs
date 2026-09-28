//! Raw HTML is shown as its source rather than interpreted.
//!
//! The notes are the only place the product renders text it did not author, so
//! this is the line that keeps them text. Two instances rather than one, because
//! CommonMark puts an inline HTML run and a block HTML run in different places and
//! a plugin that claimed both would be claiming a position it does not have.

use super::*;

pub(crate) const LITERAL_HTML_INLINE: &str = "bongocat-notes-literal-html-inline";

pub(crate) const LITERAL_HTML_BLOCK: &str = "bongocat-notes-literal-html-block";

// --- What raw HTML shows ---

/// The source to show in place of a raw HTML node, or `None` to leave the node alone.
pub(crate) fn literal_html_text(node: &markdown_ast::Node) -> Option<String> {
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
pub(crate) struct LiteralHtml<const BLOCK: bool>;

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
