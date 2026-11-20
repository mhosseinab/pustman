//! The Hex view (REQ-RESP-02): a body's bytes become `offset | hex | ascii` rows — the
//! offset counts bytes in eight hex digits, one row holds [`BYTES_PER_ROW`] of them, and
//! Hex mode draws those rows for the body on screen, however it arrived.

use std::io::Write as _;
use std::sync::Arc;

use gpui::TestAppContext;
use http_engine::{Response, ResponseBody};
use ui::test_support::{draw_window, init_test};

use super::{BYTES_PER_ROW, HexRows};
use crate::body_view::{BodyMode, BodyView};

#[gpui::test]
fn hex_view_formats_offsets(cx: &mut TestAppContext) {
    // Eighteen ASCII bytes keep the expected column strings readable: one full row, one
    // partial.
    let rows = HexRows::new(Arc::new(b"0123456789abcdefXY".to_vec()));
    assert_eq!(rows.row_count(), 2, "one row per sixteen bytes, rounded up");

    let first = rows.row(0).expect("the first row exists");
    assert_eq!(first.offset, "00000000");
    assert_eq!(
        first.hex,
        "30 31 32 33 34 35 36 37  38 39 61 62 63 64 65 66"
    );
    assert_eq!(first.ascii, "0123456789abcdef");

    let second = rows.row(1).expect("the second row exists");
    assert_eq!(second.offset, "00000010", "offsets count bytes, not rows");
    assert_eq!(
        second.hex.trim_end(),
        "58 59",
        "a partial row shows just its bytes"
    );
    assert_eq!(second.ascii.trim_end(), "XY");
    assert_eq!(
        second.hex.len(),
        first.hex.len(),
        "a partial row is padded so the columns still line up"
    );
    assert_eq!(second.ascii.len(), BYTES_PER_ROW);
    assert!(rows.row(2).is_none(), "no row runs past the body's bytes");

    // The offset keeps counting where the rows run into the thousands.
    let deep = HexRows::new(Arc::new(vec![b'x'; 0x100 * BYTES_PER_ROW + 1]));
    assert_eq!(deep.row_count(), 0x101);
    assert_eq!(
        deep.row(0x100)
            .expect("the row at offset 0x1000 exists")
            .offset,
        "00001000"
    );

    // Hex mode draws those rows from the body on screen — here one read back from disk
    // off the foreground.
    cx.update(init_test);
    let (view, cx) = cx.add_window_view(|_, cx| BodyView::new(cx));
    let mut file = tempfile::NamedTempFile::new().expect("the temp file must be created");
    file.write_all(b"AB").expect("the temp file must write");
    view.update(cx, |view, cx| {
        view.set_body(
            Arc::new(Response::new(
                200,
                "OK",
                ResponseBody::File {
                    path: file.path().to_owned(),
                    prefix: Vec::new(),
                },
            )),
            cx,
        );
        view.set_mode(BodyMode::Hex, cx);
    });
    cx.run_until_parked();
    draw_window(cx);

    let shown = view.read_with(cx, |view, _| view.hex_rows());
    assert_eq!(shown.row_count(), 1, "the file's two bytes fill one row");
    let row = shown.row(0).expect("the row exists");
    assert_eq!(row.offset, "00000000");
    assert_eq!(row.ascii.trim_end(), "AB", "the row shows the file's bytes");
}
