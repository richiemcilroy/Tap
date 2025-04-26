use crate::models::{Database, Note};
use crate::util::{
    dump_db_contents, get_db_path,
    macos_menu::{ContextMenu, MenuAction},
    NOTE_TO_DELETE,
};
use gpui::{
    Action, App, ClipboardItem, CursorStyle, ElementId, ElementInputHandler, Entity,
    EntityInputHandler, FocusHandle, Focusable, FontWeight, GlobalElementId, KeyDownEvent,
    LayoutId, Menu, MenuItem, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PaintQuad,
    Pixels, Point, Render, ShapedLine, SharedString, Style, TextRun, UTF16Selection,
    UnderlineStyle, Window, div, point, prelude::*, px, relative, rgb, rgba, size,
};
use std::ops::Range;
use std::sync::Arc;
use unicode_segmentation::UnicodeSegmentation;
use uuid::Uuid;
use objc::{class, msg_send, sel, sel_impl};
use std::sync::Once;
use lazy_static::lazy_static;
use block::ConcreteBlock;

const LINE_HEIGHT: f32 = 20.0;

pub struct NoteApp {
    db: Arc<Database>,
    notes: Vec<Note>,
    active_note_id: Option<Uuid>,
    editor: Entity<NoteEditor>,
    content_focus_handle: FocusHandle,
    title_edit_mode: bool,
    title_text: String,
    title_focus_handle: FocusHandle,
    title_editor: Entity<TitleEditor>,
}

pub struct NoteEditor {
    focus_handle: FocusHandle,
    content: SharedString,
    selected_range: Range<usize>,
    selection_reversed: bool,
    marked_range: Option<Range<usize>>,
    last_layout: Option<ShapedLine>,
    last_bounds: Option<gpui::Bounds<Pixels>>,
    is_selecting: bool,
    on_change: Option<Box<dyn Fn(String, &mut Context<NoteEditor>)>>,
}

pub struct TitleEditor {
    focus_handle: FocusHandle,
    content: SharedString,
    selected_range: Range<usize>,
    selection_reversed: bool,
    on_change: Option<Box<dyn Fn(String, &mut Context<TitleEditor>)>>,
}

impl NoteEditor {
    fn set_content(&mut self, content: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.content = content.into();
        self.selected_range = self.content.len()..self.content.len();
        self.selection_reversed = false;
        self.marked_range = None;
        cx.notify();
    }

    fn set_on_change<F>(&mut self, callback: F)
    where
        F: Fn(String, &mut Context<NoteEditor>) + 'static,
    {
        self.on_change = Some(Box::new(callback));
    }

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.is_selecting = true;

