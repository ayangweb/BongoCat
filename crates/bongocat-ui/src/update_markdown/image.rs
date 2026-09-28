//! A release manifest may not make the window fetch anything.
//!
//! The alt text is what is left in its place, and it is kept for two reasons: it
//! is the one part of the image the author wrote deliberately, and it is already
//! in the notes rather than being another thing to fetch. An image with no alt
//! text is still claimed — the node is still an image, and leaving it to the
//! built-in renderer would let it load.

use super::*;

/// Plugin names, which are also how a claimed node is told apart from another one.
pub(crate) const NO_REMOTE_IMAGE: &str = "bongocat-notes-image-alt-text";

// --- What a refused image shows ---

/// The alt text to show in place of an image, or `None` to leave the node alone.
///
/// An image with no alt text shows nothing at all, which is the same as what a reader
/// would get from a failed image and does not advertise that one was there.
pub(crate) fn image_alt_text(node: &markdown_ast::Node) -> Option<String> {
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
pub(crate) struct NoRemoteImage;

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
