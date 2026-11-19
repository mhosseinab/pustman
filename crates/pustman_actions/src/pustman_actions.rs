//! Every PRD action, declared once here (plan D10, the `zed_actions` pattern), plus the
//! loader that binds the default keymaps from `assets/keymaps/`. Feature crates implement
//! the handlers for these actions; keymap JSON refers to an action as `namespace::Name`,
//! so action names are stable API — renaming one breaks user keymaps.
//!
//! Each action namespace is its own module so that actions sharing a short name
//! (`palette::Toggle`, `quick_open::Toggle`) can coexist.

mod keymap;

pub use keymap::load_default_keymap;

/// Workspace-level commands: tabs, docks, workspaces and saving.
pub mod workspace {
    gpui::actions!(
        workspace,
        [
            /// Creates a new request and opens it in a tab.
            NewRequest,
            /// Opens a new tab.
            NewTab,
            /// Closes the active tab.
            CloseTab,
            /// Activates the next tab.
            NextTab,
            /// Activates the previous tab.
            PreviousTab,
            /// Shows or hides the left dock (collections, environments, history).
            ToggleSidebar,
            /// Shows or hides the console in the bottom dock.
            ToggleConsole,
            /// Opens a workspace from disk.
            OpenWorkspace,
            /// Creates a new workspace.
            CreateWorkspace,
            /// Opens a recently used workspace.
            OpenRecent,
            /// Saves the active request.
            Save,
            /// Saves every open request.
            SaveAll,
        ]
    );
}

/// Request editor commands: sending, cancelling and the URL bar.
pub mod request {
    gpui::actions!(
        request,
        [
            /// Sends the request, or connects the session.
            Send,
            /// Cancels the in-flight request or session.
            Cancel,
            /// Moves focus to the URL bar.
            FocusUrlBar,
            /// Copies the request as a cURL command.
            CopyAsCurl,
            /// Pastes a cURL command into the request editor.
            PasteCurl,
            /// Enables or disables TLS verification for this request.
            ToggleTlsVerify,
        ]
    );
}

/// Response viewer commands.
pub mod response {
    gpui::actions!(
        response,
        [
            /// Finds text in the response body.
            Find,
            /// Saves the response body to a file.
            SaveBody,
            /// Copies the response body to the clipboard.
            CopyBody,
        ]
    );
}

/// Command palette.
pub mod palette {
    gpui::actions!(
        palette,
        [
            /// Opens or closes the command palette.
            Toggle,
        ]
    );
}

/// Quick open (fuzzy request switcher).
pub mod quick_open {
    gpui::actions!(
        quick_open,
        [
            /// Opens or closes quick open.
            Toggle,
        ]
    );
}

/// Environment picker.
pub mod environment {
    gpui::actions!(
        environment,
        [
            /// Picks the active environment.
            SelectEnvironment,
            /// Shows the variables the active environment resolves.
            QuickLook,
        ]
    );
}

/// History panel.
pub mod history {
    gpui::actions!(
        history,
        [
            /// Moves focus to the history panel.
            Focus,
        ]
    );
}

/// Console (debug log).
pub mod console {
    gpui::actions!(
        console,
        [
            /// Removes every console entry.
            Clear,
            /// Pauses or resumes the console log.
            Pause,
        ]
    );
}

/// Cookie jar.
pub mod cookies {
    gpui::actions!(
        cookies,
        [
            /// Opens the cookie jar.
            Manage,
        ]
    );
}

/// Import and export.
pub mod import_export {
    gpui::actions!(
        import_export,
        [
            /// Imports a collection or environment.
            Import,
            /// Exports a collection or environment.
            Export,
        ]
    );
}

#[cfg(test)]
#[path = "pustman_actions_tests.rs"]
mod tests;