        if event.modifiers.shift {
            self.select_to(self.index_for_mouse_position(event.position), cx);
        } else {
            self.move_to(self.index_for_mouse_position(event.position), cx)
        }
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _window: &mut Window, _: &mut Context<Self>) {
        self.is_selecting = false;
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.is_selecting {
            self.select_to(self.index_for_mouse_position(event.position), cx);
        }
    }

    fn index_for_mouse_position(&self, position: Point<Pixels>) -> usize {
        if self.content.is_empty() {
            return 0;
        }

        let (Some(bounds), Some(line)) = (self.last_bounds.as_ref(), self.last_layout.as_ref())
        else {
            return 0;
        };

        let line_height = LINE_HEIGHT;
        let relative_y = (position.y - bounds.top()).0;
        let line_index = (relative_y / line_height).floor() as usize;
        let lines: Vec<&str> = self.content.split('\n').collect();

        if line_index >= lines.len() {
            return self.content.len();
        }

        let mut offset = 0;
        for i in 0..line_index {
            offset += lines[i].len() + 1;
        }

        if position.x < bounds.left() {
            return offset;
        }

        let current_line = lines[line_index];
        if current_line.is_empty() {
            return offset;
        }

        let x_within_line = position.x - bounds.left();
        let closest_index = line
            .closest_index_for_x(x_within_line)
            .min(current_line.len());

        offset + closest_index
    }

    fn move_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        self.selected_range = offset..offset;
        cx.notify()
    }

    fn select_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        if self.selection_reversed {
            self.selected_range.start = offset
        } else {
            self.selected_range.end = offset
        };
        if self.selected_range.end < self.selected_range.start {
            self.selection_reversed = !self.selection_reversed;
            self.selected_range = self.selected_range.end..self.selected_range.start;
        }
        cx.notify()
    }

    fn select_all(&mut self, cx: &mut Context<Self>) {
        self.move_to(0, cx);
        self.select_to(self.content.len(), cx)
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
            .find_map(|(idx, _)| (idx < offset).then_some(idx))
            .unwrap_or(0)
    }

    fn next_boundary(&self, offset: usize) -> usize {
        self.content
            .grapheme_indices(true)
            .find_map(|(idx, _)| (idx > offset).then_some(idx))
            .unwrap_or(self.content.len())
    }

    fn on_backspace(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.select_to(self.previous_boundary(self.cursor_offset()), cx)
        }
        self.replace_text_in_range(None, "", window, cx)
    }

    fn on_delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.select_to(self.next_boundary(self.cursor_offset()), cx)
        }
        self.replace_text_in_range(None, "", window, cx)
    }

    fn on_left(&mut self, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.move_to(self.previous_boundary(self.cursor_offset()), cx);
        } else {
            self.move_to(self.selected_range.start, cx)
        }
    }

    fn on_right(&mut self, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.move_to(self.next_boundary(self.selected_range.end), cx);
        } else {
            self.move_to(self.selected_range.end, cx)
        }
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if event.keystroke.key == "enter" {
            self.replace_text_in_range(None, "\n", window, cx);
            return;
        }

        if event.keystroke.key_char.is_some() {
            return;
        } else if event.keystroke.key == "backspace" {
            self.on_backspace(window, cx);
        } else if event.keystroke.key == "delete" {
            self.on_delete(window, cx);
        } else if event.keystroke.key == "arrowleft" {
            if event.keystroke.modifiers.shift {
                self.select_to(self.previous_boundary(self.cursor_offset()), cx);
            } else {
                self.on_left(cx);
            }
        } else if event.keystroke.key == "arrowright" {
            if event.keystroke.modifiers.shift {
                self.select_to(self.next_boundary(self.cursor_offset()), cx);
            } else {
                self.on_right(cx);
            }
        } else if event.keystroke.key == "arrowup" {
            if event.keystroke.modifiers.shift {
                let cursor = self.cursor_offset();
                self.move_up(cx);
                let new_cursor = self.cursor_offset();
                self.selected_range = new_cursor..new_cursor;
                self.select_to(cursor, cx);
            } else {
                self.move_up(cx);
            }
        } else if event.keystroke.key == "arrowdown" {
            if event.keystroke.modifiers.shift {
                let cursor = self.cursor_offset();
                self.move_down(cx);
                let new_cursor = self.cursor_offset();
                self.selected_range = new_cursor..new_cursor;
                self.select_to(cursor, cx);
            } else {
                self.move_down(cx);
            }
        } else if event.keystroke.key == "home" {
            let line = self.line_at_offset(self.cursor_offset());
            let line_start = self.offset_at_line_start(line);
            if event.keystroke.modifiers.shift {
                self.select_to(line_start, cx);
            } else {
                self.move_to(line_start, cx);
            }
        } else if event.keystroke.key == "end" {
            let line = self.line_at_offset(self.cursor_offset());
            let line_end = self.offset_at_line_end(line);
            if event.keystroke.modifiers.shift {
                self.select_to(line_end, cx);
            } else {
                self.move_to(line_end, cx);
            }
        } else if event.keystroke.key == "a" && event.keystroke.modifiers.platform {
            self.select_all(cx);
        } else if event.keystroke.key == "c" && event.keystroke.modifiers.platform {
            if !self.selected_range.is_empty() {
                cx.write_to_clipboard(ClipboardItem::new_string(
                    (&self.content[self.selected_range.clone()]).to_string(),
                ));
            }
        } else if event.keystroke.key == "x" && event.keystroke.modifiers.platform {
            if !self.selected_range.is_empty() {
                cx.write_to_clipboard(ClipboardItem::new_string(
                    (&self.content[self.selected_range.clone()]).to_string(),
                ));
                self.replace_text_in_range(None, "", window, cx);
            }
        } else if event.keystroke.key == "v" && event.keystroke.modifiers.platform {
            if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                self.replace_text_in_range(None, &text, window, cx);
            }
        }
    }

    fn offset_from_utf16(&self, offset: usize) -> usize {
        let mut utf8_offset = 0;
        let mut utf16_count = 0;

        for ch in self.content.chars() {
            if utf16_count >= offset {
                break;
            }
            utf16_count += ch.len_utf16();
            utf8_offset += ch.len_utf8();
        }

        utf8_offset
    }

    fn offset_to_utf16(&self, offset: usize) -> usize {
        let mut utf16_offset = 0;
        let mut utf8_count = 0;

        for ch in self.content.chars() {
            if utf8_count >= offset {
                break;
            }
            utf8_count += ch.len_utf8();
            utf16_offset += ch.len_utf16();
        }

        utf16_offset
    }

    fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_to_utf16(range.start)..self.offset_to_utf16(range.end)
    }

    fn range_from_utf16(&self, range_utf16: &Range<usize>) -> Range<usize> {
        self.offset_from_utf16(range_utf16.start)..self.offset_from_utf16(range_utf16.end)
    }

    fn line_at_offset(&self, offset: usize) -> usize {
        let mut current_offset = 0;
        for (i, line) in self.content.split('\n').enumerate() {
            let line_end = current_offset + line.len();
            if offset <= line_end {
                return i;
            }
            current_offset = line_end + 1;
        }
        self.content.split('\n').count() - 1
    }

    fn offset_at_line_start(&self, line_number: usize) -> usize {
        let mut offset = 0;
        for (i, line) in self.content.split('\n').enumerate() {
            if i == line_number {
                return offset;
            }
            offset += line.len() + 1;
        }
        self.content.len()
    }

    fn offset_at_line_end(&self, line_number: usize) -> usize {
        let mut offset = 0;
        for (i, line) in self.content.split('\n').enumerate() {
            offset += line.len();
            if i == line_number {
                return offset;
            }
            offset += 1;
        }
        self.content.len()
    }

    fn line_length(&self, line_number: usize) -> usize {
        let lines: Vec<&str> = self.content.split('\n').collect();
        if line_number < lines.len() {
            lines[line_number].len()
        } else {
            0
        }
    }

    fn move_up(&mut self, cx: &mut Context<Self>) {
        let cursor = self.cursor_offset();
        let current_line = self.line_at_offset(cursor);

        if current_line > 0 {
            let current_line_start = self.offset_at_line_start(current_line);
            let x_offset = cursor - current_line_start;

            let prev_line = current_line - 1;
            let prev_line_start = self.offset_at_line_start(prev_line);
            let prev_line_len = self.line_length(prev_line);

            let new_offset = prev_line_start + x_offset.min(prev_line_len);
            self.move_to(new_offset, cx);
        }
    }

    fn move_down(&mut self, cx: &mut Context<Self>) {
        let cursor = self.cursor_offset();
        let current_line = self.line_at_offset(cursor);
        let line_count = self.content.split('\n').count();

        if current_line < line_count - 1 {
            let current_line_start = self.offset_at_line_start(current_line);
            let x_offset = cursor - current_line_start;

            let next_line = current_line + 1;
            let next_line_start = self.offset_at_line_start(next_line);
            let next_line_len = self.line_length(next_line);

            let new_offset = next_line_start + x_offset.min(next_line_len);
            self.move_to(new_offset, cx);
        }
    }
}

