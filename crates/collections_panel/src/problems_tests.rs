//! The tree's problem badges (S42b): a file with merge-conflict markers draws its error
//! badge and a message naming the file and the line the markers start at (REQ-COLLAB-02),
//! and clicking the badge opens the details popover.

use std::fs;

use gpui::{Entity, Modifiers, TestAppContext, VisualTestContext};
use ui::test_support::{draw_window, init_test};

use super::DETAILS_SELECTOR;
use crate::tree::{CollectionsTree, TreeRow};

/// The workspace identity of the scratch workspace the test writes.
const WORKSPACE_FILE: &str = "{\n  \"id\": \"01ARZ3NDEKTSV4RRFFQ69G5FAV\",\n  \"name\": \"Conflicted workspace\",\n  \"format_version\": \"1.0\"\n}\n";

/// A collection file whose first merge-conflict marker sits on line 4 — the shape the
/// `workspace_store` conflict fixture has: three JSON lines, then `<<<<<<< HEAD`.
const CONFLICTED_COLLECTION: &str = r#"{
  "id": "01ARZ3NDEKTSV4RRFFQ69G5FB0",
  "name": "Users API",
<<<<<<< HEAD
  "description": "ours"
=======
  "description": "theirs"
>>>>>>> feature/users
}
"#;

#[gpui::test]
fn conflict_file_shows_badge_with_line(cx: &mut TestAppContext) {
    // A real workspace on disk: only the file itself knows which line the markers
    // start at, so the row's message must come from reading it.
    let root = tempfile::tempdir().expect("a scratch workspace");
    fs::write(root.path().join("pustman.json"), WORKSPACE_FILE).expect("the workspace file");
    fs::create_dir_all(root.path().join("collections/users-api"))
        .expect("the collection directory");
    fs::write(
        root.path().join("collections/users-api/collection.json"),
        CONFLICTED_COLLECTION,
    )
    .expect("the conflicted collection file");
    let snapshot = workspace_store::open(root.path()).expect("the scratch workspace opens");

    cx.update(init_test);
    let (tree, cx) = cx.add_window_view(|_window, cx| CollectionsTree::new(snapshot, cx));
    cx.run_until_parked();
    draw_window(cx);

    let rows = tree_rows(&tree, cx);
    let problem = rows
        .last()
        .expect("the conflicted file draws the tree's last row");
    let message = match problem {
        TreeRow::Problem { detail, .. } => detail.clone(),
        other => panic!("the conflicted file draws a problem row, not {other:?}"),
    };
    assert_eq!(
        message, "collections/users-api/collection.json:4 merge conflict markers",
        "the badge's message names the file and the line the markers start at (REQ-COLLAB-02)"
    );

    // The badge is drawn on the row, and clicking it opens the details popover.
    let badge_selector: &'static str =
        Box::leak(format!("{}-badge", problem.element_id()).into_boxed_str());
    let badge = cx
        .debug_bounds(badge_selector)
        .expect("the error badge is drawn on the row");
    cx.simulate_click(badge.center(), Modifiers::none());
    cx.run_until_parked();
    draw_window(cx);
    assert!(
        cx.debug_bounds(DETAILS_SELECTOR).is_some(),
        "clicking the badge opens the details popover"
    );
}

/// The rows the tree draws, read out of the panel.
fn tree_rows(tree: &Entity<CollectionsTree>, cx: &VisualTestContext) -> Vec<TreeRow> {
    cx.read(|cx| tree.read_with(cx, |tree, _| tree.rows().to_vec()))
}
