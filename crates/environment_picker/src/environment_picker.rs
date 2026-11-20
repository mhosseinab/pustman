//! `environment_picker` — the active-environment selector in the title bar (PRD
//! REQ-VAR-08): one picker listing the workspace's environments plus the first-class
//! "No environment" option, with the selection stored per workspace in the local state
//! (`storage`'s `kv` table). The environments are input (`api_model` types) — this crate
//! never reads workspace files, and it never reaches for the top crate's `AppStore`.
//!
//! Two doors in:
//! - [`selector`] — the picker delegate, the per-workspace persistence, and the
//!   `environment::SelectEnvironment` handler [`init`] registers.
//! - [`install_inputs`] — where the app's wire hands the opener what this crate cannot
//!   know on its own: the workspace id, the environments, and the
//!   `Arc<Mutex<LocalStore>>` the app holds.
//!
//! plus [`init`] — called once at startup after `command_palette` (whose [`Picker`] this
//! reuses).

pub mod quick_look;
pub mod selector;

pub use selector::{EnvironmentSelector, SelectorInputs, install_inputs};

use gpui::App;

/// Set the environment picker up: answer `environment::SelectEnvironment` by opening the
/// picker over the active workspace, and `environment::QuickLook` by opening the
/// quick-look ([`quick_look`]).
pub fn init(cx: &mut App) {
    selector::init(cx);
    quick_look::init(cx);
}
