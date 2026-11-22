//! The exported Console panel id (#166): `console::panel::PANEL_ID` was module-private,
//! so `crates/pustman/tests/wire_console.rs` could only spell the dock id as the literal
//! `"console"`. The constant is exported now and named here from outside the crate the
//! way the wire test would name it; that test's own literal is frozen and stays as it is.

use console::{ConsolePanel, PANEL_ID};
use gpui::TestAppContext;
use ui::test_support::init_test;
use workspace::Panel;

#[gpui::test]
fn exported_panel_id_is_the_id_the_panel_reports(cx: &mut TestAppContext) {
    cx.update(|cx| {
        init_test(cx);
        text_input::init(cx);
        console::init(cx);
    });
    let (panel, _cx) = cx.add_window_view(ConsolePanel::new);

    cx.read(|cx| {
        assert_eq!(
            Panel::panel_id(panel.read(cx)),
            PANEL_ID,
            "the panel reports exactly the id the dock registers it under"
        );
    });
    assert_eq!(
        PANEL_ID, "console",
        "the exported id is the one the frozen wire test spells as a literal"
    );
}
