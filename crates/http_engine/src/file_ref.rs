//! Resolving body file references against the workspace root (REQ-HTTP-05).

use std::path::{Path, PathBuf};

use crate::error::TransportError;

/// Resolve a body file reference to a concrete path (REQ-HTTP-05).
///
/// A relative reference is joined onto `workspace_root` and must stay inside it: the
/// joined path and the root are canonicalized, and a resolved path outside the root is
/// refused with [`TransportError::Io`]. Imported collections are untrusted data, so a
/// relative reference must not read arbitrary local files via `..` or symlinks. An
/// absolute reference is kept as is, because paths outside the workspace are stored
/// absolute. If the joined path does not exist, the send fails with
/// [`TransportError::FileNotFound`] naming that path.
///
/// The filesystem calls are blocking: callers run this on a background task, never on
/// the GPUI foreground thread.
pub fn resolve_file_ref(workspace_root: &Path, reference: &str) -> Result<PathBuf, TransportError> {
    if Path::new(reference).is_absolute() {
        let path = PathBuf::from(reference);
        return if path.exists() {
            Ok(path)
        } else {
            Err(TransportError::FileNotFound {
                path: path.to_string_lossy().into_owned(),
            })
        };
    }

    let path = workspace_root.join(reference);
    let canonical_path = std::fs::canonicalize(&path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            TransportError::FileNotFound {
                path: path.to_string_lossy().into_owned(),
            }
        } else {
            io_detail("the body file", &error)
        }
    })?;
    let canonical_root = std::fs::canonicalize(workspace_root)
        .map_err(|error| io_detail("the workspace root", &error))?;
    if !canonical_path.starts_with(&canonical_root) {
        return Err(TransportError::Io {
            detail: "The body file reference resolves outside the workspace root; \
                     files outside the workspace must use an absolute path."
                .to_string(),
        });
    }
    Ok(path)
}

/// `detail` is path-free and reference-free: [`TransportError`]'s `Debug` prints it
/// verbatim into logs (PRD §7), and `std::io::Error`'s `Display` can embed the path,
/// so only the error kind reaches the detail.
fn io_detail(subject: &str, error: &std::io::Error) -> TransportError {
    TransportError::Io {
        detail: format!("Could not access {subject}: {}", error.kind()),
    }
}

#[cfg(test)]
#[path = "file_ref_tests.rs"]
mod tests;
