//! The notes renderer's tests, split by the section they cover.
//!
//! The fixtures are here rather than in one of the files because every section
//! needs them: an image, a link and an HTML run are all built the same way, and a
//! test that compares one against the built-in renderer needs the same nodes.

use super::*;

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

mod bounds;
mod html;
mod image;
mod link;
mod text;
