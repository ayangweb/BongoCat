//! Markdown rendering for the update window's release notes.
//!
//! The changelog comes from the release manifest, so it is **untrusted input**: it
//! travels over the network and whoever can replace the manifest controls every byte of
//! it. It is also the only place the product renders text it did not author. The
//! renderer is therefore split in two on purpose:
//!
//! 1. [`blocks`] parses the Markdown into a small intermediate representation. It is a
//!    pure function with no GPUI types, so every syntax decision and every safety rule
//!    is unit-testable.
//! 2. [`render`] turns that representation into GPUI elements. It never sees Markdown,
//!    so it cannot accidentally interpret it.
//!
//! Nothing here produces markup of any kind — GPUI has no HTML engine, and raw HTML in
//! the notes is rendered as the literal text the author wrote. There is no escaping
//! step to get wrong because there is no markup layer to escape into.
//!
//! # What is deliberately not supported
//!
//! - **Tables.** A GFM table degrades to the pipe-separated paragraphs CommonMark sees,
//!   which is ugly but honest. Rendering one properly needs column measurement, and
//!   release notes essentially never contain one.
//! - **Images.** `![alt](url)` renders its alt text. The URL is never fetched: an
//!   update manifest must not be able to make the application issue a request of its
//!   choosing.
//! - **Raw HTML.** Shown as literal text. It cannot execute, and silently dropping it
//!   would hide content the author wrote.

use gpui_kit::base::TestSupportExt;
use gpui_kit::component::ActiveTheme;
use gpui_kit::{
    AnyElement, App, Div, FontWeight, SharedString, StatefulInteractiveElement, div, prelude::*, px,
};
use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};

use crate::window::Tokens;

/// Upper bound on the changelog this module will parse.
///
/// `crates/bongocat-packaging` already bounds the announced notes, but that is the
/// publisher's promise, not this process's guarantee: the manifest arrives over the
/// network, and an oversized notes field would otherwise become an unbounded number of
/// GPUI elements and hang the window. Truncation happens at a character boundary, so
/// the worst case is a changelog that stops mid-sentence with a visible marker.
const MAXIMUM_MARKDOWN_BYTES: usize = 32 * 1024;
const TRUNCATION_MARKER: &str = "\n\n…";

/// How deep block nesting is rendered before it is flattened.
///
/// Deeply nested quotes and lists are legal Markdown and would otherwise let a manifest
/// dictate an arbitrarily deep layout. Past this depth the content is kept and the
/// nesting is dropped.
const MAXIMUM_BLOCK_DEPTH: usize = 8;

/// Longest link this module will turn into a control.
const MAXIMUM_LINK_BYTES: usize = 2048;

/// The inline styles a span can carry.
///
/// A link span keeps the URL only when [`clickable_link`] accepted it; a rejected URL
/// leaves `link` as `None` and the span renders as plain text, so a `javascript:` or
/// `file:` target can never become an interactive control.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct SpanStyle {
    pub(crate) bold: bool,
    pub(crate) italic: bool,
    pub(crate) strikethrough: bool,
    pub(crate) code: bool,
    pub(crate) link: Option<String>,
}

/// One piece of inline content.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Inline {
    Text {
        text: String,
        style: SpanStyle,
    },
    /// A line break. Rendered as a full-width element so the next content starts on a
    /// new line without ending the block.
    Break,
}

/// One block of content.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Block {
    Heading {
        level: u8,
        spans: Vec<Inline>,
    },
    Paragraph {
        spans: Vec<Inline>,
    },
    /// A fenced or indented code block. Its text is kept verbatim.
    Code {
        text: String,
    },
    List {
        ordered: bool,
        /// The number the first item is labelled with.
        start: u64,
        items: Vec<Vec<Block>>,
    },
    Quote {
        blocks: Vec<Block>,
    },
    Rule,
}

/// Parse Markdown into blocks.
///
/// Pure: no GPUI types, no theme, no I/O. Every rule this module promises — which
/// syntax is recognised, what happens to raw HTML, which links become clickable, how
/// deep nesting is handled — is decided here and testable without a window.
pub(crate) fn blocks(markdown: &str) -> Vec<Block> {
    let text = truncate(markdown);
    let options = Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    let events: Vec<Event<'_>> = Parser::new_ext(&text, options).collect();
    let mut reader = Reader {
        events,
        index: 0,
        pending_marker: None,
    };
    reader.blocks(0).0
}

/// Shorten the input at a character boundary.
fn truncate(markdown: &str) -> String {
    if markdown.len() <= MAXIMUM_MARKDOWN_BYTES {
        return markdown.to_owned();
    }
    let mut boundary = MAXIMUM_MARKDOWN_BYTES;
    while boundary > 0 && !markdown.is_char_boundary(boundary) {
        boundary -= 1;
    }
    format!("{}{TRUNCATION_MARKER}", &markdown[..boundary])
}

