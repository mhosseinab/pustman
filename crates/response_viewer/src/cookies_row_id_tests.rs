//! An unparseable `Set-Cookie` row shows the header's raw text in its Name cell — which
//! embeds the cookie value (PRD §7) — but that text must stay out of the row's element
//! ids (#154): ids surface in debug-build a11y `NodeDebugInfo` and the opt-in `.debug()`
//! overlay, so an unparseable row's id is derived from its text, never spelled with it.

use gpui::ElementId;
use http_engine::Header;

use super::parse_set_cookies;

/// The element ids of these rows, as the table builds them.
fn row_ids(headers: &[Header]) -> Vec<String> {
    let base = ElementId::from("cookies-tab");
    parse_set_cookies(headers)
        .iter()
        .map(|row| row.key(&base).to_string())
        .collect()
}

#[test]
fn an_unparseable_row_id_never_carries_the_raw_header_text() {
    let rows = parse_set_cookies(&[Header::new("Set-Cookie", "foo bar supersecrettoken")]);
    let row = rows.first().expect("the broken header still gets a row");
    let id = row.key(&ElementId::from("cookies-tab")).to_string();

    assert!(
        !id.contains("supersecrettoken") && !id.contains("foo bar"),
        "the raw header text stays out of the element id, got {id:?}"
    );
    assert!(
        row.name.contains("supersecrettoken"),
        "the raw text still shows in the Name cell, got {:?}",
        row.name
    );
}

#[test]
fn every_unparseable_row_keeps_its_own_id() {
    let ids = row_ids(&[
        Header::new("Set-Cookie", "=supersecrettoken"),
        Header::new("Set-Cookie", "=supersecrettoken"),
        Header::new("Set-Cookie", "=another-token"),
    ]);

    assert_eq!(ids.len(), 3, "every broken header still gets a row");
    for (index, id) in ids.iter().enumerate() {
        for other in &ids[index + 1..] {
            assert_ne!(id, other, "each unparseable row keeps its own identity");
        }
    }
}

#[test]
fn a_parseable_row_id_is_the_name_and_its_occurrence() {
    let ids = row_ids(&[
        Header::new("Set-Cookie", "sid=token-REDACTED-example"),
        Header::new("Set-Cookie", "sid=token-REDACTED-example"),
    ]);

    assert_eq!(ids.len(), 2, "two Set-Cookie headers get two rows");
    let [first, second] = ids.as_slice() else {
        panic!("the rows above are two, got {ids:?}");
    };
    assert!(
        first.contains("sid#0") && second.contains("sid#1"),
        "a parseable row's id stays the cookie name and its occurrence, got {ids:?}"
    );
}
