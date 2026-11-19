//! The menu answers the keyboard through the real `"Menu"` bindings
//! (`crate::init`): Down and Up move the highlight — wrapping, and stepping over
//! separators and disabled entries — Enter runs the highlighted entry exactly once and
//! dismisses, Escape dismisses without running anything.

use std::cell::Cell;
use std::rc::Rc;

use futures::channel::mpsc::UnboundedReceiver;
use gpui::{Entity, Focusable, TestAppContext, VisualTestContext};

use crate::components::context_menu::{ContextMenu, ContextMenuEvent};
use crate::test_support::{draw_window, init_test, press_key};

/// The menu under test and what its two runnable entries see.
struct MenuHarness {
    menu: Entity<ContextMenu>,
    opened: Rc<Cell<usize>>,
    duplicated: Rc<Cell<usize>>,
}

/// A menu of `Open`, `Duplicate`, a separator and a disabled `Delete` in its own window.
fn menu_with(cx: &mut TestAppContext) -> (MenuHarness, &mut VisualTestContext) {
    let opened = Rc::new(Cell::new(0));
    let duplicated = Rc::new(Cell::new(0));
    let (menu, cx) = cx.add_window_view({
        let opened = opened.clone();
        let duplicated = duplicated.clone();
        move |_, cx| {
            ContextMenu::new(cx)
                .entry("Open", {
                    let opened = opened.clone();
                    move |_, _, _| opened.set(opened.get() + 1)
                })
                .entry("Duplicate", {
                    let duplicated = duplicated.clone();
                    move |_, _, _| duplicated.set(duplicated.get() + 1)
                })
                .separator()
                .disabled_entry("Delete")
        }
    });
    (
        MenuHarness {
            menu,
            opened,
            duplicated,
        },
        cx,
    )
}

/// Focus the menu and paint the frame its key handlers register on.
fn focus_menu(menu: &Entity<ContextMenu>, cx: &mut VisualTestContext) {
    let focus_handle = cx.read(|cx| menu.read_with(cx, |menu, cx| menu.focus_handle(cx)));
    cx.update(|window, cx| window.focus(&focus_handle, cx));
    draw_window(cx);
}

/// How many [`ContextMenuEvent::Dismissed`]s the menu has emitted since the last call.
fn dismissals(events: &mut UnboundedReceiver<ContextMenuEvent>) -> usize {
    std::iter::from_fn(|| events.try_recv().ok()).count()
}

#[gpui::test]
fn down_and_enter_activate_the_highlighted_entry_once(cx: &mut TestAppContext) {
    cx.update(init_test);
    let (harness, cx) = menu_with(cx);
    let mut events = cx.events(&harness.menu);
    focus_menu(&harness.menu, cx);

    press_key(cx, "down");
    press_key(cx, "enter");

    assert_eq!(
        harness.opened.get(),
        0,
        "Down moved the highlight off the first entry"
    );
    assert_eq!(
        harness.duplicated.get(),
        1,
        "Enter ran the highlighted entry once"
    );
    assert_eq!(
        dismissals(&mut events),
        1,
        "activating an entry dismisses the menu"
    );
}

#[gpui::test]
fn escape_dismisses_the_menu_without_activating_an_entry(cx: &mut TestAppContext) {
    cx.update(init_test);
    let (harness, cx) = menu_with(cx);
    let mut events = cx.events(&harness.menu);
    focus_menu(&harness.menu, cx);

    press_key(cx, "escape");

    assert_eq!(dismissals(&mut events), 1, "Escape dismisses the menu");
    assert_eq!(harness.opened.get(), 0, "Escape runs no entry");
    assert_eq!(harness.duplicated.get(), 0, "Escape runs no entry");
}

#[gpui::test]
fn the_keyboard_selection_wraps_past_disabled_entries_and_separators(cx: &mut TestAppContext) {
    cx.update(init_test);
    let (harness, cx) = menu_with(cx);
    let mut events = cx.events(&harness.menu);
    focus_menu(&harness.menu, cx);

    // Backwards from `Open` wraps to `Duplicate`, stepping over the disabled `Delete` and
    // the separator.
    press_key(cx, "up");
    press_key(cx, "enter");
    assert_eq!(
        harness.duplicated.get(),
        1,
        "Up wrapped to the last runnable entry, not onto the disabled one"
    );

    // Forwards from `Duplicate` wraps to `Open` the same way.
    press_key(cx, "down");
    press_key(cx, "enter");
    assert_eq!(harness.opened.get(), 1, "Down wrapped to the first entry");
    assert_eq!(
        harness.duplicated.get(),
        1,
        "the disabled and separator entries can never run"
    );
    assert_eq!(
        dismissals(&mut events),
        2,
        "each activation dismissed the menu"
    );
}