impl EntityInputHandler for NoteEditor {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.range_from_utf16(&range_utf16);
        actual_range.replace(self.range_to_utf16(&range));
        Some(self.content[range].to_string())
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.range_to_utf16(&self.selected_range),
            reversed: self.selection_reversed,
        })
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        self.marked_range
            .as_ref()
            .map(|range| self.range_to_utf16(range))
    }

    fn unmark_text(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {
        self.marked_range = None;
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|range_utf16| self.range_from_utf16(range_utf16))
            .or(self.marked_range.clone())
            .unwrap_or(self.selected_range.clone());

        self.content =
            (self.content[0..range.start].to_owned() + new_text + &self.content[range.end..])
                .into();
        self.selected_range = range.start + new_text.len()..range.start + new_text.len();
        self.marked_range.take();

        if let Some(on_change) = &self.on_change {
            on_change(self.content.to_string(), cx);
        }

        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|range_utf16| self.range_from_utf16(range_utf16))
            .or(self.marked_range.clone())
            .unwrap_or(self.selected_range.clone());

        self.content =
            (self.content[0..range.start].to_owned() + new_text + &self.content[range.end..])
                .into();
        self.marked_range = Some(range.start..range.start + new_text.len());
        self.selected_range = new_selected_range_utf16
            .as_ref()
            .map(|range_utf16| self.range_from_utf16(range_utf16))
            .map(|new_range| new_range.start + range.start..new_range.end + range.end)
            .unwrap_or_else(|| range.start + new_text.len()..range.start + new_text.len());

        if let Some(on_change) = &self.on_change {
            on_change(self.content.to_string(), cx);
        }

        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        bounds: gpui::Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<gpui::Bounds<Pixels>> {
        let last_layout = self.last_layout.as_ref()?;
        let range = self.range_from_utf16(&range_utf16);
        Some(gpui::Bounds::from_corners(
            point(
                bounds.left() + last_layout.x_for_index(range.start),
                bounds.top(),
            ),
            point(
                bounds.left() + last_layout.x_for_index(range.end),
                bounds.bottom(),
            ),
        ))
    }

    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        let line_point = self.last_bounds?.localize(&point)?;
        let last_layout = self.last_layout.as_ref()?;

        assert_eq!(last_layout.text, self.content);
        let utf8_index = last_layout.index_for_x(point.x - line_point.x)?;
        Some(self.offset_to_utf16(utf8_index))
    }
}

impl TitleEditor {
    fn set_content(&mut self, content: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.content = content.into();
        self.selected_range = self.content.len()..self.content.len();
        self.selection_reversed = false;
        cx.notify();
    }

    fn set_on_change<F>(&mut self, callback: F)
    where
        F: Fn(String, &mut Context<TitleEditor>) + 'static,
    {
        self.on_change = Some(Box::new(callback));
    }

    fn cursor_offset(&self) -> usize {
        if self.selection_reversed {
            self.selected_range.start
        } else {
            self.selected_range.end
        }
    }

    fn previous_boundary(&self, offset: usize) -> usize {
        if offset > 0 { offset - 1 } else { 0 }
    }

    fn next_boundary(&self, offset: usize) -> usize {
        if offset < self.content.len() {
            offset + 1
        } else {
            self.content.len()
        }
    }

    fn on_backspace(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            if self.selected_range.start > 0 {
                self.selected_range =
                    self.previous_boundary(self.cursor_offset())..self.selected_range.end;
            }
        }
        if !self.selected_range.is_empty() {
            self.replace_text_in_range(Some(self.selected_range.clone()), "", window, cx);
        }
    }

    fn on_delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            if self.selected_range.end < self.content.len() {
                self.selected_range =
                    self.selected_range.start..self.next_boundary(self.cursor_offset());
            }
        }
        if !self.selected_range.is_empty() {
            self.replace_text_in_range(Some(self.selected_range.clone()), "", window, cx);
        }
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if event.keystroke.key_char.is_some() {
            return;
        } else if event.keystroke.key == "backspace" {
            self.on_backspace(window, cx);
        } else if event.keystroke.key == "delete" {
            self.on_delete(window, cx);
        }
    }

    fn replace_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        new_text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range.unwrap_or(self.selected_range.clone());

        self.content =
            (self.content[0..range.start].to_owned() + new_text + &self.content[range.end..])
                .into();
        self.selected_range = range.start + new_text.len()..range.start + new_text.len();

        if let Some(on_change) = &self.on_change {
            on_change(self.content.to_string(), cx);
        }

        cx.notify();
    }
}

impl EntityInputHandler for TitleEditor {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        actual_range.replace(range.clone());
        Some(self.content[range.clone()].to_string())
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.selected_range.clone(),
            reversed: self.selection_reversed,
        })
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        None
    }

    fn unmark_text(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {}

    fn replace_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        new_text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range.unwrap_or(self.selected_range.clone());

        self.content =
            (self.content[0..range.start].to_owned() + new_text + &self.content[range.end..])
                .into();
        self.selected_range = range.start + new_text.len()..range.start + new_text.len();

        if let Some(on_change) = &self.on_change {
            on_change(self.content.to_string(), cx);
        }

        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        new_text: &str,
        new_selected_range: Option<Range<usize>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.replace_text_in_range(range, new_text, window, cx);
    }

    fn bounds_for_range(
        &mut self,
        _range: Range<usize>,
        bounds: gpui::Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<gpui::Bounds<Pixels>> {
        Some(bounds)
    }

    fn character_index_for_point(
        &mut self,
        _point: Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        Some(self.content.len())
    }
}

impl Focusable for TitleEditor {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for TitleEditor {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .size_full()
            .cursor(CursorStyle::IBeam)
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(Self::on_key_down))
            .child(TitleEditorView {
                editor: cx.entity().clone(),
            })
    }
}
struct TitleEditorView {
    editor: Entity<TitleEditor>,
}