/// The URL to make clickable, or `None` to render the link as plain text.
///
/// HTTPS only, matching the update transport's own policy (`AGENTS.md` §10): the
/// platform opener refuses anything else, and a release manifest must not be able to
/// hand the user a control that opens a `javascript:`, `file:` or `data:` target. The
/// whitespace check matters because a URL with a newline in it is how a scheme can be
/// smuggled past a naive prefix test.
fn clickable_link(url: &str) -> Option<String> {
    let trimmed = url.trim();
    let acceptable = trimmed.len() <= MAXIMUM_LINK_BYTES
        && trimmed.starts_with("https://")
        && trimmed.len() > "https://".len()
        && !trimmed.chars().any(char::is_whitespace)
        && !trimmed.chars().any(char::is_control);
    acceptable.then(|| trimmed.to_owned())
}

/// Append text, merging into the previous span when the style is unchanged.
///
/// The parser emits a `Text` event per run rather than per paragraph, so merging keeps
/// the rendered element count proportional to the number of style changes instead of
/// the number of parser events.
fn push_text(spans: &mut Vec<Inline>, text: &str, style: &SpanStyle) {
    if text.is_empty() {
        return;
    }
    if let Some(Inline::Text {
        text: previous,
        style: previous_style,
    }) = spans.last_mut()
        && previous_style == style
    {
        previous.push_str(text);
        return;
    }
    spans.push(Inline::Text {
        text: text.to_owned(),
        style: style.clone(),
    });
}

/// A cursor over the parser's events.
///
/// The event stream is a flat sequence of start/end tags; a cursor with recursion is far
/// easier to keep correct than a hand-rolled container stack, and the recursion depth is
/// bounded by [`MAXIMUM_BLOCK_DEPTH`].
struct Reader<'a> {
    events: Vec<Event<'a>>,
    index: usize,
    /// A task-list marker seen before the paragraph it labels.
    ///
    /// The parser emits `TaskListMarker` directly inside the item, ahead of the item's
    /// paragraph, so it cannot be handled while reading inline content.
    pending_marker: Option<bool>,
}

