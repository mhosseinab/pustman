//! One recent-workspaces source (#332/#333): the welcome pane's Recent list and the title
//! bar's switcher both draw the [`RecentWorkspaces`] global — not a read of their own —
//! so whatever the global holds is what both offer, and a failed read is shown from it.

use std::path::PathBuf;

use gpui::{Modifiers, TestAppContext, VisualTestContext};
use ui::components::context_menu::MENU_SELECTOR;
use ui::test_support::{draw_window, init_test};

use crate::recent_workspaces::{RecentEntry, RecentWorkspaces};
use crate::title_bar::SWITCHER_SELECTOR;
use crate::welcome::{RECENT_FAILURE_SELECTOR, recent_row_id};
use crate::{Workspace, init};

/// A window on the app's init order with no local store installed: nothing but the
/// global can put a recent workspace on screen.
fn window_without_a_store(cx: &mut TestAppContext) -> &mut VisualTestContext {
    cx.update(init_test);
    cx.update(init);
    let (_workspace, cx) = cx.add_window_view(Workspace::new);
    cx.update(|window, _cx| window.activate_window());
    cx.run_until_parked();
    draw_window(cx);
    cx
}

fn install_recents(cx: &mut VisualTestContext, entries: Vec<RecentEntry>, failed: bool) {
    cx.update(|_window, cx| cx.set_global(RecentWorkspaces { entries, failed }));
    cx.run_until_parked();
    draw_window(cx);
}

fn billing_entry() -> RecentEntry {
    RecentEntry {
        path: PathBuf::from("/workspaces/billing"),
        name: String::from("Billing API"),
    }
}

#[gpui::test]
fn test_the_welcome_recent_list_shows_what_the_recent_global_holds(cx: &mut TestAppContext) {
    let cx = window_without_a_store(cx);
    let entry = billing_entry();
    assert!(
        cx.debug_bounds(recent_row_id(&entry.path).leak()).is_none(),
        "nothing is listed before the global holds anything"
    );

    install_recents(cx, vec![entry.clone()], false);

    assert!(
        cx.debug_bounds(recent_row_id(&entry.path).leak()).is_some(),
        "the welcome lists the global's entry, with no read of its own"
    );
}

#[gpui::test]
fn test_the_welcome_says_so_when_the_recent_global_failed_to_read(cx: &mut TestAppContext) {
    let cx = window_without_a_store(cx);
    install_recents(cx, Vec::new(), true);
    assert!(
        cx.debug_bounds(RECENT_FAILURE_SELECTOR).is_some(),
        "a failed read is reported in the Recent list, not shown as an empty one"
    );
}

#[gpui::test]
fn test_the_switcher_lists_what_the_recent_global_holds(cx: &mut TestAppContext) {
    let cx = window_without_a_store(cx);
    install_recents(cx, vec![billing_entry()], false);

    let switcher = cx
        .debug_bounds(SWITCHER_SELECTOR)
        .expect("the switcher is on screen");
    cx.simulate_click(switcher.center(), Modifiers::none());
    cx.run_until_parked();
    draw_window(cx);

    assert!(
        cx.debug_bounds(MENU_SELECTOR).is_some(),
        "the switcher opens"
    );
    assert!(
        cx.debug_bounds("Billing API").is_some(),
        "the switcher lists the global's entry"
    );
}
