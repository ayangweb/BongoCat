//! A node's text, with its markup resolved away.

use super::*;

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