impl<'a> Reader<'a> {
    fn next(&mut self) -> Option<Event<'a>> {
        let event = self.events.get(self.index).cloned();
        if event.is_some() {
            self.index += 1;
        }
        event
    }

    /// Read blocks until the stream ends or the block this call started in closes.
    ///
    /// Returns the blocks and the tag that ended them, so the caller can tell "my
    /// container closed" from "input ended".
    ///
    /// Inline content is buffered rather than turned into a paragraph as it arrives,
    /// because a **tight** list item is not wrapped in a `Paragraph` by the parser: its
    /// text, emphasis and links arrive loose inside the item. Buffering them means they
    /// keep their styles and end up in one paragraph instead of one per run.
    fn blocks(&mut self, depth: usize) -> (Vec<Block>, Option<TagEnd>) {
        let mut blocks = Vec::new();
        let mut pending: Vec<Inline> = Vec::new();
        // Past the depth cap the containers are transparent: their content is kept, the
        // nesting is not, so nothing a manifest writes disappears.
        let flatten = depth > MAXIMUM_BLOCK_DEPTH;
        while let Some(event) = self.next() {
            match event {
                Event::Start(tag) => {
                    if is_inline_tag(&tag) {
                        let (inner, _) = self.inline(inline_style(&tag, &SpanStyle::default()));
                        pending.extend(inner);
                    } else {
                        self.flush_paragraph(&mut pending, &mut blocks);
                        self.block(tag, depth, flatten, &mut blocks, &mut pending);
                    }
                }
                Event::End(tag) => {
                    self.flush_paragraph(&mut pending, &mut blocks);
                    return (blocks, Some(tag));
                }
                Event::Text(text) => push_text(&mut pending, &text, &SpanStyle::default()),
                Event::Code(text) => {
                    let style = SpanStyle {
                        code: true,
                        ..SpanStyle::default()
                    };
                    push_text(&mut pending, &text, &style);
                }
                // Raw HTML is content the author wrote, so it is shown as text. It is
                // never parsed, never fetched and never interpreted.
                Event::Html(html) | Event::InlineHtml(html) => {
                    push_text(&mut pending, &html, &literal_style());
                }
                Event::SoftBreak | Event::HardBreak => pending.push(Inline::Break),
                Event::TaskListMarker(checked) => self.pending_marker = Some(checked),
                Event::Rule => {
                    self.flush_paragraph(&mut pending, &mut blocks);
                    blocks.push(Block::Rule);
                }
                Event::FootnoteReference(name) => {
                    push_text(&mut pending, &format!("[^{name}]"), &SpanStyle::default());
                }
                Event::InlineMath(text) | Event::DisplayMath(text) => {
                    let style = SpanStyle {
                        code: true,
                        ..SpanStyle::default()
                    };
                    push_text(&mut pending, &text, &style);
                }
            }
        }
        self.flush_paragraph(&mut pending, &mut blocks);
        (blocks, None)
    }

    /// Turn buffered inline content into a paragraph.
    fn flush_paragraph(&mut self, pending: &mut Vec<Inline>, blocks: &mut Vec<Block>) {
        if pending.is_empty() {
            // A task-list marker with no paragraph yet belongs to the next one.
            return;
        }
        let spans = std::mem::take(pending);
        self.paragraph(spans, blocks);
    }

    fn block(
        &mut self,
        tag: Tag<'a>,
        depth: usize,
        flatten: bool,
        blocks: &mut Vec<Block>,
        pending: &mut Vec<Inline>,
    ) {
        match tag {
            Tag::Paragraph => {
                let (spans, _) = self.inline(SpanStyle::default());
                self.paragraph(spans, blocks);
            }
            // Inline tags cannot open a block; if one appears here the content is still
            // worth keeping, so it joins whatever paragraph is being built.
            Tag::Emphasis
            | Tag::Strong
            | Tag::Strikethrough
            | Tag::Superscript
            | Tag::Subscript
            | Tag::Link { .. }
            | Tag::Image { .. } => {
                let (inner, _) = self.inline(inline_style(&tag, &SpanStyle::default()));
                pending.extend(inner);
            }
            Tag::Heading { level, .. } => {
                let (spans, _) = self.inline(SpanStyle::default());
                blocks.push(Block::Heading {
                    level: heading_level(level),
                    spans,
                });
            }
            Tag::CodeBlock(_) => blocks.push(Block::Code {
                text: self.code_block(),
            }),
            Tag::List(start) => {
                let (items, _) = self.list_items(depth, flatten);
                if flatten {
                    // Past the depth cap the items are kept and the nesting is dropped,
                    // so a manifest cannot dictate an arbitrarily deep layout.
                    blocks.extend(items.into_iter().flatten());
                } else {
                    blocks.push(Block::List {
                        ordered: start.is_some(),
                        start: start.unwrap_or(1),
                        items,
                    });
                }
            }
            Tag::BlockQuote(_) => {
                let (quoted, _) = self.blocks(depth + 1);
                if flatten {
                    blocks.extend(quoted);
                } else {
                    blocks.push(Block::Quote { blocks: quoted });
                }
            }
            // Raw HTML is content the author wrote, so it is shown as text. It is never
            // parsed, never fetched and never interpreted.
            Tag::HtmlBlock => {
                let spans = vec![Inline::Text {
                    text: self.html_block(),
                    style: literal_style(),
                }];
                self.paragraph(spans, blocks);
            }
            // Containers this module does not enable or does not model: descend so their
            // content still renders, without adding a block of its own.
            Tag::FootnoteDefinition(_)
            | Tag::DefinitionList
            | Tag::DefinitionListTitle
            | Tag::DefinitionListDefinition
            | Tag::Table(_)
            | Tag::TableHead
            | Tag::TableRow
            | Tag::TableCell
            | Tag::MetadataBlock(_)
            | Tag::Item => {
                let (nested, _) = self.blocks(depth + 1);
                blocks.extend(nested);
            }
        }
    }

    /// Push a paragraph, applying a task-list marker the item declared.
    ///
    /// A tight list item is not wrapped in a `Paragraph` by the parser, so the marker
    /// can arrive before loose text rather than before a paragraph tag. Applying it here
    /// covers both shapes.
    fn paragraph(&mut self, mut spans: Vec<Inline>, blocks: &mut Vec<Block>) {
        if let Some(checked) = self.pending_marker.take() {
            spans.insert(
                0,
                Inline::Text {
                    text: if checked { "☑ " } else { "☐ " }.to_owned(),
                    style: SpanStyle::default(),
                },
            );
        }
        blocks.push(Block::Paragraph { spans });
    }

    /// Read one list's items.
    fn list_items(&mut self, depth: usize, flatten: bool) -> (Vec<Vec<Block>>, Option<TagEnd>) {
        let mut items = Vec::new();
        while let Some(event) = self.next() {
            match event {
                Event::Start(Tag::Item) => {
                    let (item, _) = self.blocks(depth + 1);
                    items.push(item);
                }
                Event::End(tag) => return (items, Some(tag)),
                Event::Start(tag) => {
                    // A list containing anything but items is malformed; keep the content.
                    let mut stray = Vec::new();
                    let mut stray_pending = Vec::new();
                    self.block(tag, depth, flatten, &mut stray, &mut stray_pending);
                    self.flush_paragraph(&mut stray_pending, &mut stray);
                    items.push(stray);
                }
                _ => {}
            }
        }
        (items, None)
    }

    /// Read a code block's text verbatim.
    fn code_block(&mut self) -> String {
        let mut text = String::new();
        while let Some(event) = self.next() {
            match event {
                Event::Text(part) => text.push_str(&part),
                Event::Code(part) => text.push_str(&part),
                Event::SoftBreak | Event::HardBreak => text.push('\n'),
                Event::End(TagEnd::CodeBlock) => break,
                _ => {}
            }
        }
        text
    }

    /// Read an HTML block's text verbatim.
    fn html_block(&mut self) -> String {
        let mut text = String::new();
        while let Some(event) = self.next() {
            match event {
                Event::Html(part) | Event::Text(part) | Event::InlineHtml(part) => {
                    text.push_str(&part);
                }
                Event::End(TagEnd::HtmlBlock) => break,
                _ => {}
            }
        }
        text.trim_end_matches('\n').to_owned()
    }

    /// Read inline content until the current inline container closes.
    fn inline(&mut self, style: SpanStyle) -> (Vec<Inline>, Option<TagEnd>) {
        let mut spans = Vec::new();
        while let Some(event) = self.next() {
            match event {
                Event::Start(tag) => self.inline_tag(tag, &style, &mut spans),
                Event::End(tag) => return (spans, Some(tag)),
                Event::Text(text) => push_text(&mut spans, &text, &style),
                Event::Code(text) => {
                    let mut code_style = style.clone();
                    code_style.code = true;
                    push_text(&mut spans, &text, &code_style);
                }
                // A line break inside a paragraph. CommonMark calls a soft break a
                // space, but release notes are written with intentional line breaks and
                // GitHub's own comment rendering keeps them, so they are kept here.
                Event::SoftBreak | Event::HardBreak => spans.push(Inline::Break),
                Event::InlineHtml(html) | Event::Html(html) => {
                    push_text(&mut spans, &html, &literal_style());
                }
                Event::FootnoteReference(name) => {
                    push_text(&mut spans, &format!("[^{name}]"), &style);
                }
                Event::InlineMath(text) | Event::DisplayMath(text) => {
                    let mut math_style = style.clone();
                    math_style.code = true;
                    push_text(&mut spans, &text, &math_style);
                }
                Event::TaskListMarker(checked) => {
                    push_text(&mut spans, if checked { "☑ " } else { "☐ " }, &style);
                }
                Event::Rule => {}
            }
        }
        (spans, None)
    }

    fn inline_tag(&mut self, tag: Tag<'a>, style: &SpanStyle, spans: &mut Vec<Inline>) {
        let (inner, _) = self.inline(inline_style(&tag, style));
        spans.extend(inner);
    }
}

