//! `git status` for a workspace root: the checked-out branch and how much is dirty
//! (REQ-COLLAB-03).

use std::ffi::OsStr;
use std::io::ErrorKind;
use std::path::Path;

use smol::process::Command;

/// Environment variables that locate a repository of their own and beat `-C`: `git`
/// honours them over the root it is asked about, so none may reach the child process.
const REPOSITORY_LOCATING_VARIABLES: [&str; 5] = [
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_COMMON_DIR",
    "GIT_INDEX_FILE",
    "GIT_OBJECT_DIRECTORY",
];

/// What `git` answers when `root` is not inside a repository, on the `LC_ALL=C` message.
const NOT_A_REPOSITORY: &str = "not a git repository";

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
    /// `git` could not be run: it is installed but could not be started, or `git status`
    /// itself failed on a repository Pustman is asked about.
    #[error("git could not be run: {reason}")]
    Run { reason: String },

    /// `git status` answered with output this crate cannot read.
    #[error("git status output could not be read: {reason}")]
    Unreadable { reason: String },
}

/// The branch and dirty count of the repository containing `root` (REQ-COLLAB-03).
///
/// Returns `Ok(None)` only when `root` is not inside a repository, or when `git` is not
/// installed — the status bar shows nothing in either case, which is not an error worth
/// a notification. Any other failure on a repository is an [`GitError`]: a repository
/// Pustman cannot read must reach the user, never vanish as "no repository".
///
/// # Errors
///
/// [`GitError::Run`] when `git` cannot be started for any reason other than being absent
/// from `PATH`, or when `git status` fails without answering "not a git repository";
/// [`GitError::Unreadable`] when `git status` succeeds but its output has no branch
/// header or is not UTF-8.
pub async fn status(root: &Path) -> Result<Option<GitStatus>, GitError> {
    status_in(root, std::env::vars()).await
}

/// [`status`] over a `git status` command whose base environment is `environment`.
///
/// This crate-private seam lets tests supply `GIT_DIR` and friends without setting the
/// process environment, which would race every other test.
async fn status_in<I, K, V>(root: &Path, environment: I) -> Result<Option<GitStatus>, GitError>
where
    I: IntoIterator<Item = (K, V)>,
    K: AsRef<OsStr>,
    V: AsRef<OsStr>,
{
    let output = match status_command(root, environment).output().await {
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
        // `git` answers a non-repository with "fatal: not a git repository …" — matched
        // on the `LC_ALL=C` message, see `status_command` — and that is the one failure
        // the status bar swallows. Every other failure on a real repository (dubious
        // ownership, a corrupt `.git`, an unreadable root) must reach the user.
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if stderr.contains(NOT_A_REPOSITORY) {
            return Ok(None);
        }
        let reason = if stderr.is_empty() {
            format!("git status failed with {}", output.status)
        } else {
            stderr
        };
        return Err(GitError::Run { reason });
    }
    let porcelain = String::from_utf8(output.stdout).map_err(|error| GitError::Unreadable {
        reason: error.to_string(),
    })?;
    parse_porcelain(&porcelain).map(Some)
}

/// The `git status` command for `root`, built on the supplied base environment.
///
/// The environment is untrusted: `GIT_DIR` and its siblings name a repository of their
/// own and beat `-C`, so they are removed *after* the base environment is applied (last
/// write wins) and the reported repository is always the one `root` sits in. `LC_ALL=C`
/// pins git's messages so the not-a-repository match cannot be fooled by a localized
/// `git`; `GIT_OPTIONAL_LOCKS=0` and `--no-optional-locks` keep a read-only status from
/// taking the index lock; `GIT_TERMINAL_PROMPT=0` keeps git from prompting on Pustman's
/// behalf.
fn status_command<I, K, V>(root: &Path, environment: I) -> Command
where
    I: IntoIterator<Item = (K, V)>,
    K: AsRef<OsStr>,
    V: AsRef<OsStr>,
{
    let mut command = Command::new("git");
    command.envs(environment);
    for variable in REPOSITORY_LOCATING_VARIABLES {
        command.env_remove(variable);
    }
    command
        .env("LC_ALL", "C")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_TERMINAL_PROMPT", "0")
        .args(["--no-optional-locks", "-C"])
        .arg(root)
        .args(["status", "--porcelain=v2", "--branch"]);
    command
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

#[cfg(test)]
#[path = "status_failure_tests.rs"]
mod failure_tests;
