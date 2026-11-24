//! The status bar's branch item (REQ-COLLAB-03): the strip shows the checked-out branch
//! and how much is dirty of the workspace's repository. The status comes from a fixed
//! source rather than a real `git` child: a `smol::process` future is woken by the
//! async-io reactor's thread, which GPUI's test scheduler refuses (the S41a Decision
//! this test's seam exists for).

use std::path::PathBuf;
use std::sync::Arc;

use gpui::TestAppContext;
use ui::prelude::*;
use ui::test_support::{draw_window, init_test};
use workspace::Workspace;

use super::{BRANCH_SELECTOR, BranchItem, StatusAnswer, StatusSource};
use crate::status::GitStatus;

/// A source that answers with one fixed status, whatever root it is asked about.
struct FixedSource(GitStatus);

impl StatusSource for FixedSource {
    fn status(&self, _root: PathBuf) -> StatusAnswer {
        let status = self.0.clone();
        Box::pin(std::future::ready(Ok(Some(status))))
    }
}

#[gpui::test]
fn status_item_shows_branch_and_dirty_count(cx: &mut TestAppContext) {
    cx.update(init_test);
    let (workspace, cx) = cx.add_window_view(Workspace::new);
    let source = Arc::new(FixedSource(GitStatus {
        branch: "main".to_string(),
        dirty: 2,
    }));
    let item = cx.update(|window, cx| cx.new(|cx| BranchItem::new(source, window, cx)));
    workspace.update(cx, |workspace, cx| {
        let status_bar = workspace.status_bar();
        status_bar.update(cx, |status_bar, cx| {
            status_bar.add_left_item(item.clone(), cx);
        });
    });
    item.update(cx, |item, cx| {
        item.set_root(Some(PathBuf::from("/work/pustman")), cx);
    });
    cx.run_until_parked();

    let readout = item.read_with(cx, |item, _cx| item.readout());
    assert_eq!(
        readout.as_deref(),
        Some("main  2"),
        "the strip shows the branch and the dirty count (REQ-COLLAB-03)"
    );

    // Rendered-side proof: the readout paints inside the status bar it was added to.
    draw_window(cx);
    let bar = cx
        .debug_bounds("status-bar")
        .expect("the status bar renders");
    let shown = cx
        .debug_bounds(BRANCH_SELECTOR)
        .expect("the branch readout renders");
    assert!(
        shown.origin.x >= bar.origin.x
            && shown.origin.x + shown.size.width <= bar.origin.x + bar.size.width
            && shown.origin.y >= bar.origin.y
            && shown.origin.y + shown.size.height <= bar.origin.y + bar.size.height,
        "the branch readout renders inside the status bar ({shown:?} in {bar:?})"
    );
}