/// Whether a tag contributes inline styling rather than opening a block.
fn is_inline_tag(tag: &Tag<'_>) -> bool {
    matches!(
        tag,
        Tag::Emphasis
            | Tag::Strong
            | Tag::Strikethrough
            | Tag::Superscript
            | Tag::Subscript
            | Tag::Link { .. }
            | Tag::Image { .. }
    )
}

/// The style an inline tag adds on top of `base`.
fn inline_style(tag: &Tag<'_>, base: &SpanStyle) -> SpanStyle {
    match tag {
        Tag::Strong => SpanStyle {
            bold: true,
            ..base.clone()
        },
        Tag::Emphasis => SpanStyle {
            italic: true,
            ..base.clone()
        },
        Tag::Strikethrough => SpanStyle {
            strikethrough: true,
            ..base.clone()
        },
        // An image's alt text renders; its URL does not.
        Tag::Link { dest_url, .. } => SpanStyle {
            link: clickable_link(dest_url),
            ..base.clone()
        },
        _ => base.clone(),
    }
}

/// The style for text that is shown as-is because it is not Markdown this module
/// renders — raw HTML, for instance. Monospace marks it as "this was literal".
fn literal_style() -> SpanStyle {
    SpanStyle {
        code: true,
        ..SpanStyle::default()
    }
}

