//! `collections_panel` — the left-dock Collections panel (PRD REQ-UI-01): the open
//! workspace's collections, folders and requests as a virtualized tree (REQ-NFR-02's
//! 10,000-request tree at 60 fps). The rows come from `workspace_store`'s
//! [`WorkspaceSnapshot`] and follow the `order` arrays their parents store them in
//! (REQ-WSP-01).
//!
//! Two doors in:
//! - [`tree`] — the [`CollectionsTree`] panel and the rows it draws.
//! - [`init`] — the app-level wiring this crate owns: every workspace window gets the
//!   panel in its left dock.

pub mod tree;

pub use tree::{CollectionsTree, PANEL_ID, TreeRow};

use std::collections::BTreeMap;
use std::path::PathBuf;

use gpui::{App, AppContext, Global, Subscription};
use workspace::Workspace;
use workspace_store::WorkspaceSnapshot;

/// The app-wide registration: started once, and kept alive for as long as the app so the
/// workspace observer does not stop firing.
#[derive(Default)]
struct PanelWiring {
    started: bool,
    _subscriptions: Vec<Subscription>,
}

impl Global for PanelWiring {}

/// Give every workspace window the Collections panel in its left dock (REQ-UI-01).
///
/// The panel is registered when the workspace appears — deferred like
/// `history_panel`'s wire in `pustman`, because a workspace is created inside its
/// window's own update and the dock registration must not run on that stack.
pub fn init(cx: &mut App) {
    if cx.default_global::<PanelWiring>().started {
        return;
    }
    cx.default_global::<PanelWiring>().started = true;
    let subscription = cx.observe_new::<Workspace>(|_workspace, _window, cx| {
        let workspace = cx.entity();
        cx.defer(move |cx| {
            let registered = cx.with_window(workspace.entity_id(), |window, cx| {
                let panel = cx.new(|cx| CollectionsTree::new(empty_snapshot(), cx));
                workspace.update(cx, |workspace, cx| {
                    workspace.register_panel(panel, window, cx);
                });
            });
            if registered.is_none() {
                // Handled: the window closed between its workspace's creation and this
                // callback; there is no dock left to register the panel in.
            }
        });
    });
    cx.default_global::<PanelWiring>()
        ._subscriptions
        .push(subscription);
}

/// The tree's content before a workspace is open (`workspace_open` is S41a): nothing to
/// show, so the panel draws its empty state until a snapshot arrives.
fn empty_snapshot() -> WorkspaceSnapshot {
    WorkspaceSnapshot {
        root: PathBuf::new(),
        items: BTreeMap::new(),
        problems: Vec::new(),
        read_only: false,
    }
}