impl IntoElement for TitleEditorView {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

struct TitleEditorPrepaintState {
    text: ShapedLine,
    cursor: Option<PaintQuad>,
}

impl Element for TitleEditorView {
    type RequestLayoutState = ();
    type PrepaintState = Option<TitleEditorPrepaintState>;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = window.line_height().into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        bounds: gpui::Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let editor = self.editor.read(cx);
        let content = editor.content.clone();
        let focus_handle = editor.focus_handle.clone();

        let style = window.text_style();
        let font_size = style.font_size.to_pixels(window.rem_size());
        let text_color = style.color;

        let run = TextRun {
            len: content.len(),
            font: style.font(),
            color: text_color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };

        let text: ShapedLine = window
            .text_system()
            .shape_line(content, font_size, &[run])
            .unwrap();

        let cursor = if focus_handle.is_focused(window)
            && editor.selected_range.start == editor.selected_range.end
        {
            let cursor_pos = text.x_for_index(editor.selected_range.start);
            Some(gpui::fill(
                gpui::Bounds::new(
                    point(bounds.left() + cursor_pos, bounds.top()),
                    size(px(2.), window.line_height()),
                ),
                gpui::blue(),
            ))
        } else {
            None
        };

        Some(TitleEditorPrepaintState { text, cursor })
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        bounds: gpui::Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus_handle = self.editor.read(cx).focus_handle.clone();

        window.handle_input(
            &focus_handle,
            ElementInputHandler::new(bounds, self.editor.clone()),
            cx,
        );

        if let Some(state) = prepaint.take() {
            state
                .text
                .paint(bounds.origin, window.line_height(), window, cx)
                .unwrap();

            if focus_handle.is_focused(window) {
                if let Some(cursor) = state.cursor {
                    window.paint_quad(cursor);
                }
            }
        }
    }
}

struct EditorView {
    editor: Entity<NoteEditor>,
}

struct PrepaintState {
    lines: Vec<(ShapedLine, usize)>,
    cursor: Option<PaintQuad>,
    selection: Vec<PaintQuad>,
}

impl IntoElement for EditorView {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for EditorView {
    type RequestLayoutState = ();
    type PrepaintState = PrepaintState;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();

        let content = self.editor.read(cx).content.clone();
        let line_count = content.split('\n').count();
        let height = window.line_height().0 * line_count as f32;

        style.size.height = px(height).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        bounds: gpui::Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let editor = self.editor.read(cx);
        let content = editor.content.clone();
        let selected_range = editor.selected_range.clone();
        let cursor = editor.cursor_offset();
        let style = window.text_style();
        let font_size = style.font_size.to_pixels(window.rem_size());
        let text_color = style.color;
        let mut shaped_lines = Vec::new();
        let mut offset = 0;
        let mut selections = Vec::new();
        let mut cursor_quad = None;
        let content_str = content.to_string();
        let lines: Vec<String> = content_str.split('\n').map(String::from).collect();

        for line_text in &lines {
            let line_len = line_text.len();
            let total_len = line_len
                + if offset + line_len < content.len() {
                    1
                } else {
                    0
                };

            let run = TextRun {
                len: line_text.len(),
                font: style.font(),
                color: text_color,
                background_color: None,
                underline: None,
                strikethrough: None,
            };

            let runs = if let Some(marked_range) = editor.marked_range.as_ref() {
                if offset + total_len > marked_range.start && offset < marked_range.end {
                    let marked_start = marked_range.start.saturating_sub(offset);
                    let marked_end = (marked_range.end - offset).min(line_len);

                    vec![
                        TextRun {
                            len: marked_start.min(line_len),
                            ..run.clone()
                        },
                        TextRun {
                            len: marked_end.saturating_sub(marked_start),
                            underline: Some(UnderlineStyle {
                                color: Some(run.color),
                                thickness: px(1.0),
                                wavy: false,
                            }),
                            ..run.clone()
                        },
                        TextRun {
                            len: line_len.saturating_sub(marked_end),
                            ..run.clone()
                        },
                    ]
                    .into_iter()
                    .filter(|run| run.len > 0)
                    .collect()
                } else {
                    vec![run.clone()]
                }
            } else {
                vec![run.clone()]
            };

            let shaped = window
                .text_system()
                .shape_line(SharedString::from(line_text), font_size, &runs)
                .unwrap();

            let line_index = shaped_lines.len();
            let line_y = bounds.top() + (line_index as f32 * window.line_height());

            if !selected_range.is_empty() {
                if offset + line_len >= selected_range.start && offset < selected_range.end {
                    let sel_start = (selected_range.start.saturating_sub(offset)).min(line_len);
                    let sel_end = (selected_range.end.saturating_sub(offset)).min(line_len);

                    if sel_start < sel_end {
                        selections.push(gpui::fill(
                            gpui::Bounds::from_corners(
                                point(bounds.left() + shaped.x_for_index(sel_start), line_y),
                                point(
                                    bounds.left() + shaped.x_for_index(sel_end),
                                    line_y + window.line_height(),
                                ),
                            ),
                            rgba(0x3311ff30),
                        ));
                    }
                }
            } else if offset <= cursor && cursor <= offset + total_len {
                let cursor_pos = if cursor > offset + line_len {
                    shaped.x_for_index(line_len)
                } else {
                    shaped.x_for_index(cursor - offset)
                };

                cursor_quad = Some(gpui::fill(
                    gpui::Bounds::new(
                        point(bounds.left() + cursor_pos, line_y),
                        size(px(2.), window.line_height()),
                    ),
                    gpui::blue(),
                ));
            }

            shaped_lines.push((shaped, offset));
            offset += total_len;
        }

        PrepaintState {
            lines: shaped_lines,
            cursor: cursor_quad,
            selection: selections,
        }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        bounds: gpui::Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus_handle = self.editor.read(cx).focus_handle.clone();
        window.handle_input(
            &focus_handle,
            ElementInputHandler::new(bounds, self.editor.clone()),
            cx,
        );

