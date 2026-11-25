//! The sidebar's Environments view (S48u3, the blueprint's Environments section): the
//! open workspace's environments with an "Active" badge on the active one, a click (or
//! Enter on a focused row) to make one active, and "＋ New" to add one.
//!
//! `Decision:` the list reads the installed [`SelectorInputs`] — the same environments
//! and active choice the picker and the status bar read — so the three can never
//! disagree, and a selection goes through [`select_environment`], the picker's own
//! path. The app's scope wiring re-installs the inputs when the workspace's files
//! change, which is how a created environment appears here.
//!
//! `Decision:` "＋ New" writes `environments/<slug>.environment.json` named
//! "New Environment" (numbered when the name is taken) through
//! [`workspace_store::save_item`] on a background task. Before this step nothing in the
//! app created an environment except an import; the variables editor edits it once it is
//! active, and the tree's inline rename stays the collections' own.
//!
//! `Decision:` the list is drawn with plain children, not a virtual list: a workspace
//! holds a handful of environments (one per place requests go), unlike the collections
//! tree or the history.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use api_model::{Environment, ItemId};
use gpui::{
    Action, App, Context, FocusHandle, Focusable, Render, Role, SharedString, Subscription, Task,
    WeakEntity, Window,
};
use pustman_actions::workspace::{CreateWorkspace, OpenWorkspace};
use ui::prelude::*;
use workspace::{DockPosition, Panel, Workspace, WorkspaceOpen};
use workspace_store::{Item, ItemBody};

use crate::selector::{SelectorInputs, select_environment};

#[cfg(test)]
#[path = "environments_panel_tests.rs"]
mod tests;

/// The panel's stable id: its rail entry and its dock entry.
pub const PANEL_ID: &str = "environments";

/// The key context the panel's own action is answered in.
const ENVIRONMENTS_CONTEXT: &str = "EnvironmentsPanel";

/// Where environments live in a workspace (PRD §6.1).
const ENVIRONMENTS_DIR: &str = "environments";

/// What a new environment is called before the user renames it.
const NEW_ENVIRONMENT_NAME: &str = "New Environment";

/// Controls' stable ids and test selectors.
pub(crate) const NEW_BUTTON_ID: &str = "environments-new";
pub(crate) const EMPTY_NEW_ID: &str = "environments-empty-new";
pub(crate) const NO_WORKSPACE_ID: &str = "environments-no-workspace";
pub(crate) const NO_WORKSPACE_OPEN_ID: &str = "environments-no-workspace-open";
pub(crate) const EMPTY_ID: &str = "environments-empty";

gpui::actions!(
    environments_panel,
    [
        /// Adds a new environment to the open workspace.
        NewEnvironment,
    ]
);

/// One environment row's element id and test selector: the environment's stable id.
pub(crate) fn row_id(id: ItemId) -> String {
    format!("environment-row-{id}")
}

/// The left-dock Environments view (see the module docs).
pub struct EnvironmentsPanel {
    focus_handle: FocusHandle,
    workspace: WeakEntity<Workspace>,
    /// The creates in flight, keyed by a per-panel counter: each "＋ New" writes its
    /// own file, so a second click never cancels the first one's write.
    pending_creates: BTreeMap<u64, Task<()>>,
    next_create: u64,
    /// The names this panel has handed out: a create's name is unique against these as
    /// well as the listed environments, which only learn of a new file once the
    /// workspace is re-read — so two quick clicks never pick the same name.
    reserved_names: Vec<String>,
    /// Redraws when the installed inputs change (a selection, a re-read workspace).
    _subscriptions: Vec<Subscription>,
}

