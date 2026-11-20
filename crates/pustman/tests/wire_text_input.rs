//! The text input wire: the app's init order must run `text_input::init`, or every text
//! field in the app ignores Backspace, Delete, the arrows and the clipboard keys — the
//! widgets only answer to bindings that init registers.

use gpui::TestAppContext;
use pustman::StoreLocation;

#[gpui::test]
fn wire_text_input_init_registers_the_editing_keys(cx: &mut TestAppContext) {
    cx.update(|cx| pustman::init_with_store(cx, StoreLocation::InMemory));
    cx.run_until_parked();

    cx.update(|cx| {
        let keymap = cx.key_bindings();
        let keymap = keymap.borrow();
        assert!(
            keymap
                .bindings_for_action(&text_input::input::Backspace)
                .next()
                .is_some(),
            "the app's init order binds Backspace for text fields"
        );
        assert!(
            keymap
                .bindings_for_action(&text_input::input::Left)
                .next()
                .is_some(),
            "the app's init order binds Left for text fields"
        );
    });
}
