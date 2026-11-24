//! Failure and environment-override behaviour of [`status`]: a repository Pustman cannot
//! read is an error, never "no repository", and `GIT_*` variables inherited from the
//! environment never change which repository is reported.

use std::path::Path;

use smol::process::Command;
use tempfile::TempDir;

use crate::status::{GitError, status, status_in};

/// Repository-locating variables: a helper git command must run against the test's own
/// repository and no other.
const REPOSITORY_LOCATING_VARIABLES: [&str; 5] = [
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_COMMON_DIR",
    "GIT_INDEX_FILE",
    "GIT_OBJECT_DIRECTORY",
];

/// Run one `git` command in `root` hermetically: no user or system git configuration, no
/// signing, and no repository-locating variable reaching in from outside the test.
fn git(root: &Path, arguments: &[&str]) {
    let mut command = Command::new("git");
    for variable in REPOSITORY_LOCATING_VARIABLES {
        command.env_remove(variable);
    }
    command
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .args(["-c", "commit.gpgsign=false", "-C"])
        .arg(root)
        .args(arguments);
    let output = smol::block_on(command.output()).expect("git is installed for these tests");
    assert!(
        output.status.success(),
        "git {arguments:?} must succeed in a test repository: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// A temporary repository on `branch` with one committed file.
fn repository_on(branch: &str) -> TempDir {
    let directory = tempfile::tempdir().expect("a temporary directory");
    let root = directory.path();
    git(root, &["init", "-b", branch]);
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
fn garbage_git_file_reports_an_error() {
    let directory = tempfile::tempdir().expect("a temporary directory");
    std::fs::write(directory.path().join(".git"), "garbage\n").expect("the .git file is written");

    let reported = smol::block_on(status(directory.path()));

    assert!(
        matches!(&reported, Err(GitError::Run { .. })),
        "a repository Pustman cannot read must be an error, never 'no repository': {reported:?}"
    );
}

#[test]
fn git_dir_in_the_environment_does_not_hide_the_root_repository() {
    let root = repository_on("main");
    let other = repository_on("other");

    let reported = smol::block_on(status_in(
        root.path(),
        [("GIT_DIR", other.path().join(".git").into_os_string())],
    ))
    .expect("both repositories are readable")
    .expect("the root repository has a status");

    assert_eq!(
        reported.branch, "main",
        "a GIT_DIR in the environment must not decide which repository is reported"
    );
}
