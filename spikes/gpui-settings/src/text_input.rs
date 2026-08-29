use std::ops::Range;

use gpui::{
    App, Bounds, ClipboardItem, Context, CursorStyle, Element, ElementId, ElementInputHandler,
    Entity, EntityInputHandler, FocusHandle, Focusable, GlobalElementId, InspectorElementId,
    IntoElement, LayoutId, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PaintQuad,
    Pixels, Point, Render, ShapedLine, Style, TextRun, UTF16Selection, UnderlineStyle, Window,
    actions, div, fill, hsla, point, prelude::*, px, relative, rgb, rgba, size,
};
use unicode_segmentation::UnicodeSegmentation as _;

actions!(
    settings_text_input,
    [
        Backspace,
        Delete,
        Left,
        Right,
        SelectLeft,
        SelectRight,
        SelectAll,
        Home,
        End,
        Paste,
        Cut,
        Copy,
    ]
);

pub struct TextInput {
    focus_handle: FocusHandle,
    dark_theme: bool,
    buffer: TextBuffer,
    last_layout: Option<ShapedLine>,
    last_bounds: Option<Bounds<Pixels>>,
    is_selecting: bool,
}

#[derive(Debug, Default, PartialEq, Eq)]
struct TextBuffer {
    content: String,
    selected_range: Range<usize>,
    selection_reversed: bool,
    marked_range: Option<Range<usize>>,
}

impl TextBuffer {
    fn move_to(&mut self, offset: usize) {
        let offset = offset.min(self.content.len());
        self.selected_range = offset..offset;
        self.selection_reversed = false;
    }

    fn select_to(&mut self, offset: usize) {
        let offset = offset.min(self.content.len());
        if self.selection_reversed {
            self.selected_range.start = offset;
        } else {
            self.selected_range.end = offset;
        }
        if self.selected_range.end < self.selected_range.start {
            self.selection_reversed = !self.selection_reversed;
            self.selected_range = self.selected_range.end..self.selected_range.start;
        }
    }

    fn cursor_offset(&self) -> usize {
        if self.selection_reversed {
            self.selected_range.start
        } else {
            self.selected_range.end
        }
    }

    fn previous_boundary(&self, offset: usize) -> usize {
        self.content
            .grapheme_indices(true)
            .rev()
            .find_map(|(index, _)| (index < offset).then_some(index))
            .unwrap_or(0)
    }

    fn next_boundary(&self, offset: usize) -> usize {
        self.content
            .grapheme_indices(true)
            .find_map(|(index, _)| (index > offset).then_some(index))
            .unwrap_or(self.content.len())
    }

    fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        utf16_offset_from_utf8(&self.content, range.start)
            ..utf16_offset_from_utf8(&self.content, range.end)
    }

    fn range_from_utf16(&self, range: &Range<usize>) -> Range<usize> {
        utf8_range_from_utf16(&self.content, range)
    }

    fn replace_text(&mut self, range: Option<Range<usize>>, new_text: &str) {
        let range = range
            .as_ref()
            .map(|range| self.range_from_utf16(range))
            .or(self.marked_range.clone())
            .unwrap_or(self.selected_range.clone());
        self.content.replace_range(range.clone(), new_text);
        let cursor = range.start + new_text.len();
        self.selected_range = cursor..cursor;
        self.selection_reversed = false;
        self.marked_range = None;
    }

    fn replace_and_mark_text(
        &mut self,
        range: Option<Range<usize>>,
        new_text: &str,
        selected_range: Option<Range<usize>>,
    ) {
        let range = range
            .as_ref()
            .map(|range| self.range_from_utf16(range))
            .or(self.marked_range.clone())
            .unwrap_or(self.selected_range.clone());
        self.content.replace_range(range.clone(), new_text);
        self.marked_range =
            (!new_text.is_empty()).then(|| range.start..range.start + new_text.len());

        // GPUI follows the platform text-input contracts: this selection is relative to
        // the replacement text, not to the input's complete post-replacement content.
        self.selected_range = selected_range
            .as_ref()
            .map(|selected| utf8_range_from_utf16(new_text, selected))
            .map(|selected| range.start + selected.start..range.start + selected.end)
            .unwrap_or_else(|| {
                let cursor = range.start + new_text.len();
                cursor..cursor
            });
        self.selection_reversed = false;
    }
}