fn heading_level(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

/// What the renderer needs beyond the parsed blocks.
struct RenderState {
    tokens: Tokens,
    mono: SharedString,
    /// Counts links so each gets a distinct element id.
    ///
    /// A link needs an id to be clickable, and two identical URLs in one changelog would
    /// otherwise share one. The index follows render order, so it is stable for as long
    /// as the content is.
    link_index: usize,
}

impl RenderState {
    fn link_id(&mut self, url: &str) -> SharedString {
        let id = SharedString::from(format!("update-notes-link:{}:{url}", self.link_index));
        self.link_index += 1;
        id
    }
}

/// Render parsed blocks into a column.
pub(crate) fn render(blocks: &[Block], tokens: Tokens, cx: &App) -> Div {
    let mut state = RenderState {
        tokens,
        mono: cx.theme().mono_font_family.clone(),
        link_index: 0,
    };
    let mut column = div().flex().flex_col().gap_2().w_full().min_w_0();
    for block in blocks {
        column = column.child(render_block(block, &mut state));
    }
    column
}

fn render_block(block: &Block, state: &mut RenderState) -> Div {
    let tokens = state.tokens;
    match block {
        Block::Heading { level, spans } => {
            let row = inline_row(spans, state);
            let row = match level {
                1 => row.text_lg(),
                2 => row.text_base(),
                _ => row.text_sm(),
            };
            row.font_weight(FontWeight::BOLD).text_color(tokens.text)
        }
        Block::Paragraph { spans } => inline_row(spans, state).text_xs(),
        Block::Code { text } => code_block(text, tokens, &state.mono),
        Block::List {
            ordered,
            start,
            items,
        } => {
            let mut column = div().flex().flex_col().gap_1().w_full().min_w_0();
            for (index, item) in items.iter().enumerate() {
                let marker = if *ordered {
                    format!("{}.", start + index as u64)
                } else {
                    "•".to_owned()
                };
                let mut content = div().flex().flex_col().gap_1().flex_1().min_w_0();
                for nested in item {
                    content = content.child(render_block(nested, state));
                }
                column = column.child(
                    div()
                        .flex()
                        .flex_row()
                        .gap_2()
                        .w_full()
                        .min_w_0()
                        .child(
                            // A fixed marker column keeps the content edges aligned
                            // across items regardless of the marker's own width.
                            div()
                                .w(px(16.0))
                                .flex_shrink_0()
                                .text_xs()
                                .text_color(tokens.muted)
                                .child(marker),
                        )
                        .child(content),
                );
            }
            column
        }
        Block::Quote { blocks } => div()
            .flex()
            .flex_row()
            .gap_2()
            .w_full()
            .min_w_0()
            .child(div().w(px(2.0)).flex_shrink_0().bg(tokens.border))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .flex_1()
                    .min_w_0()
                    .text_color(tokens.muted)
                    .children(blocks.iter().map(|block| render_block(block, state))),
            ),
        Block::Rule => div().w_full().h(px(1.0)).bg(tokens.border),
    }
}

/// A fenced or indented code block.
///
/// Long lines wrap rather than scroll. A horizontal scroll container needs a stable
/// element identity, and several code blocks in one changelog would collide on a single
/// id; wrapping loses the alignment of a wrapped line but never hides content, which is
/// the better failure for release notes.
fn code_block(text: &str, tokens: Tokens, mono: &SharedString) -> Div {
    div()
        .w_full()
        .min_w_0()
        .p_2()
        .rounded_md()
        .bg(tokens.border.opacity(0.35))
        .font_family(mono.clone())
        .text_xs()
        .text_color(tokens.text)
        .child(text.trim_end_matches('\n').to_owned())
}

/// Lay inline content out as a wrapping row of styled chunks.
///
/// GPUI has no inline layout, so mixed styling inside one paragraph is expressed as a
/// flex row of chunks. Text wraps inside a chunk and between chunks, which means a
/// sentence can wrap at a style boundary rather than strictly at a space — the price of
/// making links clickable, which a single styled text run cannot do. For release notes
/// (short lines, mostly one style per line) the difference does not show.
fn inline_row(spans: &[Inline], state: &mut RenderState) -> Div {
    let mut row = div().flex().flex_wrap().w_full().min_w_0();
    for span in spans {
        row = match span {
            Inline::Text { text, style } => row.child(inline_chunk(text, style, state)),
            // A full-width element forces the following content onto a new line.
            Inline::Break => row.child(div().w_full()),
        };
    }
    row
}

