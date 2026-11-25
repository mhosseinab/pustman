//! The status bar's readouts (PRD §3): the console count at the left and the active
//! environment at the right — the workbench blueprint's status-bar content (`Console  0`
//! in `docs/postman_redesign_pack/06_visuals/workbench_blueprint.svg`) — wired into every
//! [`Workspace`] window's status bar here in the app shell, the one crate that can see
//! both globals they read: `console`'s [`ConsoleStore`] and `environment_picker`'s
//! [`SelectorInputs`].
//!
//! Since S48u5 the readouts are controls, as in the approved mockup's status bar: the
//! console count toggles the console (`workspace::ToggleConsole`) with Cookies beside it
//! (`cookies::Manage`), and the environment readout ends in the Sidebar toggle
//! (`workspace::ToggleSidebar`). Each shows its key binding and is a tab stop.
//!
//! `Decision:` the extra controls render inside the two existing items rather than as
//! items of their own: the status bar's item list is the shell's contract (one console
//! item left, one environment item right), and Cookies and Sidebar are part of those
//! groups in the mockup, not new regions.
//!
//! `Decision:` Cookies shows no count. The jar is an `Arc<Mutex<_>>` with no change
//! notification, so a count would go stale between sends; a number that can lie is worse
//! than none. "Saved to <collection>" and Globals are not shown: nothing in the app holds
//! a saved-to state or a globals scope to name, and the bar shows nothing that does nothing.
//!
//! The app also starts every window's console collapsed to its one-line strip
//! ([`collapse_console`]): a saved layout, restored after, wins.

use console::ConsoleStore;
use environment_picker::SelectorInputs;
use environment_picker::selector::NO_ENVIRONMENT_LABEL;
use gpui::{App, Context, EntityId, Global, Subscription, WeakEntity};
use pustman_actions::cookies::Manage;
use pustman_actions::workspace::{ToggleConsole, ToggleSidebar};
use ui::prelude::*;
use workspace::Workspace;

/// The debug selector on the console-count readout's rendered element — the one string
/// tests hand to `VisualTestContext::debug_bounds`.
pub(crate) const CONSOLE_COUNT_SELECTOR: &str = "status-console-count";

/// The debug selector on the active-environment readout's rendered element.
pub(crate) const ENVIRONMENT_SELECTOR: &str = "status-environment";

/// The debug selectors (and element ids) of the status bar's controls.
pub(crate) const CONSOLE_TOGGLE_SELECTOR: &str = "status-console-toggle";
pub(crate) const COOKIES_SELECTOR: &str = "status-cookies";
pub(crate) const SIDEBAR_TOGGLE_SELECTOR: &str = "status-sidebar-toggle";

/// The console entry count at the status bar's left: the blueprint's `Console  0`
/// readout (title, count). It follows [`ConsoleStore`] as entries arrive, are cleared,
/// or recording is paused.
pub(crate) struct ConsoleCount {
    count: usize,
    _store_observer: Subscription,
}

impl ConsoleCount {
    fn new(cx: &mut Context<Self>) -> Self {
        let count = cx.global::<ConsoleStore>().len();
        Self {
            count,
            _store_observer: cx.observe_global::<ConsoleStore>(|this, cx| {
                this.count = cx.global::<ConsoleStore>().len();
                cx.notify();
            }),
        }
    }

    /// The text the strip shows: the blueprint's `Console  0` (title, count).
    pub(crate) fn readout(&self) -> String {
        format!("Console  {}", self.count)
    }
}

impl Render for ConsoleCount {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .gap(DynamicSpacing::Base08.rems(cx))
            .child(
                div()
                    .debug_selector(|| CONSOLE_COUNT_SELECTOR.to_owned())
                    .child(status_control(
                        CONSOLE_TOGGLE_SELECTOR,
                        IconName::Terminal,
                        self.readout().into(),
                        "Toggle console",
                        Box::new(ToggleConsole),
                    )),
            )
            .child(status_control(
                COOKIES_SELECTOR,
                IconName::Cookie,
                SharedString::new_static("Cookies"),
                "Manage cookies",
                Box::new(Manage),
            ))
    }
}

/// The active environment at the status bar's right: its name, or the picker's
/// "No environment" label while none is active (pack screen S07's "No active environment"
/// state — nothing is fabricated). It follows [`SelectorInputs`]: the app's wire installs
/// them, and the picker writes a confirmed choice back.
pub(crate) struct ActiveEnvironment {
    name: Option<SharedString>,
    _inputs_observer: Subscription,
}

impl ActiveEnvironment {
    fn new(cx: &mut Context<Self>) -> Self {
        let name = active_environment_name(cx);
        Self {
            name,
            _inputs_observer: cx.observe_global::<SelectorInputs>(|this, cx| {
                this.name = active_environment_name(cx);
                cx.notify();
            }),
        }
    }

    /// The text the strip shows: the active environment's name, or "No environment".
    pub(crate) fn readout(&self) -> SharedString {
        self.name
            .clone()
            .unwrap_or_else(|| SharedString::new_static(NO_ENVIRONMENT_LABEL))
    }
}

