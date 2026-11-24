//! Thin wrapper over the user's own `git` CLI (PRD REQ-COLLAB-03/04).
//!
//! Pustman shells out to `git`: there is no embedded Git library [D], so the credential
//! setup, configuration and hooks the user already has all apply. Every command runs
//! through `smol::process` and is awaited off the foreground thread — nothing here blocks
//! the UI (AGENTS.md › Async & concurrency).
//!
//! [`status`] is the first of the wrapper's commands: the branch and dirty count the
//! status bar shows when the workspace sits inside a repository (REQ-COLLAB-03).

mod status;

pub use status::{GitError, GitStatus, status};

#[cfg(feature = "gpui")]
mod status_item;

#[cfg(feature = "gpui")]
pub use status_item::init;