fn inline_chunk(text: &str, style: &SpanStyle, state: &mut RenderState) -> AnyElement {
    let tokens = state.tokens;
    let mut chunk = div().min_w_0().text_xs();
    if style.code {
        chunk = chunk
            .font_family(state.mono.clone())
            .px_1()
            .rounded_md()
            .bg(tokens.border.opacity(0.35));
    }
    if style.bold {
        chunk = chunk.font_weight(FontWeight::BOLD);
    }
    if style.italic {
        chunk = chunk.italic();
    }
    if style.strikethrough {
        chunk = chunk.line_through();
    }
    match &style.link {
        Some(url) => chunk
            .text_color(tokens.accent)
            .underline()
            .id(state.link_id(url))
            .on_click({
                let url = url.clone();
                move |_, _, _| {
                    // The URL was already restricted to HTTPS when it was parsed, and
                    // the platform opener re-checks the scheme.
                    let _ = bongocat_platform::open_external_url(&url);
                }
            })
            .child(text.to_owned())
            // Registers the link for the headless window tests; the trait's method is
            // the identity function when `gpui-kit`'s `test-support` feature is off.
            .test_support()
            .into_any_element(),
        None => chunk
            .text_color(tokens.text)
            .child(text.to_owned())
            .into_any_element(),
    }
}

#[cfg(test)]
mod tests {
    use super::{Block, Inline, SpanStyle, blocks, clickable_link, truncate};
    use super::{MAXIMUM_MARKDOWN_BYTES, TRUNCATION_MARKER};

    fn text_of(spans: &[Inline]) -> String {
        spans
            .iter()
            .map(|span| match span {
                Inline::Text { text, .. } => text.clone(),
                Inline::Break => "\n".to_owned(),
            })
            .collect()
    }

    fn only_paragraph(blocks: &[Block]) -> &[Inline] {
        assert_eq!(blocks.len(), 1, "expected one block, got {blocks:?}");
        match &blocks[0] {
            Block::Paragraph { spans } => spans,
            other => panic!("expected a paragraph, got {other:?}"),
        }
    }

    #[test]
    fn headings_carry_their_level() {
        let parsed = blocks("# One\n\n### Three\n");
        assert_eq!(
            parsed,
            vec![
                Block::Heading {
                    level: 1,
                    spans: vec![Inline::Text {
                        text: "One".to_owned(),
                        style: SpanStyle::default()
                    }]
                },
                Block::Heading {
                    level: 3,
                    spans: vec![Inline::Text {
                        text: "Three".to_owned(),
                        style: SpanStyle::default()
                    }]
                },
            ]
        );
    }

    #[test]
    fn emphasis_strong_and_strikethrough_nest() {
        let parsed = blocks("plain **bold _both_** ~~gone~~\n");
        let spans = only_paragraph(&parsed);
        assert_eq!(text_of(spans), "plain bold both gone");
        let styles: Vec<&SpanStyle> = spans
            .iter()
            .filter_map(|span| match span {
                Inline::Text { style, .. } => Some(style),
                Inline::Break => None,
            })
            .collect();
        assert!(
            styles
                .iter()
                .any(|style| !style.bold && !style.italic && !style.strikethrough),
            "the plain run must stay plain: {styles:?}"
        );
        assert!(
            styles.iter().any(|style| style.bold && !style.italic),
            "the bold run must be bold: {styles:?}"
        );
        assert!(
            styles.iter().any(|style| style.bold && style.italic),
            "emphasis inside strong must carry both: {styles:?}"
        );
        assert!(
            styles.iter().any(|style| style.strikethrough),
            "strikethrough must be marked: {styles:?}"
        );
    }

    /// Adjacent runs with the same style have to merge, or a long paragraph becomes one
    /// element per parser event.
    #[test]
    fn adjacent_runs_merge_into_one_span() {
        let parsed = blocks("**bold**\n");
        let spans = only_paragraph(&parsed);
        assert_eq!(spans.len(), 1, "got {spans:?}");
    }

    #[test]
    fn bullet_and_ordered_lists_keep_their_shape() {
        let parsed = blocks("- a\n- b\n\n1. one\n2. two\n");
        let [
            Block::List {
                ordered: false,
                items: bullets,
                ..
            },
            Block::List {
                ordered: true,
                start: 1,
                items: numbers,
            },
        ] = parsed.as_slice()
        else {
            panic!("expected two lists, got {parsed:?}");
        };
        assert_eq!(bullets.len(), 2);
        assert_eq!(numbers.len(), 2);
    }

    /// An ordered list starting at a number other than one keeps that number.
    #[test]
    fn an_ordered_list_keeps_its_start() {
        let parsed = blocks("3. third\n4. fourth\n");
        let [
            Block::List {
                ordered: true,
                start: 3,
                items,
            },
        ] = parsed.as_slice()
        else {
            panic!("expected one ordered list, got {parsed:?}");
        };
        assert_eq!(items.len(), 2);
    }

