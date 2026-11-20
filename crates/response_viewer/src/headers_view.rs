//! The response headers table: every field, duplicates kept, in wire order
//! (REQ-RESP-05).
//!
//! The rows are a snapshot taken when the response arrives ([`HeadersView::set_headers`]);
//! render lays them out through [`VirtualList`], so a response with hundreds of header
//! fields only shapes the rows the viewport can show.

use std::collections::HashMap;
use std::sync::Arc;

use http_engine::Header;
use ui::prelude::*;

use crate::child_id;

/// One header field as the table shows it: name and value, as they arrived.
#[derive(Clone, Debug)]
pub struct HeaderRow {
    /// The field name, as it went on the wire.
    pub name: SharedString,
    /// The field value.
    pub value: SharedString,
    /// Which occurrence of `name` this row is. A duplicate `Set-Cookie` keeps its own row
    /// and its own element identity.
    occurrence: usize,
}

impl HeaderRow {
    /// This row's element identity under `base`: its name and which occurrence of that
    /// name it is — stable across scrolling and independent of the row's position, so
    /// focus and hover never jump to a different field.
    fn key(&self, base: &ElementId) -> ElementId {
        child_id(base, format!("{}#{}", self.name, self.occurrence))
    }
}

/// The headers table: the rows of one response, and the two-column layout that draws
/// them.
pub struct HeadersView {
    rows: Arc<Vec<HeaderRow>>,
}

impl HeadersView {
    /// An empty table, waiting for a response.
    pub fn new(_cx: &mut Context<Self>) -> Self {
        Self {
            rows: Arc::new(Vec::new()),
        }
    }

    /// Show these header fields exactly as they arrived: every occurrence, in order. The
    /// rows are rebuilt here (never in `render`), so render stays a pure layout of what
    /// this snapshot holds.
    pub fn set_headers(&mut self, headers: &[Header], cx: &mut Context<Self>) {
        let mut occurrences: HashMap<String, usize> = HashMap::new();
        let rows = headers
            .iter()
            .map(|header| {
                let counter = occurrences.entry(header.name.clone()).or_default();
                let occurrence = *counter;
                *counter += 1;
                HeaderRow {
                    name: header.name.as_str().into(),
                    value: header.value.as_str().into(),
                    occurrence,
                }
            })
            .collect();
        self.rows = Arc::new(rows);
        cx.notify();
    }

    /// The rows the table draws, top to bottom.
    pub fn rows(&self) -> &[HeaderRow] {
        &self.rows
    }
}

impl Render for HeadersView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let rows = Arc::clone(&self.rows);
        // One base id per entity, so two viewers in one frame never share element ids.
        let base = ElementId::from(("headers-view", cx.entity_id()));
        v_flex()
            .size_full()
            .child(
                h_flex()
                    .w_full()
                    .gap(DynamicSpacing::Base04.rems(cx))
                    .px(DynamicSpacing::Base04.rems(cx))
                    .py(DynamicSpacing::Base02.rems(cx))
                    .border_b_1()
                    .border_color(cx.theme().colors().border_variant)
                    .child(column_heading("Name"))
                    .child(column_heading("Value")),
            )
            .map(|this| {
                if rows.is_empty() {
                    this.child(empty_state())
                } else {
                    let row_keys = Arc::clone(&rows);
                    let key_base = base.clone();
                    this.child(VirtualList::new(
                        child_id(&base, "rows"),
                        rows.len(),
                        move |index| match row_keys.get(index) {
                            Some(row) => row.key(&key_base),
                            None => child_id(&key_base, format!("row-{index}")),
                        },
                        move |index, _window, cx| render_row(&rows, index, &base, cx),
                    ))
                }
            })
    }
}

/// The `Name` / `Value` headings above the rows.
fn column_heading(heading: &'static str) -> impl IntoElement {
    div().flex_1().min_w_0().child(
        Label::new(heading)
            .size(LabelSize::XSmall)
            .color(Color::Muted),
    )
}

/// One header row: name and value, each truncating with its full value in a tooltip.
fn render_row(rows: &[HeaderRow], index: usize, base: &ElementId, cx: &mut App) -> AnyElement {
    let Some(row) = rows.get(index) else {
        return div().into_any_element();
    };
    let row_id = row.key(base);
    h_flex()
        .w_full()
        .gap(DynamicSpacing::Base04.rems(cx))
        .px(DynamicSpacing::Base04.rems(cx))
        .py(DynamicSpacing::Base02.rems(cx))
        .border_b_1()
        .border_color(cx.theme().colors().border_variant)
        .child(cell(child_id(&row_id, "name"), row.name.clone(), cx))
        .child(cell(child_id(&row_id, "value"), row.value.clone(), cx))
        .into_any_element()
}

/// One header cell: the content font, truncated with the full value behind a tooltip.
fn cell(id: ElementId, text: SharedString, cx: &App) -> impl IntoElement {
    div()
        .id(id)
        .flex_1()
        .min_w_0()
        .font_buffer(cx)
        .text_ui_sm(cx)
        .text_color(cx.theme().colors().text)
        .tooltip(Tooltip::text(text.clone()).builder())
        .child(div().w_full().min_w_0().truncate().child(text))
}

/// The state where there is nothing to list: before the first response, or a response
/// that carried no header fields at all.
fn empty_state() -> impl IntoElement {
    v_flex()
        .size_full()
        .justify_center()
        .items_center()
        .child(Label::new("This response has no headers.").color(Color::Muted))
}

#[cfg(test)]
#[path = "headers_view_tests.rs"]
mod tests;
