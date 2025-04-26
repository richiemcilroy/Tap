use crate::models::Note;
use gpui::{FontWeight, Render, Window, div, prelude::*, rgb};

pub struct NoteContent {
    note: Option<Note>,
    content: String,
}

impl Render for NoteContent {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .flex_grow()
            .h_full()
            .bg(rgb(0xffffff))
            .child(if let Some(note) = &self.note {
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .p_4()
                    .size_full()
                    .child(
                        div()
                            .font_weight(FontWeight::BOLD)
                            .text_lg()
                            .child(note.title.clone()),
                    )
                    .child(div().flex_grow().size_full().child(self.content.clone()))
            } else {
                div().p_4().child("Select a note or create a new one")
            })
    }
}