    #[test]
    fn nested_lists_stay_nested() {
        let parsed = blocks("- outer\n  - inner\n");
        let [Block::List { items, .. }] = parsed.as_slice() else {
            panic!("expected a list, got {parsed:?}");
        };
        assert_eq!(items.len(), 1);
        assert!(
            items[0]
                .iter()
                .any(|block| matches!(block, Block::List { .. })),
            "the inner list must survive: {items:?}"
        );
    }

    #[test]
    fn fenced_and_indented_code_blocks_keep_their_text() {
        let fenced = blocks("```rust\nlet x = 1;\n```\n");
        let [Block::Code { text }] = fenced.as_slice() else {
            panic!("expected a code block, got {fenced:?}");
        };
        assert_eq!(text, "let x = 1;\n");

        let indented = blocks("    indented\n");
        let [Block::Code { text }] = indented.as_slice() else {
            panic!("expected a code block, got {indented:?}");
        };
        assert_eq!(text, "indented\n");
    }

    /// Code block content is not Markdown: markers inside it must survive verbatim.
    #[test]
    fn code_block_content_is_not_reparsed() {
        let parsed = blocks("```\n**not bold** [not a link](https://x)\n```\n");
        let [Block::Code { text }] = parsed.as_slice() else {
            panic!("expected a code block, got {parsed:?}");
        };
        assert_eq!(text, "**not bold** [not a link](https://x)\n");
    }

    #[test]
    fn inline_code_is_marked_as_code() {
        let parsed = blocks("run `bongocat` now\n");
        let spans = only_paragraph(&parsed);
        let code = spans
            .iter()
            .find_map(|span| match span {
                Inline::Text { text, style } if style.code => Some(text.clone()),
                _ => None,
            })
            .expect("the code span must be marked");
        assert_eq!(code, "bongocat");
    }

    #[test]
    fn block_quotes_and_rules_are_their_own_blocks() {
        let parsed = blocks("> quoted\n\n---\n");
        let [Block::Quote { blocks: quoted }, Block::Rule] = parsed.as_slice() else {
            panic!("expected a quote and a rule, got {parsed:?}");
        };
        assert_eq!(text_of(only_paragraph(quoted)), "quoted");
    }

    /// A soft break is kept, because release notes are written with intentional lines.
    #[test]
    fn line_breaks_are_kept() {
        let parsed = blocks("first\nsecond\n");
        let spans = only_paragraph(&parsed);
        assert_eq!(text_of(spans), "first\nsecond");
    }

    #[test]
    fn a_hard_break_is_kept_too() {
        let parsed = blocks("first  \nsecond\n");
        let spans = only_paragraph(&parsed);
        assert_eq!(text_of(spans), "first\nsecond");
    }

    #[test]
    fn task_list_markers_survive_as_text() {
        let parsed = blocks("- [x] done\n- [ ] todo\n");
        let [Block::List { items, .. }] = parsed.as_slice() else {
            panic!("expected a list, got {parsed:?}");
        };
        let rendered: Vec<String> = items
            .iter()
            .map(|item| text_of(only_paragraph(item)))
            .collect();
        assert_eq!(rendered, vec!["☑ done".to_owned(), "☐ todo".to_owned()]);
    }

    /// Only HTTPS links become controls; anything else renders as plain text with the
    /// destination still visible.
    #[test]
    fn only_https_links_are_clickable() {
        assert_eq!(
            clickable_link("https://example.com/a"),
            Some("https://example.com/a".to_owned())
        );
        for rejected in [
            "http://example.com",
            "javascript:alert(1)",
            "file:///etc/passwd",
            "data:text/html,<script>",
            "HTTPS://example.com",
            "https://",
            "https://exa mple.com",
            "https://example.com/\n<script>",
        ] {
            assert_eq!(
                clickable_link(rejected),
                None,
                "{rejected} must not be clickable"
            );
        }
    }

    #[test]
    fn a_link_span_carries_the_validated_url() {
        let parsed = blocks("see [docs](https://example.com/x)\n");
        let spans = only_paragraph(&parsed);
        let link = spans
            .iter()
            .find_map(|span| match span {
                Inline::Text { style, .. } => style.link.clone(),
                Inline::Break => None,
            })
            .expect("the link must be marked");
        assert_eq!(link, "https://example.com/x");
    }

    /// A link inside a list item must be parsed like any other link.
    #[test]
    fn a_link_inside_a_list_item_is_marked() {
        let parsed = blocks("## Fixes\n\n- fixed [the issue](https://example.com/issues/47)\n");
        let rendered = format!("{parsed:?}");
        assert!(
            rendered.contains("https://example.com/issues/47"),
            "the link URL must survive list parsing: {rendered}"
        );
    }