        for selection in prepaint.selection.drain(..) {
            window.paint_quad(selection);
        }

        for (i, (line, _)) in prepaint.lines.iter().enumerate() {
            let y_offset = i as f32 * window.line_height();
            let line_origin = point(bounds.origin.x, bounds.origin.y + y_offset);
            line.paint(line_origin, window.line_height(), window, cx)
                .unwrap();
        }

        if focus_handle.is_focused(window) {
            if let Some(cursor) = prepaint.cursor.take() {
                window.paint_quad(cursor);
            }
        }

        let lines = std::mem::take(&mut prepaint.lines);

        self.editor.update(cx, |editor, _cx| {
            if let Some((first_line, _)) = lines.first() {
                editor.last_layout = Some(first_line.clone());
            }
            editor.last_bounds = Some(bounds);
        });
    }
}

impl Focusable for NoteEditor {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for NoteEditor {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .size_full()
            .cursor(CursorStyle::IBeam)
            .track_focus(&self.focus_handle)
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_key_down(cx.listener(Self::on_key_down))
            .child(EditorView {
                editor: cx.entity().clone(),
            })
    }
}

impl NoteApp {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let db_path = get_db_path();
        println!("Database path: {:?}", db_path);

        if let Some(parent) = db_path.parent() {
            if !parent.exists() {
                match std::fs::create_dir_all(parent) {
                    Ok(_) => {
                        println!("Created directory: {:?}", parent);
                    }
                    Err(e) => {
                        eprintln!("Warning: Failed to create directory for database: {}", e);
                    }
                }
            }
        }

        let db = match Database::new(&db_path) {
            Ok(db) => {
                println!("Successfully opened database at: {:?}", db_path);
                Arc::new(db)
            }
            Err(e) => {
                eprintln!("Failed to initialize database at {:?}: {}", db_path, e);
                match Database::new(":memory:") {
                    Ok(memory_db) => {
                        eprintln!("Using in-memory database as fallback");
                        Arc::new(memory_db)
                    }
                    Err(e2) => {
                        eprintln!("Even in-memory database failed: {}", e2);
                        panic!("Database initialization completely failed");
                    }
                }
            }
        };

        let notes = match db.notes.list_notes() {
            Ok(notes) => {
                println!("Loaded {} notes from database", notes.len());
                println!(
                    "Note IDs: {:?}",
                    notes.iter().map(|n| n.id).collect::<Vec<_>>()
                );

                if notes.is_empty() {
                    let mut welcome_note = Note::new("Welcome".into());
                    welcome_note.content = "Welcome to your new note-taking app!".into();

                    if let Err(e) = db.notes.create_note(&welcome_note) {
                        eprintln!("Failed to create welcome note: {}", e);
                    } else {
                        println!("Created welcome note");

                        match db.notes.list_notes() {
                            Ok(notes_after_welcome) => {
                                println!(
                                    "After creating welcome note, there are {} notes in DB",
                                    notes_after_welcome.len()
                                );
                            }
                            Err(e) => {
                                eprintln!("Failed to verify welcome note creation: {}", e);
                            }
                        }
                    }
                    vec![welcome_note]
                } else {
                    notes
                }
            }
            Err(e) => {
                eprintln!("Failed to load notes: {}", e);
                let mut welcome_note = Note::new("Welcome".into());
                welcome_note.content = "Welcome to your new note-taking app!".into();
                vec![welcome_note]
            }
        };

        let active_note_id = notes.first().map(|note| note.id);
        let initial_title = notes
            .first()
            .map(|note| note.title.clone())
            .unwrap_or_default();

        let editor = cx.new(|cx| {
            let mut editor = NoteEditor {
                focus_handle: cx.focus_handle(),
                content: SharedString::from(""),
                selected_range: 0..0,
                selection_reversed: false,
                marked_range: None,
                last_layout: None,
                last_bounds: None,
                is_selecting: false,
                on_change: None,
            };

            if let Some(first_note) = notes.first() {
                editor.content = first_note.content.clone().into();
                editor.selected_range = editor.content.len()..editor.content.len();
            }

            editor
        });

        let title_editor = cx.new(|cx| TitleEditor {
            focus_handle: cx.focus_handle(),
            content: SharedString::from(initial_title.clone()),
            selected_range: initial_title.len()..initial_title.len(),
            selection_reversed: false,
            on_change: None,
        });

        let db_clone = db.clone();
        let active_note_id_clone = active_note_id;
        editor.update(cx, move |editor, cx| {
            editor.set_on_change(move |content, _cx| {
                if let Some(note_id) = active_note_id_clone {
                    if let Ok(Some(existing_note)) = db_clone.notes.get_note(&note_id.to_string()) {
                        if let Err(e) = db_clone.notes.update_note(&Note {
                            id: note_id,
                            title: existing_note.title,
                            content: content.clone(),
                            created_at: existing_note.created_at,
                        }) {
                            eprintln!("Failed to update note content: {}", e);
                        }
                    }
                }
            });
        });

        let db_clone = db.clone();
        let active_note_id_clone = active_note_id;
        let app_entity = cx.entity();
        let app_entity_clone = cx.entity();
        let active_note_id_for_on_change = active_note_id;
        title_editor.update(cx, move |editor, cx| {
            editor.set_on_change(move |new_title, _cx| {
                app_entity_clone.update(_cx, |app, cx| {
                    app.title_text = new_title.clone();

                    if let Some(note_id) = active_note_id_for_on_change {
                        for note in &mut app.notes {
                            if note.id == note_id {
                                note.title = new_title.clone();
                                break;
                            }
                        }
                    }
                    cx.notify();
                });
            });
        });

        println!("Dumping database at startup:");
        if let Err(e) = dump_db_contents() {
            println!("Failed to dump database: {}", e);
        }

