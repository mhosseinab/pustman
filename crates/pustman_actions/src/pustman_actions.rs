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
            /// Creates a new GraphQL request and opens it in a tab.
            NewGraphQlRequest,
            /// Creates a new WebSocket request and opens it in a tab.
            NewWebSocketRequest,
            /// Creates a new Socket.IO request and opens it in a tab.
            NewSocketIoRequest,
            /// Creates a new gRPC request and opens it in a tab.
            NewGrpcRequest,
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

    /// Opens the variables editor on the active scope with the cursor in one secret's
    /// masked value field (S43c): what the `UnresolvedSecret` error panel's "Set <name>"
    /// button dispatches. `name` is the variable key — never its value — and it is the
    /// lookup, not a display string: the editor aims at the row that carries it.
    //
    // `gpui` re-exports the `schemars` its own `Action` schema API is built on
    // (`gpui::private` is the re-export macro-generated code names); the derive's
    // generated schema impl resolves through this alias, so no new dependency appears.
    use gpui::private::schemars;

    #[derive(
        Clone, PartialEq, Eq, Debug, serde::Deserialize, schemars::JsonSchema, gpui::Action,
    )]
    #[action(namespace = environment)]
    pub struct EditSecret {
        /// The variable key whose secret row takes the cursor.
        #[serde(default)]
        pub name: String,
    }
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