    #[test]
    fn a_rejected_link_still_renders_its_text() {
        let parsed = blocks("[click](javascript:alert(1))\n");
        let spans = only_paragraph(&parsed);
        assert_eq!(text_of(spans), "click");
        assert!(
            spans.iter().all(|span| match span {
                Inline::Text { style, .. } => style.link.is_none(),
                Inline::Break => true,
            }),
            "a rejected link must not be marked clickable: {spans:?}"
        );
    }

    /// Raw HTML is shown, never interpreted. There is no markup layer for it to reach.
    #[test]
    fn raw_html_renders_as_literal_text() {
        let parsed = blocks("<script>alert(1)</script>\n");
        let spans = only_paragraph(&parsed);
        assert_eq!(text_of(spans), "<script>alert(1)</script>");
        assert!(
            spans.iter().all(|span| match span {
                Inline::Text { style, .. } => style.link.is_none(),
                Inline::Break => true,
            }),
            "raw HTML must not produce a link"
        );

        let block = blocks("<div>\nblock html\n</div>\n");
        assert!(
            text_of(only_paragraph(&block)).contains("block html"),
            "an HTML block's content must survive: {block:?}"
        );
    }

    #[test]
    fn an_image_renders_its_alt_text_and_no_url() {
        let parsed = blocks("![a diagram](https://example.com/x.png)\n");
        let spans = only_paragraph(&parsed);
        assert_eq!(text_of(spans), "a diagram");
        assert!(
            spans.iter().all(|span| match span {
                Inline::Text { style, .. } => style.link.is_none(),
                Inline::Break => true,
            }),
            "an image must not become a link: {spans:?}"
        );
    }

    /// Character references are decoded by the parser, so what reaches the renderer is
    /// the character the author meant — not an entity that a later layer could
    /// re-interpret.
    #[test]
    fn character_references_are_decoded_once() {
        let parsed = blocks("a &amp; b &lt;tag&gt; &#35;\n");
        let spans = only_paragraph(&parsed);
        assert_eq!(text_of(spans), "a & b <tag> #");
    }

    /// Deep nesting is legal input, so it has to be handled rather than trusted.
    #[test]
    fn deep_nesting_is_flattened_without_losing_text() {
        let mut markdown = String::new();
        for depth in 0..40 {
            markdown.push_str(&"  ".repeat(depth));
            markdown.push_str("- item\n");
        }
        let parsed = blocks(&markdown);
        let rendered = format!("{parsed:?}");
        assert!(
            rendered.contains("item"),
            "the content must survive deep nesting"
        );
        assert!(
            rendered.matches("List").count() <= super::MAXIMUM_BLOCK_DEPTH + 2,
            "nesting must be capped, got {} lists",
            rendered.matches("List").count()
        );
    }

    #[test]
    fn an_over_long_changelog_is_truncated_at_a_character_boundary() {
        let markdown = "é".repeat(MAXIMUM_MARKDOWN_BYTES);
        let truncated = truncate(&markdown);
        assert!(truncated.len() <= MAXIMUM_MARKDOWN_BYTES + TRUNCATION_MARKER.len());
        assert!(truncated.ends_with(TRUNCATION_MARKER));
        assert!(std::str::from_utf8(truncated.as_bytes()).is_ok());

        let short = "# fine";
        assert_eq!(truncate(short), short);
    }

    /// Arbitrary bytes must not panic the parser or the renderer's input stage.
    #[test]
    fn malformed_markdown_does_not_panic() {
        for input in [
            "",
            "#",
            "- ",
            "> > > unbalanced",
            "```\nunterminated",
            "[link](",
            "**",
            "***",
            "|a|b|\n|-|-|",
            "\u{0}\u{1}\u{7f}",
            "a\tb\rc\nd",
            "🎉 **粗体** 🎉",
        ] {
            let _ = blocks(input);
        }
    }

    /// The renderer must cope with every block shape the parser can produce.
    #[test]
    fn every_block_shape_parses_from_real_markdown() {
        let markdown = "\
# Title
## Section
Paragraph with **bold**, _italic_, `code` and [a link](https://example.com).

- bullet
- bullet

1. one
2. two

> quote

```sh
echo hi
```

---
";
        let parsed = blocks(markdown);
        assert!(parsed.iter().any(|b| matches!(b, Block::Heading { .. })));
        assert!(parsed.iter().any(|b| matches!(b, Block::Paragraph { .. })));
        assert!(parsed.iter().any(|b| matches!(b, Block::List { .. })));
        assert!(parsed.iter().any(|b| matches!(b, Block::Quote { .. })));
        assert!(parsed.iter().any(|b| matches!(b, Block::Code { .. })));
        assert!(parsed.iter().any(|b| matches!(b, Block::Rule)));
    }
}