fn utf8_offset_from_utf16(text: &str, offset: usize) -> usize {
    let mut utf8_offset = 0;
    let mut utf16_count = 0;
    for character in text.chars() {
        if utf16_count >= offset {
            break;
        }
        utf16_count += character.len_utf16();
        utf8_offset += character.len_utf8();
    }
    utf8_offset
}

fn utf16_offset_from_utf8(text: &str, offset: usize) -> usize {
    let mut utf8_count = 0;
    let mut utf16_offset = 0;
    for character in text.chars() {
        if utf8_count >= offset {
            break;
        }
        utf8_count += character.len_utf8();
        utf16_offset += character.len_utf16();
    }
    utf16_offset
}

fn utf8_range_from_utf16(text: &str, range: &Range<usize>) -> Range<usize> {
    let start = utf8_offset_from_utf16(text, range.start);
    let end = utf8_offset_from_utf16(text, range.end);
    start.min(end)..start.max(end)
}

impl TextInput {
    pub fn new(cx: &mut Context<Self>, dark_theme: bool) -> Self {
        Self {
            focus_handle: cx.focus_handle().tab_index(1).tab_stop(true),
            dark_theme,
            buffer: TextBuffer::default(),
            last_layout: None,
            last_bounds: None,
            is_selecting: false,
        }
    }

    pub fn content(&self) -> &str {
        &self.buffer.content
    }

    pub fn set_dark_theme(&mut self, dark_theme: bool) {
        self.dark_theme = dark_theme;
    }

    fn left(&mut self, _: &Left, _: &mut Window, cx: &mut Context<Self>) {
        if self.buffer.selected_range.is_empty() {
            self.move_to(
                self.buffer.previous_boundary(self.buffer.cursor_offset()),
                cx,
            );
        } else {
            self.move_to(self.buffer.selected_range.start, cx);
        }
    }

    fn right(&mut self, _: &Right, _: &mut Window, cx: &mut Context<Self>) {
        if self.buffer.selected_range.is_empty() {
            self.move_to(
                self.buffer.next_boundary(self.buffer.selected_range.end),
                cx,
            );
        } else {
            self.move_to(self.buffer.selected_range.end, cx);
        }
    }

    fn select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(
            self.buffer.previous_boundary(self.buffer.cursor_offset()),
            cx,
        );
    }

    fn select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.buffer.next_boundary(self.buffer.cursor_offset()), cx);
    }

    fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, cx);
        self.select_to(self.buffer.content.len(), cx);
    }

    fn home(&mut self, _: &Home, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, cx);
    }

    fn end(&mut self, _: &End, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.buffer.content.len(), cx);
    }

    fn backspace(&mut self, _: &Backspace, window: &mut Window, cx: &mut Context<Self>) {
        if self.buffer.selected_range.is_empty() {
            self.select_to(
                self.buffer.previous_boundary(self.buffer.cursor_offset()),
                cx,
            );
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    fn delete(&mut self, _: &Delete, window: &mut Window, cx: &mut Context<Self>) {
        if self.buffer.selected_range.is_empty() {
            self.select_to(self.buffer.next_boundary(self.buffer.cursor_offset()), cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    fn paste(&mut self, _: &Paste, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            self.replace_text_in_range(None, &text.replace(['\n', '\r'], " "), window, cx);
        }
    }

    fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        if !self.buffer.selected_range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.buffer.content[self.buffer.selected_range.clone()].to_string(),
            ));
        }
    }

    fn cut(&mut self, _: &Cut, window: &mut Window, cx: &mut Context<Self>) {
        if !self.buffer.selected_range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.buffer.content[self.buffer.selected_range.clone()].to_string(),
            ));
            self.replace_text_in_range(None, "", window, cx);
        }
    }

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus_handle);
        self.is_selecting = true;
        let offset = self.index_for_mouse_position(event.position);
        if event.modifiers.shift {
            self.select_to(offset, cx);
        } else {
            self.move_to(offset, cx);
        }
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
        self.is_selecting = false;
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.is_selecting {
            self.select_to(self.index_for_mouse_position(event.position), cx);
        }
    }

    fn move_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        self.buffer.move_to(offset);
        cx.notify();
    }

    fn select_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        self.buffer.select_to(offset);
        cx.notify();
    }

    fn index_for_mouse_position(&self, position: Point<Pixels>) -> usize {
        if self.buffer.content.is_empty() {
            return 0;
        }
        let (Some(bounds), Some(line)) = (self.last_bounds.as_ref(), self.last_layout.as_ref())
        else {
            return 0;
        };
        if position.y < bounds.top() {
            return 0;
        }
        if position.y > bounds.bottom() {
            return self.buffer.content.len();
        }
        line.closest_index_for_x(position.x - bounds.left())
    }
}

