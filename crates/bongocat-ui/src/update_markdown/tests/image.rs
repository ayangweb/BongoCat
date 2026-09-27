//! An image shows its alt text and nothing else.

use super::*;

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
