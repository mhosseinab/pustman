use gpui::TestAppContext;

/// Every action the PRD needs, as `namespace::Name` (S21 action set).
const EXPECTED_ACTIONS: &[&str] = &[
    "workspace::NewRequest",
    "workspace::NewGraphQlRequest",
    "workspace::NewWebSocketRequest",
    "workspace::NewSocketIoRequest",
    "workspace::NewGrpcRequest",
    "workspace::NewTab",
    "workspace::CloseTab",
    "workspace::NextTab",
    "workspace::PreviousTab",
    "workspace::ToggleSidebar",
    "workspace::ToggleConsole",
    "workspace::OpenWorkspace",
    "workspace::CreateWorkspace",
    "workspace::OpenRecent",
    "workspace::Save",
    "workspace::SaveAll",
    "request::Send",
    "request::Cancel",
    "request::FocusUrlBar",
    "request::CopyAsCurl",
    "request::PasteCurl",
    "request::ToggleTlsVerify",
    "response::Find",
    "response::SaveBody",
    "response::CopyBody",
    "palette::Toggle",
    "quick_open::Toggle",
    "environment::SelectEnvironment",
    "environment::QuickLook",
    "environment::EditSecret",
    "history::Focus",
    "console::Clear",
    "console::Pause",
    "cookies::Manage",
    "import_export::Import",
    "import_export::Export",
];

/// The namespaces this crate declares actions under.
const ACTION_NAMESPACES: &[&str] = &[
    "workspace",
    "request",
    "response",
    "palette",
    "quick_open",
    "environment",
    "history",
    "console",
    "cookies",
    "import_export",
];

#[gpui::test]
fn every_action_name_resolves(cx: &mut TestAppContext) {
    cx.update(|cx| {
        for &name in EXPECTED_ACTIONS {
            cx.build_action(name, None).unwrap_or_else(|error| {
                panic!("{name} must resolve through build_action: {error}")
            });
        }

        let registered = cx.all_action_names();
        for &name in EXPECTED_ACTIONS {
            assert!(
                registered.contains(&name),
                "{name} builds but is not registered"
            );
        }

        let undeclared: Vec<&str> = registered
            .iter()
            .copied()
            .filter(|name| {
                ACTION_NAMESPACES
                    .iter()
                    .any(|namespace| name.starts_with(&format!("{namespace}::")))
            })
            .filter(|name| !EXPECTED_ACTIONS.contains(name))
            .collect();
        assert!(
            undeclared.is_empty(),
            "registered actions missing from EXPECTED_ACTIONS: {undeclared:?}"
        );
    });
}