impl Render for ActiveEnvironment {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let readout = self.readout();
        h_flex()
            .min_w_0()
            .gap(DynamicSpacing::Base08.rems(cx))
            .child(
                div()
                    .id(ENVIRONMENT_SELECTOR)
                    .debug_selector(|| ENVIRONMENT_SELECTOR.to_owned())
                    .min_w_0()
                    .tooltip(Tooltip::text(readout.clone()).builder())
                    .child(
                        Label::new(readout)
                            .size(LabelSize::Small)
                            .color(Color::Muted)
                            .truncate(),
                    ),
            )
            .child(status_control(
                SIDEBAR_TOGGLE_SELECTOR,
                IconName::PanelLeft,
                SharedString::new_static("Sidebar"),
                "Toggle sidebar",
                Box::new(ToggleSidebar),
            ))
    }
}

/// One status-bar control: an icon, its label and the action's key binding, as the
/// mockup's `▣ Console  Ctrl Alt C`. A click dispatches `action` from the control, so
/// the workspace handler above it answers; the control is a tab stop (Enter/Space run
/// it through the button) and is named for assistive tech by its tooltip title.
fn status_control(
    selector: &'static str,
    icon: IconName,
    label: SharedString,
    what: &'static str,
    action: Box<dyn gpui::Action>,
) -> impl IntoElement {
    let hint = KeyBinding::for_action(action.as_ref());
    let tooltip = Tooltip::for_action(what, action.as_ref());
    div().debug_selector(move || selector.to_owned()).child(
        Button::new(selector, label)
            .size(ButtonSize::Default)
            .style(ButtonStyle::Transparent)
            .label_color(Color::Muted)
            .start_icon(Icon::new(icon).size(IconSize::Small).color(Color::Muted))
            .key_binding(hint)
            .tooltip(tooltip)
            .on_click(move |_event, window, cx| window.dispatch_action(action.boxed_clone(), cx)),
    )
}

/// The installed inputs' active environment's name — `None` while no environment is
/// active, or before the wire installed the inputs (nothing is fabricated).
fn active_environment_name(cx: &App) -> Option<SharedString> {
    let inputs = cx.try_global::<SelectorInputs>()?;
    let active = inputs.active.as_ref()?;
    inputs
        .environments
        .iter()
        .find(|environment| &environment.id == active)
        .map(|environment| SharedString::from(environment.name.clone()))
}

/// The windows that already got their readouts, plus the subscription that wires the
/// next one. The subscription lives here for the app's lifetime — dropping it stops the
/// wiring (AGENTS.md).
#[derive(Default)]
struct StatusItemsWiring {
    started: bool,
    registered: Vec<EntityId>,
    subscriptions: Vec<Subscription>,
}

impl Global for StatusItemsWiring {}

/// Wire the readouts into every workspace window's status bar: the console count on the
/// left, the active environment on the right.
pub(crate) fn init(cx: &mut App) {
    if cx.default_global::<StatusItemsWiring>().started {
        return;
    }
    cx.default_global::<StatusItemsWiring>().started = true;
    let subscription = cx.observe_new::<Workspace>(|_workspace, _window, cx| {
        let workspace = cx.entity().downgrade();
        // Deferred: the window is on the update stack while its workspace is being
        // created (the shape the history-panel wire uses).
        cx.defer(move |cx| add_status_items(&workspace, cx));
    });
    cx.default_global::<StatusItemsWiring>()
        .subscriptions
        .push(subscription);
}

/// Start the window's console as its one-line strip (S48u5): an empty console pane took
/// ~200 px from the request on every launch. This runs as the window opens, before the
/// saved layout is read back, so a layout saved with the console open still wins.
fn collapse_console(workspace: &gpui::Entity<Workspace>, cx: &mut App) {
    workspace.update(cx, |workspace, cx| {
        workspace
            .bottom_dock()
            .update(cx, |dock, cx| dock.set_collapsed(true, cx));
    });
}

/// Add the readouts to one workspace's status bar, at most once per workspace whichever
/// path gets there first.
fn add_status_items(workspace: &WeakEntity<Workspace>, cx: &mut App) {
    let Some(workspace) = workspace.upgrade() else {
        // Gone with its window before the deferred wire ran.
        return;
    };
    let workspace_id = workspace.entity_id();
    if cx
        .default_global::<StatusItemsWiring>()
        .registered
        .contains(&workspace_id)
    {
        return;
    }
    collapse_console(&workspace, cx);
    let console_count = cx.new(ConsoleCount::new);
    let environment = cx.new(ActiveEnvironment::new);
    workspace.update(cx, |workspace, cx| {
        let status_bar = workspace.status_bar();
        status_bar.update(cx, |status_bar, cx| {
            status_bar.add_left_item(console_count, cx);
            status_bar.add_right_item(environment, cx);
        });
    });
    cx.default_global::<StatusItemsWiring>()
        .registered
        .push(workspace_id);
}

#[cfg(test)]
#[path = "status_items_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "status_controls_tests.rs"]
mod status_controls_tests;
