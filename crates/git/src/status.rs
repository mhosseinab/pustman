//! `git status` for a workspace root: the checked-out branch and how much is dirty
//! (REQ-COLLAB-03).

use std::io::ErrorKind;
use std::path::Path;

use smol::process::Command;

/// What `git status` reports for one repository (REQ-COLLAB-03).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GitStatus {
    /// The checked-out branch, or `HEAD (detached)` when HEAD is detached from any
    /// branch. The status bar shows this string as it is.
    pub branch: String,
    /// How many entries `git status` lists as changed: tracked changes, unmerged paths
    /// and untracked files. Zero in a clean repository.
    pub dirty: usize,
}

/// Why the status could not be read.
#[derive(Debug, thiserror::Error)]
pub enum GitError {
    /// `git` is installed but could not be started.
    #[error("git could not be started: {reason}")]
    Run { reason: String },

    /// `git status` answered with output this crate cannot read.
    #[error("git status output could not be read: {reason}")]
    Unreadable { reason: String },
}

/// The branch and dirty count of the repository containing `root` (REQ-COLLAB-03).
///
/// Returns `Ok(None)` when `root` is not inside a repository, or when `git` is not
/// installed — the status bar shows nothing in either case, which is not an error worth
/// a notification.
///
/// # Errors
///
/// [`GitError::Run`] when `git` cannot be started for any reason other than being absent
/// from `PATH`; [`GitError::Unreadable`] when `git status` succeeds but its output has no
/// branch header or is not UTF-8.
pub async fn status(root: &Path) -> Result<Option<GitStatus>, GitError> {
    let output = match Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["status", "--porcelain=v2", "--branch"])
        .output()
        .await
    {
        Ok(output) => output,
        // `git` is not installed: no repository to report on.
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(GitError::Run {
                reason: error.to_string(),
            });
        }
    };
    if !output.status.success() {
        // Every fatal `git status` exit — not a repository, an unreadable root, a broken
        // repository — means there is no status to report here, which is what `Ok(None)`
        // says. `git` explains itself on stderr, and the status bar has no use for it.
        return Ok(None);
    }
    let porcelain = String::from_utf8(output.stdout).map_err(|error| GitError::Unreadable {
        reason: error.to_string(),
    })?;
    parse_porcelain(&porcelain).map(Some)
}

/// Parse `git status --porcelain=v2 --branch` output into a [`GitStatus`].
///
/// Header lines start with `#`; every other line is one changed entry, so the dirty
/// count is simply how many of them there are. Paths may be C-quoted by git, but only
/// ever inside entry lines, which are counted but not read.
///
/// # Errors
///
/// [`GitError::Unreadable`] when no `# branch.head` header is present.
fn parse_porcelain(porcelain: &str) -> Result<GitStatus, GitError> {
    let mut branch = None;
    let mut dirty = 0usize;
    for line in porcelain.lines() {
        if let Some(head) = line.strip_prefix("# branch.head ") {
            branch = Some(if head == "(detached)" {
                "HEAD (detached)".to_string()
            } else {
                head.to_string()
            });
        } else if !line.is_empty() && !line.starts_with('#') {
            dirty += 1;
        }
    }
    let branch = branch.ok_or_else(|| GitError::Unreadable {
        reason: "git status printed no branch.head header".to_string(),
    })?;
    Ok(GitStatus { branch, dirty })
}

#[cfg(test)]
#[path = "status_tests.rs"]
mod tests;
