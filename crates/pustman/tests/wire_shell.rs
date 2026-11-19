//! The shell wire (S22e): the binary's startup path in the test platform — the same
//! `pustman::init` order `main.rs` runs, with an in-memory store so a test never touches
//! the user's config dir, then the workspace window `main.rs` opens. The window must show
//! the shell's three regions: the left dock, the bottom dock and the status bar.

use gpui::TestAppContext;
use pustman::{AppStore, StoreLocation};
use ui::test_support::draw_window;
use workspace::Workspace;

#[gpui::test]
fn wire_shell_init_opens_workspace_with_docks(cx: &mut TestAppContext) {
    cx.update(|cx| pustman::init_with_store(cx, StoreLocation::InMemory));
    let (workspace, cx) = cx.add_window_view(Workspace::new);
    cx.update(|_window, cx| pustman::StartupProblems::attach_workspace(&workspace, cx));
    // The store opens on a background task; let it land before reading it.
    cx.run_until_parked();

    cx.read(|cx| {
        assert!(
            cx.try_global::<AppStore>().is_some(),
            "the init order opens the local store and keeps it reachable (S22e)"
        );
    });

    draw_window(cx);

    assert!(
        cx.debug_bounds("left-dock").is_some(),
        "the workspace window shows the left dock (REQ-UI-01)"
    );
    assert!(
        cx.debug_bounds("bottom-dock").is_some(),
        "the workspace window shows the bottom dock (REQ-UI-04)"
    );
    assert!(
        cx.debug_bounds("status-bar").is_some(),
        "the workspace window shows the status bar"
    );
}
