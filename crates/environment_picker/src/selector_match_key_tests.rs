//! The element ids the selector hands its picker for the rows it draws: every row keeps
//! its own id, including the out-of-range fallback, which must not become one shared
//! second id (issue #119) — a shared id would alias hover and focus state between rows.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use api_model::{Environment, ItemId};
use command_palette::PickerDelegate;
use gpui::TestAppContext;
use storage::LocalStore;
use ui::test_support::init_test;
use workspace::Workspace;

use super::EnvironmentSelector;

/// One environment with no variables: the selector lists names and hands out row ids.
fn environment(name: &str) -> Environment {
    Environment {
        id: ItemId::new(),
        name: name.to_string(),
        variables: Vec::new(),
        extra: BTreeMap::new(),
    }
}

#[gpui::test]
fn the_out_of_range_fallback_gives_every_row_its_own_element_id(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().expect("a temporary directory");
    let store =
        LocalStore::open(directory.path().join("pustman.db")).expect("the local store opens");
    cx.update(init_test);
    let (workspace, _window) = cx.add_window_view(Workspace::new);
    // One environment plus the first-class "No environment" row: rows 0 and 1 exist,
    // rows 2 and 3 are the out-of-range fallback this issue is about.
    let selector = EnvironmentSelector::new(
        "workspace-a",
        vec![environment("Staging")],
        None,
        Arc::new(Mutex::new(store)),
        workspace.downgrade(),
    );

    let row_ids: Vec<_> = (0..4).map(|index| selector.match_key(index)).collect();
    for (index, row_id) in row_ids.iter().enumerate() {
        for (other_index, other_row_id) in row_ids.iter().enumerate() {
            if index != other_index {
                assert_ne!(
                    row_id, other_row_id,
                    "rows {index} and {other_index} share the element id {row_id:?}"
                );
            }
        }
    }
}