impl EnvironmentsPanel {
    /// The panel for `workspace`'s window.
    pub fn new(workspace: WeakEntity<Workspace>, cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            workspace,
            pending_creates: BTreeMap::new(),
            next_create: 0,
            reserved_names: Vec::new(),
            _subscriptions: vec![cx.observe_global::<SelectorInputs>(|_panel, cx| cx.notify())],
        }
    }

    /// The open workspace's folder, if this window has one.
    fn root(&self, cx: &App) -> Option<PathBuf> {
        let workspace = self.workspace.upgrade()?;
        let opener = WorkspaceOpen::for_window(workspace.entity_id(), cx)?;
        opener.read(cx).root().map(Path::to_path_buf)
    }

    /// `＋ New`: write a new environment into the open workspace, off the foreground.
    fn new_environment(
        &mut self,
        _: &NewEnvironment,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(root) = self.root(cx) else {
            self.report("Open a workspace first — environments are saved in it.", cx);
            return;
        };
        let mut taken: Vec<String> = cx
            .try_global::<SelectorInputs>()
            .map(|inputs| {
                inputs
                    .environments
                    .iter()
                    .map(|environment| environment.name.clone())
                    .collect()
            })
            .unwrap_or_default();
        taken.extend(self.reserved_names.iter().cloned());
        let name = unique_name(&taken);
        self.reserved_names.push(name.clone());
        let environment = Environment {
            id: ItemId::new(),
            name,
            variables: Vec::new(),
            extra: Default::default(),
        };
        let key = self.next_create;
        self.next_create += 1;
        let task = cx.spawn(async move |this, cx| {
            let name = environment.name.clone();
            let written = cx
                .background_executor()
                .spawn(async move {
                    let item = Item {
                        body: ItemBody::Environment(environment),
                        dir: PathBuf::from(ENVIRONMENTS_DIR),
                        path: None,
                    };
                    workspace_store::save_item(&root, &item)
                })
                .await;
            // The panel went away with its window; nobody is left to tell.
            this.update(cx, |panel, cx| {
                panel.pending_creates.remove(&key);
                match written {
                    Ok(_) => panel.announce(created_message(&name), Severity::Success, cx),
                    Err(error) => {
                        // The name was never used; the next create may take it.
                        panel.reserved_names.retain(|reserved| *reserved != name);
                        panel.announce(
                            format!("{name} could not be created — {error}"),
                            Severity::Error,
                            cx,
                        );
                    }
                }
            })
            .ok();
        });
        self.pending_creates.insert(key, task);
    }

    /// Show `message` as an error toast on this window's workspace (AGENTS.md › errors
    /// reach the user).
    fn report(&self, message: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.announce(message, Severity::Error, cx);
    }

    /// Show `message` as a toast at `severity` on this window's workspace. Deferred:
    /// the workspace may be on the update stack.
    fn announce(
        &self,
        message: impl Into<SharedString>,
        severity: Severity,
        cx: &mut Context<Self>,
    ) {
        let message = message.into();
        let workspace = self.workspace.clone();
        cx.defer(move |cx| {
            if workspace
                .update(cx, |workspace, cx| {
                    workspace.show_toast(message, severity, None, cx);
                })
                .is_err()
            {
                // Handled: the window closed; there is no one left to tell.
            }
        });
    }

    /// One environment row: its name, and the "Active" badge on the active one — the
    /// badge's word carries the state, not its colour (REQ-NFR-06).
    fn row(&self, environment: &Environment, active: bool, cx: &App) -> impl IntoElement {
        let colors = cx.theme().colors();
        let id = row_id(environment.id);
        let selector = id.clone();
        let environment_id = environment.id;
        let workspace = self.workspace.clone();
        let name: SharedString = environment.name.clone().into();
        h_flex()
            .id(SharedString::from(id))
            .debug_selector(move || selector)
            .role(Role::ListBoxOption)
            .aria_label(name.clone())
            .aria_selected(active)
            .focusable()
            .tab_stop(true)
            .cursor_pointer()
            .relative()
            .w_full()
            .min_w_0()
            .h(px_to_rems(32.))
            .gap(DynamicSpacing::Base03.rems(cx))
            .px(DynamicSpacing::Base06.rems(cx))
            .rounded_sm()
            .border_1()
            .border_color(colors.border_transparent)
            .focus_visible(|row| row.border_color(colors.border_focused))
            .map(|row| {
                if active {
                    row.bg(colors.element_selected)
                } else {
                    row.hover(|row| row.bg(colors.ghost_element_hover))
                }
            })
            .tooltip(Tooltip::text(name.clone()).builder())
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(Label::new(name).truncate().color(if active {
                        Color::Default
                    } else {
                        Color::Muted
                    })),
            )
            .when(active, |row| {
                let badge_selector = format!("{}-active", row_id(environment_id));
                row.child(
                    div()
                        .debug_selector(move || badge_selector)
                        .flex_none()
                        .child(Badge::new("Active").severity(Severity::Success)),
                )
            })
            .on_click(move |_, _window, cx| {
                select_environment(Some(environment_id), workspace.clone(), cx);
            })
    }
}

