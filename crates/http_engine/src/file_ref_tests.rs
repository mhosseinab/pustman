//! Body file references resolve from the workspace root, and a missing file names
//! its path (REQ-HTTP-05).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::{TransportError, resolve_file_ref};

static NEXT_WORKSPACE_ROOT: AtomicUsize = AtomicUsize::new(0);

/// A fresh workspace root per call: the tests touch the real filesystem, so two tests
/// must never share a directory.
fn workspace_root() -> PathBuf {
    let unique = NEXT_WORKSPACE_ROOT.fetch_add(1, Ordering::Relaxed);
    let root =
        std::env::temp_dir().join(format!("pustman-file-ref-{}-{unique}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    root
}

#[test]
fn relative_file_ref_resolves_from_workspace_root() {
    let workspace_root = workspace_root();
    let body_file = workspace_root.join("fixtures").join("body.json");
    std::fs::create_dir_all(body_file.parent().unwrap()).unwrap();
    std::fs::write(&body_file, b"{}").unwrap();

    let resolved = resolve_file_ref(&workspace_root, "fixtures/body.json").unwrap();
    assert_eq!(resolved, body_file);

    // Absolute references point outside the workspace and are kept as they are
    // (REQ-HTTP-05), so the resolved path is the reference itself.
    let absolute = body_file.to_string_lossy().into_owned();
    let resolved = resolve_file_ref(&workspace_root, &absolute).unwrap();
    assert_eq!(resolved, body_file);

    std::fs::remove_dir_all(&workspace_root).unwrap();
}

#[test]
fn missing_file_is_file_not_found_error() {
    let workspace_root = workspace_root();

    let error = resolve_file_ref(&workspace_root, "fixtures/missing.json").unwrap_err();

    let expected_path = workspace_root
        .join("fixtures")
        .join("missing.json")
        .to_string_lossy()
        .into_owned();
    assert_eq!(
        error,
        TransportError::FileNotFound {
            path: expected_path,
        }
    );

    std::fs::remove_dir_all(&workspace_root).unwrap();
}

#[test]
fn relative_file_ref_escaping_with_dotdot_is_rejected() {
    let workspace_root = workspace_root();
    let unique = NEXT_WORKSPACE_ROOT.fetch_add(1, Ordering::Relaxed);
    let sibling = std::env::temp_dir().join(format!(
        "pustman-file-ref-outside-{}-{unique}",
        std::process::id()
    ));
    std::fs::create_dir_all(&sibling).unwrap();
    std::fs::write(sibling.join("outside.json"), b"{}").unwrap();

    let reference = format!(
        "../{}/outside.json",
        sibling.file_name().unwrap().to_string_lossy()
    );
    let error = resolve_file_ref(&workspace_root, &reference).unwrap_err();

    assert!(
        matches!(error, TransportError::Io { .. }),
        "expected Io, got {error:?}"
    );

    std::fs::remove_dir_all(&workspace_root).unwrap();
    std::fs::remove_dir_all(&sibling).unwrap();
}

#[cfg(unix)]
#[test]
fn relative_file_ref_through_symlink_out_of_root_is_rejected() {
    let workspace_root = workspace_root();
    let unique = NEXT_WORKSPACE_ROOT.fetch_add(1, Ordering::Relaxed);
    let outside = std::env::temp_dir().join(format!(
        "pustman-file-ref-outside-{}-{unique}",
        std::process::id()
    ));
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("outside.json"), b"{}").unwrap();
    std::os::unix::fs::symlink(&outside, workspace_root.join("link")).unwrap();

    let error = resolve_file_ref(&workspace_root, "link/outside.json").unwrap_err();

    assert!(
        matches!(error, TransportError::Io { .. }),
        "expected Io, got {error:?}"
    );

    std::fs::remove_dir_all(&workspace_root).unwrap();
    std::fs::remove_dir_all(&outside).unwrap();
}

#[test]
fn relative_file_ref_with_inner_dotdot_inside_root_resolves() {
    let workspace_root = workspace_root();
    let body_file = workspace_root.join("fixtures").join("body.json");
    std::fs::create_dir_all(body_file.parent().unwrap()).unwrap();
    std::fs::write(&body_file, b"{}").unwrap();

    let reference = "fixtures/../fixtures/body.json";
    let resolved = resolve_file_ref(&workspace_root, reference).unwrap();
    assert_eq!(resolved, workspace_root.join(reference));

    std::fs::remove_dir_all(&workspace_root).unwrap();
}

/// Neither `Debug` (what error logging prints, PRD §7) nor `Display` (what the user
/// sees) may name the marker directory or the workspace root.
fn assert_error_is_path_free(error: &TransportError, marker: &str, workspace_root: &Path) {
    assert!(
        matches!(error, TransportError::Io { .. }),
        "expected Io, got {error:?}"
    );
    for rendered in [format!("{error:?}"), format!("{error}")] {
        assert!(!rendered.contains(marker), "leaked marker in {rendered:?}");
        assert!(
            !rendered.contains(workspace_root.to_string_lossy().as_ref()),
            "leaked workspace root in {rendered:?}"
        );
    }
}

#[test]
fn test_io_errors_never_carry_the_path_or_reference() {
    let workspace_root = workspace_root();
    let unique = NEXT_WORKSPACE_ROOT.fetch_add(1, Ordering::Relaxed);
    let marker = format!("pustman-file-ref-outside-{}-{unique}", std::process::id());
    let sibling = std::env::temp_dir().join(&marker);
    std::fs::create_dir_all(&sibling).unwrap();
    std::fs::write(sibling.join("outside.json"), b"{}").unwrap();

    // The containment refusal: the reference names the marker directory and can carry
    // resolved variable values, so it must not reach the detail either.
    let reference = format!("../{marker}/outside.json");
    let error = resolve_file_ref(&workspace_root, &reference).unwrap_err();
    assert_error_is_path_free(&error, &marker, &workspace_root);

    // A permission-denied `io_detail` on unix. Root ignores the directory mode, so
    // probe first and skip in that case.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let denied = workspace_root.join("denied");
        std::fs::create_dir(&denied).unwrap();
        std::fs::set_permissions(&denied, std::fs::Permissions::from_mode(0o000)).unwrap();
        let running_as_root = std::fs::write(denied.join("probe"), b"").is_ok();
        if !running_as_root {
            let error = resolve_file_ref(&workspace_root, "denied/secret.json").unwrap_err();
            assert_error_is_path_free(&error, &marker, &workspace_root);
        }
        std::fs::set_permissions(&denied, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::remove_dir_all(&denied).unwrap();
    }

    std::fs::remove_dir_all(&workspace_root).unwrap();
    std::fs::remove_dir_all(&sibling).unwrap();
}
