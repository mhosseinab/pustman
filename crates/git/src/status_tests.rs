use std::path::Path;

use smol::process::Command;
use tempfile::TempDir;

use crate::status::{GitStatus, status};

/// Run one `git` command in `root`; a test repository must accept every command.
fn git(root: &Path, arguments: &[&str]) {
    let output = smol::block_on(
        Command::new("git")
            .arg("-C")
            .arg(root)
            .args(arguments)
            .output(),
    )
    .expect("git is installed for these tests");
    assert!(
        output.status.success(),
        "git {arguments:?} must succeed in a test repository: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// A temporary repository on `main` with one committed file, so `status` has a branch
/// and a clean baseline to report.
fn repository_with_commit() -> TempDir {
    let directory = tempfile::tempdir().expect("a temporary directory");
    let root = directory.path();
    git(root, &["init", "-b", "main"]);
    std::fs::write(root.join("tracked.txt"), "one").expect("the tracked file is written");
    git(root, &["add", "tracked.txt"]);
    git(
        root,
        &[
            "-c",
            "user.name=Pustman Test",
            "-c",
            "user.email=test@example.invalid",
            "commit",
            "-m",
            "initial",
        ],
    );
    directory
}

#[test]
fn status_reports_branch_and_dirty_count() {
    let directory = repository_with_commit();
    let root = directory.path();
    std::fs::write(root.join("tracked.txt"), "changed").expect("the tracked file is modified");
    std::fs::write(root.join("untracked.txt"), "new").expect("the untracked file is written");

    let reported = smol::block_on(status(root))
        .expect("a repository's status is read")
        .expect("a repository has a status");

    assert_eq!(
        reported,
        GitStatus {
            branch: "main".to_string(),
            dirty: 2
        },
        "the branch and every changed entry — tracked or untracked — must be reported"
    );
}

#[test]
fn non_repo_returns_none() {
    let directory = tempfile::tempdir().expect("a temporary directory");

    let reported = smol::block_on(status(directory.path()))
        .expect("a directory outside a repository is not an error");

    assert_eq!(
        reported, None,
        "outside a repository there is no status to report"
    );
}

#[test]
fn detached_head_reported() {
    let directory = repository_with_commit();
    let root = directory.path();
    git(root, &["checkout", "--detach", "HEAD"]);

    let reported = smol::block_on(status(root))
        .expect("a detached repository's status is read")
        .expect("a detached repository has a status");

    assert_eq!(
        reported.branch, "HEAD (detached)",
        "a detached HEAD must be reported as such, never as a branch name"
    );
    assert_eq!(reported.dirty, 0);
}