impl EntityInputHandler for TextInput {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.buffer.range_from_utf16(&range);
        actual_range.replace(self.buffer.range_to_utf16(&range));
        Some(self.buffer.content[range].to_string())
    }

    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.buffer.range_to_utf16(&self.buffer.selected_range),
            reversed: self.buffer.selection_reversed,
        })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.buffer
            .marked_range
            .as_ref()
            .map(|range| self.buffer.range_to_utf16(range))
    }

    fn unmark_text(&mut self, _: &mut Window, _: &mut Context<Self>) {
        self.buffer.marked_range = None;
    }

    fn replace_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        new_text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.buffer.replace_text(range, new_text);
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        new_text: &str,
        selected_range: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.buffer
            .replace_and_mark_text(range, new_text, selected_range);
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range: Range<usize>,
        bounds: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let line = self.last_layout.as_ref()?;
        let range = self.buffer.range_from_utf16(&range);
        Some(Bounds::from_corners(
            point(bounds.left() + line.x_for_index(range.start), bounds.top()),
            point(bounds.left() + line.x_for_index(range.end), bounds.bottom()),
        ))
    }

    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        let local = self.last_bounds?.localize(&point)?;
        let line = self.last_layout.as_ref()?;
        let index = line.index_for_x(point.x - local.x)?;
        Some(utf16_offset_from_utf8(&self.buffer.content, index))
    }
}

struct TextElement {
    input: Entity<TextInput>,
}

struct PrepaintState {
    line: Option<ShapedLine>,
    cursor: Option<PaintQuad>,
    selection: Option<PaintQuad>,
}

impl IntoElement for TextElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for TextElement {
    type RequestLayoutState = ();
    type PrepaintState = PrepaintState;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = window.line_height().into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> PrepaintState {
        let input = self.input.read(cx);
        let content = input.buffer.content.clone();
        let display_text = if content.is_empty() {
            "Type a model name...".to_string()
        } else {
            content
        };
        let text_color = if input.buffer.content.is_empty() {
            if input.dark_theme {
                hsla(0., 0., 0.61, 0.6)
            } else {
                hsla(0., 0., 0.45, 0.6)
            }
        } else {
            if input.dark_theme {
                hsla(0., 0., 0.96, 1.)
            } else {
                hsla(0., 0., 0.12, 1.)
            }
        };
        let base_run = TextRun {
            len: display_text.len(),
            font: window.text_style().font(),
            color: text_color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let runs = if let Some(marked) = input.buffer.marked_range.as_ref() {
            vec![
                TextRun {
                    len: marked.start,
                    ..base_run.clone()
                },
                TextRun {
                    len: marked.end - marked.start,
                    underline: Some(UnderlineStyle {
                        color: Some(base_run.color),
                        thickness: px(1.),
                        wavy: false,
                    }),
                    ..base_run.clone()
                },
                TextRun {
                    len: display_text.len() - marked.end,
                    ..base_run
                },
            ]
            .into_iter()
            .filter(|run| run.len > 0)
            .collect()
        } else {
            vec![base_run]
        };
        let font_size = window.text_style().font_size.to_pixels(window.rem_size());
        let line = window
            .text_system()
            .shape_line(display_text.into(), font_size, &runs, None);
        let selected = input.buffer.selected_range.clone();
        let cursor_x = line.x_for_index(input.buffer.cursor_offset());
        let (selection, cursor) = if selected.is_empty() {
            (
                None,
                Some(fill(
                    Bounds::new(
                        point(bounds.left() + cursor_x, bounds.top()),
                        size(px(1.5), bounds.size.height),
                    ),
                    rgb(0x4f8cff),
                )),
            )
        } else {
            (
                Some(fill(
                    Bounds::from_corners(
                        point(
                            bounds.left() + line.x_for_index(selected.start),
                            bounds.top(),
                        ),
                        point(
                            bounds.left() + line.x_for_index(selected.end),
                            bounds.bottom(),
                        ),
                    ),
                    rgba(0x4f8cff55),
                )),
                None,
            )
        };
        PrepaintState {
            line: Some(line),
            cursor,
            selection,
        }
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        prepaint: &mut PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus = self.input.read(cx).focus_handle.clone();
        window.handle_input(
            &focus,
            ElementInputHandler::new(bounds, self.input.clone()),
            cx,
        );
        if let Some(selection) = prepaint.selection.take() {
            window.paint_quad(selection);
        }
        let line = prepaint.line.take().expect("text line was shaped");
        line.paint(bounds.origin, window.line_height(), window, cx)
            .expect("text line paints");
        if focus.is_focused(window)
            && let Some(cursor) = prepaint.cursor.take()
        {
            window.paint_quad(cursor);
        }
        self.input.update(cx, |input, _| {
            input.last_layout = Some(line);
            input.last_bounds = Some(bounds);
        });
    }
}

impl Render for TextInput {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let focused = self.focus_handle.is_focused(window);
        let dark = self.dark_theme;
        div()
            .id("model-name-input")
            .key_context("SettingsTextInput")
            .track_focus(&self.focus_handle)
            .tab_index(1)
            .cursor(CursorStyle::IBeam)
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::left))
            .on_action(cx.listener(Self::right))
            .on_action(cx.listener(Self::select_left))
            .on_action(cx.listener(Self::select_right))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::home))
            .on_action(cx.listener(Self::end))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::copy))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .w_full()
            .h(px(42.))
            .flex()
            .items_center()
            .px_3()
            .rounded_md()
            .border_1()
            .border_color(if focused {
                rgb(0x4f8cff)
            } else if dark {
                rgb(0x59616f)
            } else {
                rgb(0xc5cad2)
            })
            .bg(if dark { rgb(0x181b20) } else { rgb(0xffffff) })
            .text_color(if dark { rgb(0xf6f7f9) } else { rgb(0x1d2129) })
            .text_size(px(15.))
            .line_height(px(22.))
            .child(TextElement { input: cx.entity() })
    }
}