        Self {
            db,
            notes,
            active_note_id,
            editor,
            content_focus_handle: cx.focus_handle(),
            title_edit_mode: false,
            title_text: initial_title,
            title_focus_handle: cx.focus_handle(),
            title_editor,
        }
    }

    pub fn dump_database(&self) {
        println!("Dumping database contents:");
        match dump_db_contents() {
            Ok(_) => println!("Database dump completed"),
            Err(e) => println!("Failed to dump database: {}", e),
        }
    }

    pub fn add_note(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let title = format!("Untitled {}", self.notes.len() + 1);
        let new_note = Note::new(title.clone());
        let new_id = new_note.id;

        println!("Adding new note with ID: {}", new_id);

        match self.db.notes.create_note(&new_note) {
            Ok(()) => {
                println!(
                    "Successfully saved new note to database with ID: {}",
                    new_id
                );
                self.notes.push(new_note);
                self.active_note_id = Some(new_id);

                self.editor.update(cx, |editor, cx| {
                    editor.set_content("", cx);
                });

                self.title_text = title;
                self.title_editor.update(cx, |editor, cx| {
                    editor.set_content(self.title_text.clone(), cx);
                });

                
                let db_clone = self.db.clone();
                let active_id = new_id;
                self.editor.update(cx, move |editor, cx| {
                    editor.set_on_change(move |content, _cx| {
                        if let Ok(Some(existing_note)) =
                            db_clone.notes.get_note(&active_id.to_string())
                        {
                            if let Err(e) = db_clone.notes.update_note(&Note {
                                id: active_id,
                                title: existing_note.title,
                                content: content.clone(),
                                created_at: existing_note.created_at,
                            }) {
                                eprintln!("Failed to update note content: {}", e);
                            }
                        }
                    });
                });

                self.dump_database();

                
                let editor_focus = self.editor.read(cx).focus_handle.clone();
                editor_focus.focus(window);

                cx.notify();
            }
            Err(e) => {
                eprintln!("Failed to save new note: {}", e);
            }
        }
    }

    pub fn delete_note(&mut self, id: Uuid, cx: &mut Context<Self>) {
        
        if let Err(e) = self.db.notes.delete_note(&id.to_string()) {
            eprintln!("Failed to delete note: {}", e);
            return;
        }

        
        self.notes.retain(|note| note.id != id);

        
        if self.active_note_id == Some(id) {
            self.active_note_id = self.notes.first().map(|note| note.id);

            if let Some(new_active_id) = self.active_note_id {
                self.set_active_note(new_active_id, cx);
            } else {
                
                self.editor.update(cx, |editor, cx| {
                    editor.set_content("", cx);
                });
                self.title_text = String::new();
                self.title_editor.update(cx, |editor, cx| {
                    editor.set_content("", cx);
                });
            }
        }

        cx.notify();
    }

    pub fn set_active_note(&mut self, id: Uuid, cx: &mut Context<Self>) {
        let db_clone = self.db.clone();
        let active_id = id;

        let fresh_note = self.db.notes.get_note(&id.to_string()).ok().flatten();

        if let Some(note) = fresh_note.clone() {
            for cached_note in &mut self.notes {
                if cached_note.id == id {
                    *cached_note = note.clone();
                    break;
                }
            }

            self.active_note_id = Some(id);
            self.title_edit_mode = false;
            self.title_text = note.title.clone();

            let content = note.content.clone();
            self.editor.update(cx, move |editor, cx| {
                editor.set_content(content, cx);
            });

            self.editor.update(cx, move |editor, cx| {
                editor.set_on_change(move |content, _cx| {
                    if let Ok(Some(existing_note)) = db_clone.notes.get_note(&active_id.to_string())
                    {
                        if let Err(e) = db_clone.notes.update_note(&Note {
                            id: active_id,
                            title: existing_note.title,
                            content: content.clone(),
                            created_at: existing_note.created_at,
                        }) {
                            eprintln!("Failed to update note content: {}", e);
                        }
                    }
                });
            });
        } else {
            let fallback_note = self.notes.iter().find(|n| n.id == id).cloned();

            if let Some(note) = fallback_note {
                self.active_note_id = Some(id);
                self.title_edit_mode = false;
                self.title_text = note.title.clone();

                let content = note.content.clone();
                self.editor.update(cx, move |editor, cx| {
                    editor.set_content(content, cx);
                });

                self.editor.update(cx, move |editor, cx| {
                    editor.set_on_change(move |content, _cx| {
                        if let Ok(Some(existing_note)) =
                            db_clone.notes.get_note(&active_id.to_string())
                        {
                            if let Err(e) = db_clone.notes.update_note(&Note {
                                id: active_id,
                                title: existing_note.title,
                                content: content.clone(),
                                created_at: existing_note.created_at,
                            }) {
                                eprintln!("Failed to update note content: {}", e);
                            }
                        }
                    });
                });
            }
        }

        cx.notify();
    }

    pub fn get_active_note(&self) -> Option<&Note> {
        if let Some(id) = self.active_note_id {
            self.notes.iter().find(|note| note.id == id)
        } else {
            None
        }
    }

    pub fn toggle_title_edit_mode(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let current_edit_mode = self.title_edit_mode;
        let new_edit_mode = !current_edit_mode;

        let title_to_use: Option<String> = if let Some(active_note) = self.get_active_note() {
            Some(active_note.title.clone())
        } else {
            None
        };

        if let Some(title_ref) = &title_to_use {
            self.title_edit_mode = new_edit_mode;

            if new_edit_mode {
                self.title_text = title_ref.clone();
                self.title_editor.update(cx, |editor, cx| {
                    editor.set_content(title_ref.clone(), cx);
                });
            } else {
                let title_content = self.title_editor.read(cx).content.to_string();

                if title_content.trim().is_empty() {
                    self.title_text = title_ref.clone();
                    self.title_editor.update(cx, |editor, cx| {
                        editor.set_content(title_ref.clone(), cx);
                    });
                    if let Some(active_id) = self.active_note_id {
                        for note in &mut self.notes {
                            if note.id == active_id {
                                note.title = title_ref.clone();
                                break;
                            }
                        }
                    }
                } else {
                    self.title_text = title_content;
                    self.save_title(cx);
                }
            }

            cx.notify();

            let main_editor_focus = self.editor.read(cx).focus_handle.clone();
            main_editor_focus.focus(window);
        }
    }

    pub fn save_title(&mut self, cx: &mut Context<Self>) {
        if self.title_text.trim().is_empty() {
            if let Some(note_id) = self.active_note_id {
                if let Ok(Some(existing_note)) = self.db.notes.get_note(&note_id.to_string()) {
                    let default_title = "Untitled Note".to_string();

                    
                    let final_title = if existing_note.title.trim().is_empty() {
                        println!("Existing title in database is empty, using default title");
                        default_title
                    } else {
                        existing_note.title.clone()
                    };

                    self.title_text = final_title.clone();
                    self.title_editor.update(cx, |editor, cx| {
                        editor.set_content(final_title.clone(), cx);
                    });

                    
                    if existing_note.title.trim().is_empty() {
                        if let Err(e) = self.db.notes.update_note(&Note {
                            id: note_id,
                            title: final_title.clone(),
                            content: existing_note.content.clone(),
                            created_at: existing_note.created_at,
                        }) {
                            eprintln!("Failed to update note title: {}", e);
                        } else {
                            
                            for note in &mut self.notes {
                                if note.id == note_id {
                                    note.title = final_title;
                                    break;
                                }
                            }
                        }
                    }

                    cx.notify();
                }
            }
            return;
        }

        if let Some(note_id) = self.active_note_id {
            if let Ok(Some(existing_note)) = self.db.notes.get_note(&note_id.to_string()) {
                if existing_note.title != self.title_text {
                    if let Err(e) = self.db.notes.update_note(&Note {
                        id: note_id,
                        title: self.title_text.clone(),
                        content: existing_note.content.clone(),
                        created_at: existing_note.created_at,
                    }) {
                        eprintln!("Failed to update note title: {}", e);
                    } else {
                        for note in &mut self.notes {
                            if note.id == note_id {
                                note.title = self.title_text.clone();
                                break;
                            }
                        }
                    }
                }
            }
        }
    }

    pub fn handle_title_key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if event.keystroke.key == "enter" {
            let title_content = self.title_editor.read(cx).content.to_string();
            if title_content.trim().is_empty() {
                if let Some(active_note) = self.get_active_note() {
                    self.title_editor.update(cx, |editor, cx| {
                        editor.set_content(active_note.title.clone(), cx);
                    });
                    self.on_title_blur(window, cx);
                }
            } else {
                self.toggle_title_edit_mode(window, cx);
            }
        } else if event.keystroke.key == "escape" {
            self.on_title_blur(window, cx);
        }
    }

    
    
    pub fn on_title_blur(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.title_edit_mode {
            println!("Title edit mode is active, handling blur event");
            let title_content;
            let should_revert;
            let original_title = self.get_active_note().map(|note| note.title.clone());

            {
                title_content = self.title_editor.read(cx).content.to_string();
                should_revert = title_content.trim().is_empty();
                println!(
                    "Title content: '{}', should revert: {}",
                    title_content, should_revert
                );
            }

            if should_revert {
                if let Some(title) = original_title {
                    let default_title = "Untitled Note".to_string();
                    let final_title = if title.trim().is_empty() {
                        println!("Original title is empty, using default title");
                        default_title
                    } else {
                        println!("Reverting to original title: '{}'", title);
                        title.clone()
                    };

                    self.title_text = final_title.clone();
                    self.title_editor.update(cx, |editor, cx| {
                        editor.set_content(final_title.clone(), cx);
                    });

                    if let Some(active_id) = self.active_note_id {
                        for note in &mut self.notes {
                            if note.id == active_id {
                                note.title = final_title.clone();
                                println!("Updated note title in memory for note ID: {}", active_id);
                                break;
                            }
                        }

                        
                        if title.trim().is_empty() {
                            if let Ok(Some(existing_note)) =
                                self.db.notes.get_note(&active_id.to_string())
                            {
                                if let Err(e) = self.db.notes.update_note(&Note {
                                    id: active_id,
                                    title: final_title,
                                    content: existing_note.content.clone(),
                                    created_at: existing_note.created_at,
                                }) {
                                    eprintln!("Failed to update note title: {}", e);
                                }
                            }
                        }
                    }
                }
            } else {
                println!("Saving new title: '{}'", title_content);
                self.title_text = title_content;
                self.save_title(cx);
            }

            self.title_edit_mode = false;
            println!("Exiting title edit mode");
            cx.notify();

            let main_editor_focus = self.editor.read(cx).focus_handle.clone();
            println!("Focusing main editor");
            main_editor_focus.focus(window);
        }
    }
}

