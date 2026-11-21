//! The status bar's readouts (PRD §3): the app's init order fills every workspace's
//! status bar — the console count on the left, the active environment on the right —
//! instead of leaving the redesign pack's 28 px strip empty, and both readouts follow
//! their globals as things change.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use api_model::{Environment, ItemId};
use console::{ConsoleEntry, ConsoleLevel, ConsoleStore};
use environment_picker::selector::{SelectorInputs, install_inputs};
use gpui::{Entity, TestAppContext, VisualTestContext};
use ui::test_support::draw_window;
use workspace::Workspace;

use crate::status_items::{ActiveEnvironment, ConsoleCount};
use crate::{AppStore, StoreLocation, init_with_store};

/// The real init order (`main.rs` runs `pustman::init`) and the workspace window it
/// opens, on an in-memory store so the test never touches the user's config dir — the
/// fixture `tests/wire_shell.rs` uses.
fn open_workspace(cx: &mut TestAppContext) -> (Entity<Workspace>, &mut VisualTestContext) {
    cx.update(|cx| init_with_store(cx, StoreLocation::InMemory));
    let (workspace, cx) = cx.add_window_view(Workspace::new);
    cx.update(|_window, cx| crate::StartupProblems::attach_workspace(&workspace, cx));
    // The store opens on a background task; let it land before reading the shell.
    cx.run_until_parked();
    (workspace, cx)
}

#[gpui::test]
fn the_status_bar_carries_the_console_count_and_the_environment(cx: &mut TestAppContext) {
    let (workspace, cx) = open_workspace(cx);
    draw_window(cx);

    let (left_items, right_items) = cx.read(|cx| {
        workspace.read_with(cx, |workspace, cx| {
            let status_bar = workspace.status_bar().read(cx);
            (
                status_bar.left_items().len(),
                status_bar.right_items().len(),
            )
        })
    });
    assert_eq!(
        left_items, 1,
        "the status bar's left half carries the console count, not an empty strip"
    );
    assert_eq!(
        right_items, 1,
        "the status bar's right half carries the active environment"
    );

    // Rendered-side proof: the readouts render in the status bar, under the selectors
    // `crate::status_items` puts on them.
    let status_bar = cx
        .debug_bounds("status-bar")
        .expect("the status bar renders");
    for selector in ["status-console-count", "status-environment"] {
        let item = cx
            .debug_bounds(selector)
            .unwrap_or_else(|| panic!("the {selector} readout renders in the status bar"));
        assert!(
            item.origin.x >= status_bar.origin.x
                && item.origin.x + item.size.width <= status_bar.origin.x + status_bar.size.width
                && item.origin.y >= status_bar.origin.y
                && item.origin.y + item.size.height <= status_bar.origin.y + status_bar.size.height,
            "the {selector} readout renders inside the status bar ({item:?} in {status_bar:?})"
        );
    }
}

/// An obviously fake console entry (`example.test`, no secrets) to record.
fn console_entry(url: &str) -> ConsoleEntry {
    ConsoleEntry {
        level: ConsoleLevel::Info,
        protocol: "http".to_owned(),
        method: None,
        url: url.to_owned(),
        status: Some(200),
        time: Some(Duration::from_millis(42)),
        size: Some(128),
        raw_request: String::new(),
        raw_response: String::new(),
    }
}

/// The status bar's left item as its own view type.
fn console_count(workspace: &Entity<Workspace>, cx: &VisualTestContext) -> Entity<ConsoleCount> {
    cx.read(|cx| {
        workspace.read_with(cx, |workspace, cx| {
            let item = workspace.status_bar().read(cx).left_items()[0].clone();
            item.downcast::<ConsoleCount>()
                .unwrap_or_else(|_| panic!("the left status item is the console count"))
        })
    })
}

/// The status bar's right item as its own view type.
fn active_environment(
    workspace: &Entity<Workspace>,
    cx: &VisualTestContext,
) -> Entity<ActiveEnvironment> {
    cx.read(|cx| {
        workspace.read_with(cx, |workspace, cx| {
            let item = workspace.status_bar().read(cx).right_items()[0].clone();
            item.downcast::<ActiveEnvironment>()
                .unwrap_or_else(|_| panic!("the right status item is the active environment"))
        })
    })
}

/// Install `environment_picker`'s inputs with one environment named `name` active — the
/// shape the app's wire installs once a workspace supplies environments.
fn activate_environment(name: &str, cx: &mut TestAppContext) {
    cx.update(|cx| {
        let store = Arc::clone(&cx.global::<AppStore>().0);
        let environment_id = ItemId::new();
        install_inputs(
            cx,
            SelectorInputs {
                workspace_id: "default".to_owned(),
                environments: vec![Environment {
                    id: environment_id,
                    name: name.to_owned(),
                    variables: Vec::new(),
                    extra: BTreeMap::new(),
                }],
                active: Some(environment_id),
                store,
            },
        );
    });
}

#[gpui::test]
fn the_console_count_follows_the_console_store(cx: &mut TestAppContext) {
    let (workspace, cx) = open_workspace(cx);
    let count = console_count(&workspace, cx);
    assert_eq!(
        count.read_with(cx, |item, _| item.readout()),
        "Console  0",
        "the strip starts at zero recorded entries"
    );

    cx.update(|_, cx| {
        ConsoleStore::push(cx, console_entry("https://example.test/one"));
        ConsoleStore::push(cx, console_entry("https://example.test/two"));
    });
    cx.run_until_parked();

    assert_eq!(
        count.read_with(cx, |item, _| item.readout()),
        "Console  2",
        "the readout follows the entries in the store"
    );
}

#[gpui::test]
fn the_environment_readout_shows_the_active_environment(cx: &mut TestAppContext) {
    let (workspace, cx) = open_workspace(cx);
    let environment_item = active_environment(&workspace, cx);
    assert_eq!(
        environment_item.read_with(cx, |item, _| item.readout()),
        "No environment",
        "with nothing active the readout says so (pack S07), it fabricates nothing"
    );

    activate_environment("Staging", cx);
    cx.run_until_parked();

    assert_eq!(
        environment_item.read_with(cx, |item, _| item.readout()),
        "Staging",
        "the readout follows the active environment"
    );
}