impl Focusable for TextInput {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marked_text_selection_is_relative_to_each_composition_update() {
        let mut buffer = TextBuffer::default();
        buffer.replace_text(None, "猫");

        buffer.replace_and_mark_text(None, "ni", Some(2..2));
        assert_eq!(buffer.content, "猫ni");
        assert_eq!(buffer.marked_range, Some(3..5));
        assert_eq!(buffer.selected_range, 5..5);

        buffer.replace_and_mark_text(None, "你", Some(1..1));
        assert_eq!(buffer.content, "猫你");
        assert_eq!(buffer.marked_range, Some(3..6));
        assert_eq!(buffer.selected_range, 6..6);
        buffer.replace_text(None, "你");

        buffer.replace_and_mark_text(None, "hao", Some(3..3));
        buffer.replace_and_mark_text(None, "好", Some(1..1));
        buffer.replace_text(None, "好");
        assert_eq!(buffer.content, "猫你好");
        assert_eq!(buffer.selected_range, 9..9);
        assert_eq!(buffer.marked_range, None);
    }

    #[test]
    fn marked_text_selection_handles_utf16_surrogate_pairs() {
        let mut buffer = TextBuffer::default();
        buffer.replace_text(None, "前");
        buffer.replace_and_mark_text(None, "😀a", Some(2..2));

        assert_eq!(buffer.content, "前😀a");
        assert_eq!(buffer.marked_range, Some(3..8));
        assert_eq!(buffer.selected_range, 7..7);
        assert_eq!(buffer.range_to_utf16(&buffer.selected_range), 3..3);
    }

    #[test]
    fn committed_replacement_clears_reversed_selection_state() {
        let mut buffer = TextBuffer {
            content: "A😀B".to_string(),
            selected_range: 1..5,
            selection_reversed: true,
            marked_range: None,
        };

        buffer.replace_text(None, "猫");

        assert_eq!(buffer.content, "A猫B");
        assert_eq!(buffer.selected_range, 4..4);
        assert!(!buffer.selection_reversed);
    }

    #[test]
    fn malformed_utf16_ranges_are_clamped_and_normalized() {
        let text = "A😀猫";
        let reversed_start = 99;
        let reversed_end = 1;

        assert_eq!(utf8_range_from_utf16(text, &(1..3)), 1..5);
        assert_eq!(
            utf8_range_from_utf16(text, &(reversed_start..reversed_end)),
            1..8
        );
        assert_eq!(utf8_range_from_utf16(text, &(2..2)), 5..5);
    }
}