impl Focusable for NoteApp {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.content_focus_handle.clone()
    }
}

impl Render for NoteApp {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        
        println!("Render called, checking for notes to delete");
        match NOTE_TO_DELETE.lock() {
            Ok(mut note_to_delete) => {
                println!("Successfully locked NOTE_TO_DELETE mutex in render");
                if let Some(id) = note_to_delete.take() {
                    println!("Found note to delete with ID: {}", id);
                    self.delete_note(id, cx);
                    println!("Deletion completed for note: {}", id);
                } else {
                    println!("No notes queued for deletion");
                }
            },
            Err(e) => {
                println!("Failed to lock NOTE_TO_DELETE mutex in render: {:?}", e);
            }
        }
    
        div()
            .flex()
            .bg(rgb(0xf5f5f5))
            .size_full()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|view, event, window, cx| {
                    if view.title_edit_mode {
                        let title_focused =
                            view.title_editor.read(cx).focus_handle.is_focused(window);
                        if !title_focused {
                            view.on_title_blur(window, cx);
                        }
                    }
                }),
            )
            .child(self.render_sidebar(cx))
            .child(self.render_content(cx))
    }
}

impl NoteApp {
    fn render_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let notes = self.notes.clone();
        let active_note_id = self.active_note_id;

        div()
            .flex()
            .flex_col()
            .w(px(200.0))
            .h_full()
            .bg(rgb(0xf0f0f0))
            .px_2()
            .border_r_1()
            .rounded_lg()
            .border_color(rgb(0xE0E0E0))
            .child(
                div().flex().justify_end().items_center().p_2().child(
                    div()
                        .size(px(28.0))
                        .flex()
                        .justify_center()
                        .items_center()
                        .bg(rgb(0x4287f5))
                        .text_color(rgb(0xffffff))
                        .text_lg()
                        .font_weight(FontWeight::BOLD)
                        .rounded_full()
                        .cursor_pointer()
                        .hover(|s| s.bg(rgb(0x3276e4)))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|view, _: &MouseDownEvent, window, cx| {
                                view.add_note(window, cx);
                            }),
                        )
                        .child("+"),
                ),
            )
            .child(
                div().flex().flex_col().p_2().children(
                    notes
                        .iter()
                        .map(|note| {
                            let is_active = active_note_id == Some(note.id);
                            let note_id = note.id;

                            div()
                                .flex()
                                .justify_between()
                                .items_center()
                                .bg(if is_active {
                                    rgb(0xdddddd)
                                } else {
                                    rgb(0xf0f0f0)
                                })
                                .child(
                                    div()
                                        .flex_grow()
                                        .font_weight(if is_active {
                                            FontWeight::BOLD
                                        } else {
                                            FontWeight::NORMAL
                                        })
                                        .cursor_pointer()
                                        .on_mouse_down(
                                            MouseButton::Left,
                                            cx.listener(
                                                move |view, event: &MouseDownEvent, window, cx| {
                                                    view.set_active_note(note_id, cx);
                                                },
                                            ),
                                        )
                                        .on_mouse_down(
                                            MouseButton::Right,
                                            cx.listener(
                                                move |view, event: &MouseDownEvent, window, cx| {
                                                    let mut menu = ContextMenu::new();
                                                    menu.add_delete_item("Delete", note_id);

                                                    
                                                    let db_clone = view.db.clone();
                                                    menu.set_direct_delete_callback(move |uuid| {
                                                        println!("Executing direct delete for note: {}", uuid);
                                                        if let Err(e) = db_clone.notes.delete_note(&uuid.to_string()) {
                                                            println!("Direct delete failed: {}", e);
                                                            return false;
                                                        }
                                                        println!("Note {} was deleted directly!", uuid);
                                                        return true;
                                                    });

                                                    
                                                    let callback = Box::new(move |action| {
                                                        match action {
                                                            MenuAction::Delete(delete_note_id) => {
                                                                println!(
                                                                    "Menu action: Delete note {}",
                                                                    delete_note_id
                                                                );
                                                                
                                                                
                                                                if let Ok(mut guard) = NOTE_TO_DELETE.lock() {
                                                                    *guard = Some(delete_note_id);
                                                                    println!("Set note {} for deletion in the global mutex", delete_note_id);
                                                                }
                                                                
                                                                
                                                                unsafe {
                                                                    let dispatch_queue = objc::class!(NSOperationQueue);
                                                                    let main_queue: cocoa::base::id = msg_send![dispatch_queue, mainQueue];
                                                                    let block = ConcreteBlock::new(|| {
                                                                        println!("Attempting to force UI refresh after deletion signal");
                                                                        
                                                                        
                                                                        let app: cocoa::base::id = msg_send![objc::class!(NSApplication), sharedApplication];
                                                                        let _: () = msg_send![app, updateWindows];
                                                                        
                                                                    }).copy();
                                                                    let _: () = msg_send![main_queue, addOperationWithBlock:block];
                                                                }
                                                            }
                                                        }
                                                    });

                                                    menu.show_at_position(
                                                        event.position.x.0 as f64,
                                                        event.position.y.0 as f64,
                                                        callback,
                                                    );
                                                },
                                            ),
                                        )
                                        .child(note.title.clone()),
                                )
                        })
                        .collect::<Vec<_>>(),
                ),
            )
    }

    fn render_content(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let active_note = self.get_active_note().cloned();

        div()
            .id("content-area")
            .flex()
            .flex_col()
            .flex_grow()
            .h_full()
            .overflow_y_scroll()
            .bg(rgb(0xffffff))
            .child(if let Some(note) = active_note {
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .p_4()
                    .w_full()
                    .child(if self.title_edit_mode {
                        div()
                            .flex()
                            .rounded_md()
                            .font_weight(FontWeight::BOLD)
                            .text_xl()
                            .on_key_down(cx.listener(Self::handle_title_key_down))
                            .child(self.title_editor.clone())
                    } else {
                        div()
                            .font_weight(FontWeight::BOLD)
                            .text_xl()
                            .cursor_pointer()
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|view, _event, window, cx| {
                                    view.toggle_title_edit_mode(window, cx);
                                    let editor_handle =
                                        view.title_editor.read(cx).focus_handle.clone();
                                    editor_handle.focus(window);
                                }),
                            )
                            .child(note.title)
                    })
                    .child(
                        div()
                            .id("editor-area")
                            .w_full()
                            .py_2()
                            .font_family("monospace")
                            .text_size(px(16.0))
                            .line_height(px(LINE_HEIGHT))
                            .child(self.editor.clone()),
                    )
            } else {
                div().p_4().child("Select a note or create a new one")
            })
    }
}
