//! The Hex view (REQ-RESP-02): a body's raw bytes as hexdump rows — `offset | hex |
//! ascii`, one row per [`BYTES_PER_ROW`] bytes.
//!
//! A body can fill hundreds of thousands of rows, so rows are formatted lazily: the pane
//! draws a [`VirtualList`] and only the rows on screen are ever formatted. A row is
//! identified by its byte offset, which never changes for a body, so scroll and hover
//! state survive scrolling (`references/performance.md`).

use std::sync::Arc;

use gpui::{AnyElement, App, ElementId, IntoElement, RenderOnce, Window};
use ui::prelude::*;

use crate::child_id;

/// The bytes one hexdump row shows.
pub const BYTES_PER_ROW: usize = 16;

/// The full width of the `hex` column: two digits per byte, one space between bytes and
/// the traditional extra gap after the eighth byte.
const HEX_COLUMN_WIDTH: usize = 3 * BYTES_PER_ROW;

/// One hexdump row: where its bytes start, the bytes as hex, and the printable ASCII
/// they carry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HexRow {
    /// The offset of the row's first byte: eight lowercase hex digits.
    pub offset: SharedString,
    /// The row's bytes as two hex digits each, padded so every row's columns line up.
    pub hex: SharedString,
    /// The row's bytes as printable ASCII (`.` for the rest), padded to [`BYTES_PER_ROW`].
    pub ascii: SharedString,
}

/// A body's bytes split into hexdump rows, one row formatted at a time.
pub struct HexRows {
    bytes: Arc<Vec<u8>>,
}

impl HexRows {
    /// The rows `bytes` fill; the last row may be partial.
    pub fn new(bytes: Arc<Vec<u8>>) -> Self {
        Self { bytes }
    }

    /// How many rows the body fills.
    pub fn row_count(&self) -> usize {
        self.bytes.len().div_ceil(BYTES_PER_ROW)
    }

    /// The row at `index`, formatted on demand — [`None`] past the body's end.
    pub fn row(&self, index: usize) -> Option<HexRow> {
        let start = index.checked_mul(BYTES_PER_ROW)?;
        if start >= self.bytes.len() {
            return None;
        }
        let tail = self.bytes.get(start..)?;
        // `count` is at most `tail.len()`, so this slice cannot panic. It also keeps a
        // partial last row short instead of padding the data.
        let count = tail.len().min(BYTES_PER_ROW);
        let row_bytes = &tail[..count];
        Some(HexRow {
            offset: format!("{start:08x}").into(),
            hex: hex_column(row_bytes),
            ascii: ascii_column(row_bytes),
        })
    }
}

/// The `hex` column: the row's bytes as two lowercase hex digits each, with the
/// hexdump gap after half the row (eight bytes = [`BYTES_PER_ROW`] hex digits) and
/// padded to [`HEX_COLUMN_WIDTH`] so partial rows still line up.
fn hex_column(bytes: &[u8]) -> SharedString {
    // `hex::encode` emits exactly two digits per byte; this loop only lays them out.
    let encoded = hex::encode(bytes);
    let mut column = String::with_capacity(HEX_COLUMN_WIDTH);
    for (index, digit) in encoded.chars().enumerate() {
        if index > 0 && index % 2 == 0 {
            column.push(' ');
        }
        if index == BYTES_PER_ROW {
            column.push(' ');
        }
        column.push(digit);
    }
    while column.len() < HEX_COLUMN_WIDTH {
        column.push(' ');
    }
    column.into()
}

/// The `ascii` column: the row's printable ASCII bytes as characters, `.` for the rest,
/// padded to [`BYTES_PER_ROW`] so the columns line up.
fn ascii_column(bytes: &[u8]) -> SharedString {
    let mut column = String::with_capacity(BYTES_PER_ROW);
    for byte in bytes {
        let printable = byte.is_ascii_graphic() || *byte == b' ';
        column.push(if printable { char::from(*byte) } else { '.' });
    }
    while column.len() < BYTES_PER_ROW {
        column.push(' ');
    }
    column.into()
}

/// The Hex pane: a virtualized hexdump of a body's bytes.
#[derive(IntoElement)]
pub struct HexView {
    base: ElementId,
    rows: HexRows,
}

impl HexView {
    /// The hexdump of `bytes`, rows keyed under `base`.
    pub fn new(base: impl Into<ElementId>, bytes: Arc<Vec<u8>>) -> Self {
        Self {
            base: base.into(),
            rows: HexRows::new(bytes),
        }
    }
}

impl RenderOnce for HexView {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let base = self.base;
        let rows = Arc::new(self.rows);
        VirtualList::new(
            child_id(&base, "rows"),
            rows.row_count(),
            {
                let base = base.clone();
                move |index| row_id(&base, index)
            },
            {
                let base = base.clone();
                let rows = Arc::clone(&rows);
                move |index, _window, cx| render_row(&rows, index, &base, cx)
            },
        )
    }
}

/// A row's element id: its byte offset, which never changes for a body.
fn row_id(base: &ElementId, index: usize) -> ElementId {
    child_id(
        base,
        format!("row-{:08x}", index.saturating_mul(BYTES_PER_ROW)),
    )
}

/// One hexdump row: `offset | hex | ascii`, in the body font the Raw lines use.
fn render_row(rows: &HexRows, index: usize, base: &ElementId, cx: &mut App) -> AnyElement {
    let Some(row) = rows.row(index) else {
        return div().into_any_element();
    };
    div()
        .id(row_id(base, index))
        .w_full()
        .min_w_0()
        .overflow_hidden()
        .px(DynamicSpacing::Base04.rems(cx))
        .py(DynamicSpacing::Base01.rems(cx))
        .font_buffer(cx)
        .text_buffer(cx)
        .child(
            h_flex()
                .gap(DynamicSpacing::Base04.rems(cx))
                .child(
                    div()
                        .text_color(cx.theme().colors().text_muted)
                        .child(row.offset),
                )
                .child(div().text_color(cx.theme().colors().text).child(row.hex))
                .child(div().text_color(cx.theme().colors().text).child(row.ascii)),
        )
        .into_any_element()
}

#[cfg(test)]
#[path = "hex_view_tests.rs"]
mod tests;