impl Render for EnvironmentsPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let has_workspace = self.root(cx).is_some();
        let inputs = cx.try_global::<SelectorInputs>();
        let environments: Vec<Environment> = inputs
            .map(|inputs| inputs.environments.clone())
            .unwrap_or_default();
        let active = inputs.and_then(|inputs| inputs.active);
        let focus_handle = self.focus_handle.clone();

        let root = v_flex()
            .id("environments-panel")
            .key_context(ENVIRONMENTS_CONTEXT)
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::new_environment))
            .size_full()
            .font_ui(cx);

        if !has_workspace {
            return root
                .child(side_empty(
                    NO_WORKSPACE_ID,
                    "Open a workspace to see its environments.",
                    vec![
                        dispatch_button(
                            NO_WORKSPACE_OPEN_ID,
                            "Open workspace…",
                            IconName::FolderOpen,
                            &OpenWorkspace,
                        )
                        .into_any_element(),
                        dispatch_button(
                            "environments-no-workspace-new",
                            "New workspace…",
                            IconName::Plus,
                            &CreateWorkspace,
                        )
                        .into_any_element(),
                    ],
                    cx,
                ))
                .into_any_element();
        }
        if environments.is_empty() {
            return root
                .child(side_empty(
                    EMPTY_ID,
                    "No environments yet. An environment holds the variables for one place \
                     you send requests to, like staging or production.",
                    vec![
                        div()
                            .debug_selector(|| EMPTY_NEW_ID.to_owned())
                            .child(
                                Button::new(EMPTY_NEW_ID, "New environment…")
                                    .size(ButtonSize::Medium)
                                    .style(ButtonStyle::Outlined)
                                    .full_width()
                                    .start_icon(Icon::new(IconName::Plus))
                                    .on_click(move |_, window, cx| {
                                        focus_handle.dispatch_action(&NewEnvironment, window, cx);
                                    }),
                            )
                            .into_any_element(),
                    ],
                    cx,
                ))
                .into_any_element();
        }
        let rows: Vec<AnyElement> = environments
            .iter()
            .map(|environment| {
                self.row(environment, active == Some(environment.id), cx)
                    .into_any_element()
            })
            .collect();
        root.p(DynamicSpacing::Base08.rems(cx))
            .gap(DynamicSpacing::Base04.rems(cx))
            .child(
                h_flex()
                    .w_full()
                    .child(
                        div().flex_1().min_w_0().child(
                            Label::new(if active.is_some() {
                                "Click an environment to make it active."
                            } else {
                                "No environment is active — click one to use it."
                            })
                            .size(LabelSize::Small)
                            .color(Color::Muted),
                        ),
                    )
                    .child(
                        div().debug_selector(|| NEW_BUTTON_ID.to_owned()).child(
                            Button::new(NEW_BUTTON_ID, "New")
                                .size(ButtonSize::Medium)
                                .style(ButtonStyle::Tinted(TintColor::Accent))
                                .start_icon(Icon::new(IconName::Plus))
                                .tooltip(Tooltip::text("New environment"))
                                .on_click(move |_, window, cx| {
                                    focus_handle.dispatch_action(&NewEnvironment, window, cx);
                                }),
                        ),
                    ),
            )
            .child(
                v_flex()
                    .id("environments-list")
                    .role(Role::ListBox)
                    .aria_label(SharedString::new_static("Environments"))
                    .w_full()
                    .gap(DynamicSpacing::Base01.rems(cx))
                    .children(rows),
            )
            .into_any_element()
    }
}

impl Focusable for EnvironmentsPanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Panel for EnvironmentsPanel {
    fn panel_id(&self) -> &'static str {
        PANEL_ID
    }

    fn title(&self, _cx: &App) -> SharedString {
        SharedString::new_static("Environments")
    }

    fn icon(&self) -> IconName {
        IconName::SlidersHorizontal
    }

    fn position(&self, _cx: &App) -> DockPosition {
        DockPosition::Left
    }

    fn create_action(&self, _cx: &App) -> Option<Box<dyn Action>> {
        Some(Box::new(NewEnvironment))
    }
}

/// What a successful create says: the new environment appears in the list once the
/// workspace is re-read, and this tells the user which one it is.
pub(crate) fn created_message(name: &str) -> String {
    format!("Created {name}. Click it in the list to make it active.")
}

/// "New Environment", or the first "New Environment N" no environment already has.
fn unique_name(taken: &[String]) -> String {
    let is_taken = |candidate: &str| taken.iter().any(|name| name == candidate);
    if !is_taken(NEW_ENVIRONMENT_NAME) {
        return NEW_ENVIRONMENT_NAME.to_owned();
    }
    (2..)
        .map(|number| format!("{NEW_ENVIRONMENT_NAME} {number}"))
        .find(|candidate| !is_taken(candidate))
        .unwrap_or_else(|| NEW_ENVIRONMENT_NAME.to_owned())
}

/// A sidebar empty state: one muted line and the buttons that add the first item.
fn side_empty(
    id: &'static str,
    message: &'static str,
    buttons: Vec<AnyElement>,
    cx: &App,
) -> impl IntoElement {
    v_flex()
        .debug_selector(move || id.to_owned())
        .w_full()
        .gap(DynamicSpacing::Base04.rems(cx))
        .px(DynamicSpacing::Base08.rems(cx))
        .py(DynamicSpacing::Base12.rems(cx))
        .child(Label::new(message).color(Color::Muted))
        .children(buttons)
}

/// A full-width button that dispatches an app-level `action`.
fn dispatch_button(
    id: &'static str,
    label: &'static str,
    icon: IconName,
    action: &dyn Action,
) -> impl IntoElement {
    let action = action.boxed_clone();
    div().debug_selector(move || id.to_owned()).child(
        Button::new(id, label)
            .size(ButtonSize::Medium)
            .style(ButtonStyle::Outlined)
            .full_width()
            .start_icon(Icon::new(icon))
            .on_click(move |_, window, cx| window.dispatch_action(action.boxed_clone(), cx)),
    )
}
