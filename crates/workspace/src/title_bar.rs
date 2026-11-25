//! The title bar along the top of every window (S48u2) and the window frame Pustman draws
//! when the platform leaves decorations to the app.
//!
//! [`TitleBar`] is always on screen: the app's mark, the in-window menu bar (the app's one
//! menu model, [`install_app_menus`]), the workspace switcher and the search box that
//! opens quick open. When the platform leaves decorations to the app — GNOME on Wayland
//! has no server-side decorations, so without this the window cannot be moved, resized or
//! closed — its empty stretches also drag the window (double-click maximizes, right-click
//! shows the window menu) and it carries the minimize/maximize/close buttons, while
//! [`client_frame`] is the invisible band around the content that resizes the window.
//! The frame's shape follows GPUI's Apache-2.0 `window_shadow` example; the code is our
//! own.

use std::collections::BTreeSet;
use std::rc::Rc;

use gpui::{
    Action, Anchor, AnyElement, App, Bounds, ClickEvent, Context, CursorStyle, Decorations, Entity,
    EntityId, FocusHandle, Focusable, Global, HitboxBehavior, IntoElement,
    KeyBindingContextPredicate, Menu, MouseButton, MouseDownEvent, OwnedMenu, OwnedMenuItem,
    ParentElement, Pixels, Point, Render, ResizeEdge, Role, Size, Subscription, Tiling, WeakEntity,
    WeakFocusHandle, Window, anchored, canvas, deferred, point, px,
};
use pustman_actions::quick_open;
use pustman_actions::workspace::{CreateWorkspace, OpenWorkspace};
use ui::components::context_menu::ContextMenuEvent;
use ui::prelude::*;
use ui::{Icon, IconName, IconSize, keymap_binding_text, px_to_rems};
use workspace_store::ItemBody;

use crate::CloseWindow;
use crate::recent_workspaces::{
    OpenRecentWorkspace, RecentWorkspaces, disambiguation, recent_workspaces,
};
use crate::workspace_open::WorkspaceOpen;

/// The window-frame title bar's test hook: on screen only in a client-decorated window.
pub(crate) const TITLE_BAR_SELECTOR: &str = "title-bar";
/// The application title bar's element id and test hook: on screen in every window.
pub(crate) const APP_TITLE_BAR_SELECTOR: &str = "app-title-bar";
/// The workspace switcher's test hook.
pub(crate) const SWITCHER_SELECTOR: &str = "workspace-switcher";
/// The search box's element id and test hook.
pub(crate) const SEARCH_SELECTOR: &str = "title-bar-search";
/// What the switcher says before a workspace is open.
pub(crate) const NO_WORKSPACE_LABEL: &str = "No workspace";
/// The search box's text.
const SEARCH_LABEL: &str = "Search requests and actions";
/// The longest workspace name the switcher shows whole; a longer one is cut with an
/// ellipsis and read in full from its tooltip.
pub(crate) const SWITCHER_NAME_MAX_CHARS: usize = 32;
/// The switcher's two verbs, after the recent workspaces.
const NEW_WORKSPACE_ENTRY: &str = "New workspace…";
const OPEN_WORKSPACE_ENTRY: &str = "Open workspace…";
/// The end-of-row mark of an entry that opens a submenu.
const SUBMENU_HINT: &str = "▸";
/// The key context the bar's arrows (Left/Right between open menus) are bound in.
const TITLE_BAR_CONTEXT: &str = "TitleBar";

/// A menu title's test hook: `app-menu-File`.
pub(crate) fn menu_title_selector(name: &str) -> String {
    format!("app-menu-{name}")
}
/// The window control buttons: element ids and debug selectors both.
pub(crate) const MINIMIZE_SELECTOR: &str = "title-bar-minimize";
pub(crate) const MAXIMIZE_SELECTOR: &str = "title-bar-maximize";
pub(crate) const CLOSE_SELECTOR: &str = "title-bar-close";

/// The title bar's height: Blueprint A's title strip (the approved S48u mockup's 44 px).
pub(crate) const TITLE_BAR_HEIGHT_PX: f32 = 44.;
/// The height of the controls a popover hangs under (`ButtonSize::Medium`).
const ANCHOR_HEIGHT_PX: f32 = 28.;
/// The app mark's side.
const MARK_SIZE_PX: f32 = 24.;
/// The search box's width (the mockup's 360 px).
const SEARCH_WIDTH_PX: f32 = 360.;

/// How wide the resize band around an untiled window is. It is window geometry the
/// compositor is told about (`set_client_inset`), not layout spacing, so it is fixed
/// pixels rather than a density-scaled `DynamicSpacing` step.
pub(crate) const RESIZE_INSET_PX: f32 = 8.;

/// The window edge or corner `position` resizes, or `None` over the content. A tiled edge
/// has no band (the compositor pins it), so it never resizes.
pub(crate) fn resize_edge(
    position: Point<Pixels>,
    inset: Pixels,
    size: Size<Pixels>,
    tiling: Tiling,
) -> Option<ResizeEdge> {
    let top = !tiling.top && position.y < inset;
    let bottom = !tiling.bottom && position.y > size.height - inset;
    let left = !tiling.left && position.x < inset;
    let right = !tiling.right && position.x > size.width - inset;
    match (top, bottom, left, right) {
        (true, _, true, _) => Some(ResizeEdge::TopLeft),
        (true, _, _, true) => Some(ResizeEdge::TopRight),
        (true, _, _, _) => Some(ResizeEdge::Top),
        (_, true, true, _) => Some(ResizeEdge::BottomLeft),
        (_, true, _, true) => Some(ResizeEdge::BottomRight),
        (_, true, _, _) => Some(ResizeEdge::Bottom),
        (_, _, true, _) => Some(ResizeEdge::Left),
        (_, _, _, true) => Some(ResizeEdge::Right),
        _ => None,
    }
}

fn resize_cursor(edge: ResizeEdge) -> CursorStyle {
    match edge {
        ResizeEdge::Top | ResizeEdge::Bottom => CursorStyle::ResizeUpDown,
        ResizeEdge::Left | ResizeEdge::Right => CursorStyle::ResizeLeftRight,
        ResizeEdge::TopLeft | ResizeEdge::BottomRight => CursorStyle::ResizeUpLeftDownRight,
        ResizeEdge::TopRight | ResizeEdge::BottomLeft => CursorStyle::ResizeUpRightDownLeft,
    }
}

/// Wrap the workspace `content` in the resize band a client-decorated window needs.
pub(crate) fn client_frame(
    tiling: Tiling,
    content: impl IntoElement,
    window: &mut Window,
    cx: &App,
) -> AnyElement {
    let inset = px(RESIZE_INSET_PX);
    // A request to the platform about the surface's geometry, not a state change: it does
    // not notify, so it is safe in render (GPUI's own example sets it there).
    window.set_client_inset(inset);
    let colors = cx.theme().colors();

    div()
        .id("window-frame")
        .size_full()
        .when(!tiling.top, |frame| frame.pt(inset))
        .when(!tiling.bottom, |frame| frame.pb(inset))
        .when(!tiling.left, |frame| frame.pl(inset))
        .when(!tiling.right, |frame| frame.pr(inset))
        .child(
            canvas(
                |_bounds, window, _cx| {
                    let size = window.window_bounds().get_bounds().size;
                    window.insert_hitbox(
                        Bounds::new(point(px(0.), px(0.)), size),
                        HitboxBehavior::Normal,
                    )
                },
                move |_bounds, hitbox, window, _cx| {
                    let size = window.window_bounds().get_bounds().size;
                    if let Some(edge) = resize_edge(window.mouse_position(), inset, size, tiling) {
                        window.set_cursor_style(resize_cursor(edge), &hitbox);
                    }
                },
            )
            .size_full()
            .absolute(),
        )
        // Repaint so the cursor follows the band; the content stops these moves below.
        .on_mouse_move(|_event, window, _cx| window.refresh())
        .on_mouse_down(MouseButton::Left, move |event, window, _cx| {
            let size = window.window_bounds().get_bounds().size;
            if let Some(edge) = resize_edge(event.position, inset, size, tiling) {
                window.start_window_resize(edge);
            }
        })
        .child(
            div()
                .size_full()
                .cursor(CursorStyle::Arrow)
                .border_color(colors.border)
                .when(!tiling.top, |content| content.border_t_1())
                .when(!tiling.bottom, |content| content.border_b_1())
                .when(!tiling.left, |content| content.border_l_1())
                .when(!tiling.right, |content| content.border_r_1())
                .when(!tiling.is_tiled(), |content| content.shadow_md())
                .on_mouse_move(|_event, _window, cx| cx.stop_propagation())
                .child(content),
        )
        .into_any_element()
}

/// The application title bar (S48u2): the app's mark, the in-window menu bar, the
/// workspace switcher, the search box that opens quick open and — in a client-decorated
/// window — the drag area and the window controls.
///
/// `Decision:` the menu bar draws the app's one menu model, the [`gpui::Menu`]s the app
/// installs with [`install_app_menus`] (`cx.set_menus`, read back with `cx.get_menus`).
/// macOS shows that same model in its native menu bar, so the in-window bar is drawn
/// everywhere else only. An entry is enabled when its action has a handler where the user
/// was when the menu opened (the focused element's dispatch path, or an app-level
/// handler), the check macOS runs for its native menu; a disabled entry is shown and
/// never runs.
pub struct TitleBar {
    /// Set by a press on the drag area and spent by the first move after it: starting the
    /// compositor's move on the press itself would swallow the second click of a
    /// double-click, so maximize-by-double-click could never fire. Any left-button release
    /// disarms it — `on_mouse_up` over the drag area, `on_mouse_up_out` anywhere else — so a
    /// press released elsewhere can never arm a move on a later hover.
    should_move: bool,
    /// The bar's own focus, never a tab stop: it tells "focus is in the title bar" (a
    /// title, the switcher, the search box or an open menu) from "focus is in the
    /// content".
    pub(crate) focus_handle: FocusHandle,
    /// The workspace this bar belongs to: the key its [`WorkspaceOpen`] is found under.
    workspace_id: EntityId,
    /// Where focus goes back to when a menu closes and nothing else had it.
    workspace_focus: FocusHandle,
    /// The last element outside the bar that had the keyboard — the user's working
    /// context. [`Self::note_content_focus`] keeps it current from the shell's capture
    /// phase, before any key or press can move focus into the bar.
    content_focus: Option<WeakFocusHandle>,
    /// The menu or switcher popover on screen, one at a time.
    open: Option<OpenPopover>,
    /// The anchor whose popover a pointer press outside it just closed, with where that
    /// press went down: only the click made of that very press (a click on the open
    /// menu's own title) is kept from reopening it. Any other click ignores it, so it
    /// can never go stale.
    closed_by_press: Option<(PopoverAnchor, Point<Pixels>)>,
    _subscriptions: Vec<Subscription>,
}

/// What a popover hangs under.
#[derive(Clone, Debug, PartialEq, Eq)]
enum PopoverAnchor {
    /// A top-level menu, by its name in the model.
    Menu(SharedString),
    /// The workspace switcher.
    Switcher,
}

/// One open popover: the menu entity, the anchor it hangs under, and where focus returns.
struct OpenPopover {
    anchor: PopoverAnchor,
    menu: Entity<ContextMenu>,
    /// The element that had focus when the first popover opened. Actions run from there,
    /// so a menu entry acts exactly like its key binding would have.
    restore_focus: FocusHandle,
    _subscription: Subscription,
}

// The title-bar actions: the arrows walk from an open menu to its neighbours.
gpui::actions!(
    title_bar,
    [
        /// Opens the next menu in the bar while one is open.
        SelectNextMenu,
        /// Opens the previous menu in the bar while one is open.
        SelectPreviousMenu,
    ]
);

// `gpui` re-exports the `schemars` its `Action` schema API is built on; the derive's
// generated impl resolves through this alias (the `environment::EditSecret` shape).
use gpui::private::schemars;

/// Opens the menu bar's menu whose name starts with `mnemonic` — Alt+F opens File.
#[derive(Clone, PartialEq, Eq, Debug, serde::Deserialize, schemars::JsonSchema, gpui::Action)]
#[action(namespace = title_bar)]
pub struct OpenApplicationMenu {
    /// The menu's access letter, lower case.
    #[serde(default)]
    pub mnemonic: String,
}

/// The mnemonics already bound, so reinstalling the model (its Open recent list changes)
/// never stacks duplicate bindings.
#[derive(Default)]
struct BoundMnemonics(BTreeSet<String>);

impl Global for BoundMnemonics {}

/// Set when the platform's own menu bar shows the app menus (macOS). Decided at runtime by
/// the binary, not by `cfg!`, so the in-window bar — and its tests — work on every platform
/// whenever no native bar has been claimed.
#[derive(Default)]
struct NativeMenuBar(bool);

impl Global for NativeMenuBar {}

/// Claim the platform's native menu bar for the app menus: the title bar then draws no
/// in-window menus and binds no Alt access keys. Call before [`install_app_menus`].
pub fn use_native_menu_bar(cx: &mut App) {
    cx.set_global(NativeMenuBar(true));
}

fn native_menu_bar(cx: &App) -> bool {
    cx.try_global::<NativeMenuBar>()
        .is_some_and(|native| native.0)
}

/// Install the app's menu model: the native menu bar on macOS and the in-window one
/// elsewhere both draw it. Off macOS each menu's first letter becomes its Alt access key.
pub fn install_app_menus(menus: Vec<Menu>, cx: &mut App) {
    if !native_menu_bar(cx) {
        let mnemonics: Vec<String> = menus
            .iter()
            .filter_map(|menu| menu.name.chars().next())
            .map(|letter| letter.to_lowercase().to_string())
            .collect();
        let context = match KeyBindingContextPredicate::parse(crate::WORKSPACE_CONTEXT) {
            Ok(context) => Some(Rc::new(context)),
            Err(error) => {
                log::error!("the menu access keys have no context to bind in: {error}");
                None
            }
        };
        let mut bindings = Vec::new();
        for mnemonic in mnemonics.into_iter().filter(|_| context.is_some()) {
            if !cx
                .default_global::<BoundMnemonics>()
                .0
                .insert(mnemonic.clone())
            {
                continue;
            }
            let keystroke = format!("alt-{mnemonic}");
            match gpui::KeyBinding::load(
                &keystroke,
                Box::new(OpenApplicationMenu { mnemonic }),
                context.clone(),
                false,
                None,
                cx,
            ) {
                Ok(binding) => bindings.push(binding),
                // A menu named with a character no key spells has no access key; the
                // menu is still one click or one Tab away.
                Err(error) => log::warn!("no access key for a menu: {error}"),
            }
        }
        cx.bind_keys(bindings);
    }
    cx.set_menus(menus);
}

/// Bind the arrows that walk the menu bar while one of its menus has the keyboard.
pub(crate) fn init(cx: &mut App) {
    let context = format!("{TITLE_BAR_CONTEXT} > Menu");
    cx.bind_keys([
        gpui::KeyBinding::new("right", SelectNextMenu, Some(&context)),
        gpui::KeyBinding::new("left", SelectPreviousMenu, Some(&context)),
    ]);
}

/// The name the switcher shows: the open workspace's own name, its folder's name when
/// the workspace file has none, or [`NO_WORKSPACE_LABEL`].
pub(crate) fn workspace_display_name(workspace_id: EntityId, cx: &App) -> SharedString {
    WorkspaceOpen::for_window(workspace_id, cx)
        .and_then(|opener| {
            let snapshot = opener.read(cx).snapshot()?;
            let name = snapshot
                .items
                .values()
                .find_map(|item| match &item.body {
                    ItemBody::Workspace(workspace) if !workspace.name.trim().is_empty() => {
                        Some(workspace.name.clone())
                    }
                    _ => None,
                })
                .or_else(|| {
                    snapshot
                        .root
                        .file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                })?;
            Some(SharedString::from(name))
        })
        .unwrap_or_else(|| SharedString::new_static(NO_WORKSPACE_LABEL))
}

/// The switcher's label for `name`: whole when it fits [`SWITCHER_NAME_MAX_CHARS`], else
/// cut with an ellipsis. `Decision:` a character budget rather than a measured width —
/// the bar's other items have fixed widths, so the budget is what keeps the search box
/// centred, and the tooltip carries the full name.
pub(crate) fn switcher_label(name: &str) -> SharedString {
    if name.chars().count() <= SWITCHER_NAME_MAX_CHARS {
        return SharedString::from(name.to_owned());
    }
    let kept: String = name
        .chars()
        .take(SWITCHER_NAME_MAX_CHARS.saturating_sub(1))
        .collect();
    SharedString::from(format!("{}…", kept.trim_end()))
}

/// The application menus the in-window bar draws; none on macOS, whose native bar shows
/// the same model.
fn in_window_menus(cx: &App) -> Vec<OwnedMenu> {
    if native_menu_bar(cx) {
        return Vec::new();
    }
    cx.get_menus().unwrap_or_default()
}

/// Whether `action` can run from where the user was: a handler on the focused element's
/// dispatch path, or an app-level handler (`globally`, taken from the available actions).
fn is_runnable(
    action: &dyn Action,
    restore_focus: &FocusHandle,
    globally: &[Box<dyn Action>],
    window: &Window,
) -> bool {
    window.is_action_available_in(action, restore_focus)
        || globally
            .iter()
            .any(|available| available.as_any().type_id() == action.as_any().type_id())
}

/// Fill `menu` with the model's `items`: runnable actions as entries with their keymap
/// binding, the rest disabled, submenus as entries that open them in place.
fn populate(
    mut menu: ContextMenu,
    items: &[OwnedMenuItem],
    restore_focus: &FocusHandle,
    globally: &[Box<dyn Action>],
    bar: &WeakEntity<TitleBar>,
    window: &Window,
    cx: &App,
) -> ContextMenu {
    for item in items {
        match item {
            OwnedMenuItem::Separator => menu = menu.separator(),
            OwnedMenuItem::Action {
                name,
                action,
                disabled,
                ..
            } => {
                let runnable =
                    !disabled && is_runnable(action.as_ref(), restore_focus, globally, window);
                menu = if runnable {
                    let action = action.boxed_clone();
                    let target = restore_focus.clone();
                    menu.entry(name.clone(), move |_event, window, cx| {
                        run_from(&target, action.boxed_clone(), window, cx);
                    })
                } else {
                    menu.disabled_entry(name.clone())
                };
                if let Some(keys) = keymap_binding_text(action.as_ref(), cx) {
                    menu = menu.hint(keys);
                }
            }
            OwnedMenuItem::Submenu(submenu) => {
                if submenu.disabled || submenu.items.is_empty() {
                    menu = menu.disabled_entry(submenu.name.clone());
                } else {
                    let items = submenu.items.clone();
                    let bar = bar.clone();
                    menu = menu.entry(submenu.name.clone(), move |_event, window, cx| {
                        let items = items.clone();
                        if let Err(error) =
                            bar.update(cx, |bar, cx| bar.open_submenu(&items, window, cx))
                        {
                            // The bar went with its window: there is no menu left to open
                            // and no one to tell.
                            log::debug!("the title bar closed before its submenu opened: {error}");
                        }
                    });
                }
                menu = menu.hint(SharedString::new_static(SUBMENU_HINT));
            }
            // macOS-only system menus (Services) have no in-window form.
            OwnedMenuItem::SystemMenu(_) => {}
        }
    }
    menu
}

/// Run `action` from `target`, the element that had focus before the menu: focus goes
/// back there first, so the action meets the same handlers its key binding would.
fn run_from(target: &FocusHandle, action: Box<dyn Action>, window: &mut Window, cx: &mut App) {
    window.focus(target, cx);
    window.dispatch_action(action, cx);
}

impl TitleBar {
    /// The title bar of the workspace `workspace_id`, whose root takes focus back from a
    /// closed menu when nothing else had it.
    pub fn new(
        workspace_id: EntityId,
        workspace_focus: FocusHandle,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            should_move: false,
            focus_handle: cx.focus_handle(),
            workspace_id,
            workspace_focus,
            content_focus: None,
            open: None,
            closed_by_press: None,
            // A new recent list redraws the switcher and the File › Open recent entries.
            _subscriptions: vec![cx.observe_global::<RecentWorkspaces>(|_bar, cx| cx.notify())],
        }
    }

    /// Where focus returns and actions run from — the user's working context: kept from
    /// the first popover while the user walks between menus; else whatever has focus now
    /// when that is outside the bar; else the last element outside the bar that had it
    /// (a title reached from the keyboard); else the workspace root.
    fn restore_focus(&self, window: &Window, cx: &App) -> FocusHandle {
        if let Some(open) = self.open.as_ref() {
            return open.restore_focus.clone();
        }
        window
            .focused(cx)
            .filter(|_| !self.focus_handle.contains_focused(window, cx))
            .or_else(|| {
                self.content_focus
                    .as_ref()
                    .and_then(WeakFocusHandle::upgrade)
            })
            .unwrap_or_else(|| self.workspace_focus.clone())
    }

    /// Remember what has the keyboard when it is outside the bar. The shell calls this
    /// in the capture phase of every key and pointer press, so the working context is
    /// known before the press can move focus into the bar.
    pub(crate) fn note_content_focus(&mut self, window: &Window, cx: &App) {
        if let Some(focused) = window.focused(cx)
            && !self.focus_handle.contains_focused(window, cx)
        {
            self.content_focus = Some(focused.downgrade());
        }
    }

    /// Open the menu whose name starts with `mnemonic` (Alt+letter).
    pub(crate) fn open_menu_by_mnemonic(
        &mut self,
        mnemonic: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let name = in_window_menus(cx).into_iter().find_map(|menu| {
            menu.name
                .to_lowercase()
                .starts_with(mnemonic)
                .then_some(menu.name)
        });
        if let Some(name) = name {
            self.open_menu(name, window, cx);
        }
    }

    /// Open the top-level menu `name` under its title.
    fn open_menu(&mut self, name: SharedString, window: &mut Window, cx: &mut Context<Self>) {
        let Some(model) = in_window_menus(cx)
            .into_iter()
            .find(|menu| menu.name == name)
        else {
            return;
        };
        self.show(PopoverAnchor::Menu(name), &model.items, window, cx);
    }

    /// Replace the open menu with one of its submenus, in place (File › Open recent).
    fn open_submenu(
        &mut self,
        items: &[OwnedMenuItem],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let anchor = self
            .open
            .as_ref()
            .map(|open| open.anchor.clone())
            .unwrap_or(PopoverAnchor::Switcher);
        self.show(anchor, items, window, cx);
    }

    /// Build a menu of `items` under `anchor`, focus it and show it.
    fn show(
        &mut self,
        anchor: PopoverAnchor,
        items: &[OwnedMenuItem],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let restore_focus = self.restore_focus(window, cx);
        let globally = window.available_actions(cx);
        let bar = cx.weak_entity();
        let menu = cx.new(|cx| {
            populate(
                ContextMenu::new(cx),
                items,
                &restore_focus,
                &globally,
                &bar,
                window,
                cx,
            )
        });
        self.present(anchor, menu, restore_focus, window, cx);
    }

    /// The workspace switcher's popover: the recent workspaces, then New workspace… and
    /// Open workspace….
    fn open_switcher(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let restore_focus = self.restore_focus(window, cx);
        let recents = recent_workspaces(cx).to_vec();
        let menu = cx.new(|cx| {
            let mut menu = ContextMenu::new(cx);
            for (index, recent) in recents.iter().enumerate() {
                let action = OpenRecentWorkspace {
                    path: recent.path.to_string_lossy().into_owned(),
                };
                let target = restore_focus.clone();
                menu = menu.entry(recent.name.clone(), move |_event, window, cx| {
                    run_from(&target, Box::new(action.clone()), window, cx);
                });
                // Two recent workspaces of the same name are told apart by where they
                // live; a unique name needs no path beside it.
                if let Some(path) = disambiguation(&recents, index) {
                    menu = menu.hint(path);
                }
            }
            if !recents.is_empty() {
                menu = menu.separator();
            }
            let new_target = restore_focus.clone();
            menu = menu.entry(NEW_WORKSPACE_ENTRY, move |_event, window, cx| {
                run_from(&new_target, Box::new(CreateWorkspace), window, cx);
            });
            if let Some(keys) = keymap_binding_text(&CreateWorkspace, cx) {
                menu = menu.hint(keys);
            }
            let open_target = restore_focus.clone();
            menu = menu.entry(OPEN_WORKSPACE_ENTRY, move |_event, window, cx| {
                run_from(&open_target, Box::new(OpenWorkspace), window, cx);
            });
            if let Some(keys) = keymap_binding_text(&OpenWorkspace, cx) {
                menu = menu.hint(keys);
            }
            menu
        });
        self.present(PopoverAnchor::Switcher, menu, restore_focus, window, cx);
    }

    /// Put `menu` on screen under `anchor` with the keyboard on it.
    fn present(
        &mut self,
        anchor: PopoverAnchor,
        menu: Entity<ContextMenu>,
        restore_focus: FocusHandle,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let subscription = cx.subscribe_in(&menu, window, Self::on_menu_event);
        menu.update(cx, |menu, cx| menu.focus(window, cx));
        self.closed_by_press = None;
        self.open = Some(OpenPopover {
            anchor,
            menu,
            restore_focus,
            _subscription: subscription,
        });
        cx.notify();
    }

    /// A popover is done — an entry ran or Escape backed out. A menu that a submenu or a
    /// neighbour already replaced is not the open one any more and changes nothing.
    fn on_menu_event(
        &mut self,
        menu: &Entity<ContextMenu>,
        event: &ContextMenuEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if *event != ContextMenuEvent::Dismissed {
            return;
        }
        if self.open.as_ref().is_some_and(|open| &open.menu == menu) {
            self.close(window, cx);
        }
    }

    /// Close the popover and give focus back to where the user was — unless an entry
    /// already moved it on (a menu entry that opened a modal keeps the modal focused).
    fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(open) = self.open.take() {
            let menu_focus = open.menu.read(cx).focus_handle(cx);
            if menu_focus.contains_focused(window, cx) || window.focused(cx).is_none() {
                window.focus(&open.restore_focus, cx);
            }
            cx.notify();
        }
    }

    /// A press outside the open popover closes it. Where it went down is kept: if the
    /// press was on this popover's own title, the click it completes must not reopen it.
    fn close_on_outside_press(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.closed_by_press = self
            .open
            .as_ref()
            .map(|open| (open.anchor.clone(), event.position));
        self.close(window, cx);
    }

    /// A click on an anchor: open its popover, or close it when it was the one open —
    /// including the click whose press just closed it from outside.
    fn toggle(
        &mut self,
        anchor: PopoverAnchor,
        event: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let completes_the_closing_press = match (self.closed_by_press.take(), event) {
            (Some((closed, pressed_at)), ClickEvent::Mouse(click)) => {
                closed == anchor && click.down.position == pressed_at
            }
            _ => false,
        };
        if completes_the_closing_press {
            return;
        }
        if self.open.as_ref().is_some_and(|open| open.anchor == anchor) {
            self.close(window, cx);
            return;
        }
        match anchor {
            PopoverAnchor::Menu(name) => self.open_menu(name, window, cx),
            PopoverAnchor::Switcher => self.open_switcher(window, cx),
        }
    }

    /// Left/Right while a menu is open: open the neighbour, wrapping at the ends.
    fn step_menu(&mut self, forward: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(PopoverAnchor::Menu(current)) = self.open.as_ref().map(|open| open.anchor.clone())
        else {
            return;
        };
        let names: Vec<SharedString> = in_window_menus(cx)
            .into_iter()
            .map(|menu| menu.name)
            .collect();
        let Some(index) = names.iter().position(|name| *name == current) else {
            return;
        };
        let count = names.len();
        let next = if forward {
            (index + 1) % count
        } else {
            (index + count - 1) % count
        };
        if let Some(name) = names.get(next).cloned() {
            self.open_menu(name, window, cx);
        }
    }

    fn select_next_menu(
        &mut self,
        _: &SelectNextMenu,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.step_menu(true, window, cx);
    }

    fn select_previous_menu(
        &mut self,
        _: &SelectPreviousMenu,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.step_menu(false, window, cx);
    }

    /// The popover hanging under `anchor`, if that is the open one.
    fn popover_under(&self, anchor: &PopoverAnchor, cx: &mut Context<Self>) -> Option<AnyElement> {
        let open = self.open.as_ref().filter(|open| &open.anchor == anchor)?;
        // `ui::floating_menu`'s shape — anchored under the trigger, snapped to the
        // window — with the outside-press watch on the menu's own box: a watch on the
        // zero-size slot would count every press on the menu as outside it.
        Some(
            div()
                .absolute()
                .top(px_to_rems(ANCHOR_HEIGHT_PX))
                .left_0()
                .child(
                    deferred(
                        anchored()
                            .anchor(Anchor::TopLeft)
                            .snap_to_window_with_margin(DynamicSpacing::Base04.px(cx))
                            .child(
                                div()
                                    .id("title-bar-popover")
                                    .on_mouse_down_out(cx.listener(
                                        |bar, event: &MouseDownEvent, window, cx| {
                                            bar.close_on_outside_press(event, window, cx);
                                        },
                                    ))
                                    .child(open.menu.clone()),
                            ),
                    )
                    .with_priority(1),
                )
                .into_any_element(),
        )
    }

    /// One menu title: opens its menu on click, and follows the pointer while another
    /// menu is open, the way a desktop menu bar does.
    fn render_menu_title(&self, name: SharedString, cx: &mut Context<Self>) -> impl IntoElement {
        let selector = menu_title_selector(&name);
        let anchor = PopoverAnchor::Menu(name.clone());
        let is_open = self.open.as_ref().is_some_and(|open| open.anchor == anchor);
        let mnemonic = name
            .chars()
            .next()
            .map(|letter| letter.to_uppercase().to_string())
            .unwrap_or_default();
        let tooltip = if native_menu_bar(cx) || mnemonic.is_empty() {
            Tooltip::text(format!("{name} menu"))
        } else {
            Tooltip::text(format!("{name} menu (Alt+{mnemonic})"))
        };
        let click_anchor = anchor.clone();
        let hover_name = name.clone();
        div()
            .id(SharedString::from(format!("{selector}-slot")))
            .debug_selector(move || selector.clone())
            .relative()
            .on_hover(cx.listener(move |bar, hovered: &bool, window, cx| {
                let another_is_open = bar.open.as_ref().is_some_and(|open| {
                    matches!(&open.anchor, PopoverAnchor::Menu(open_name) if *open_name != hover_name)
                });
                if *hovered && another_is_open {
                    bar.open_menu(hover_name.clone(), window, cx);
                }
            }))
            .child(
                Button::new(SharedString::from(format!("app-menu-button-{name}")), name.clone())
                    .size(ButtonSize::Medium)
                    .style(if is_open {
                        ButtonStyle::Tinted(TintColor::Accent)
                    } else {
                        ButtonStyle::Subtle
                    })
                    .when(!is_open, |button| button.label_color(Color::Muted))
                    .tooltip(tooltip.hidden(is_open))
                    .on_click(cx.listener(move |bar, event, window, cx| {
                        bar.toggle(click_anchor.clone(), event, window, cx);
                    })),
            )
            .children(self.popover_under(&anchor, cx))
    }

    /// The workspace switcher: the open workspace's name and a chevron.
    fn render_switcher(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let name = workspace_display_name(self.workspace_id, cx);
        let is_open = self
            .open
            .as_ref()
            .is_some_and(|open| open.anchor == PopoverAnchor::Switcher);
        div()
            .id("workspace-switcher-slot")
            .debug_selector(|| SWITCHER_SELECTOR.to_owned())
            .relative()
            .flex_shrink_1()
            .min_w_0()
            .ml(DynamicSpacing::Base06.rems(cx))
            .child(
                Button::new("workspace-switcher-button", switcher_label(&name))
                    .size(ButtonSize::Medium)
                    .style(if is_open {
                        ButtonStyle::Tinted(TintColor::Accent)
                    } else {
                        ButtonStyle::Subtle
                    })
                    .end_icon(Icon::new(IconName::ChevronDown).size(IconSize::Small))
                    .tooltip(Tooltip::text(format!("Switch workspace — {name}")).hidden(is_open))
                    .on_click(cx.listener(|bar, event, window, cx| {
                        bar.toggle(PopoverAnchor::Switcher, event, window, cx);
                    })),
            )
            .children(self.popover_under(&PopoverAnchor::Switcher, cx))
    }

    /// The search box: opens quick open, the same as its key binding.
    fn render_search(&self, cx: &mut Context<Self>) -> impl IntoElement {
        // The hint is read from the whole keymap, like the menu entries' hints: the
        // focus-scoped `KeyBinding` hint goes blank while a menu holds the keyboard.
        let keys = keymap_binding_text(&quick_open::Toggle, cx);
        let colors = cx.theme().colors();
        h_flex()
            .id(SEARCH_SELECTOR)
            .debug_selector(|| SEARCH_SELECTOR.to_owned())
            .role(Role::Button)
            .aria_label(SharedString::new_static(SEARCH_LABEL))
            .focusable()
            .tab_stop(true)
            .w(px_to_rems(SEARCH_WIDTH_PX))
            .flex_shrink_1()
            .min_w_0()
            .h(px_to_rems(ANCHOR_HEIGHT_PX))
            .px(DynamicSpacing::Base08.rems(cx))
            .gap(DynamicSpacing::Base04.rems(cx))
            .rounded_sm()
            .border_1()
            .border_color(colors.border)
            .bg(colors.background)
            .cursor_pointer()
            .hover(|style| style.bg(colors.ghost_element_hover))
            .focus_visible(|style| style.border_color(colors.border_focused))
            .tooltip(
                Tooltip::text(match &keys {
                    Some(keys) => format!("Quick open ({keys})"),
                    None => String::from("Quick open"),
                })
                .builder(),
            )
            // Quick open runs from the working context, so it hands the keyboard back
            // there when it closes; the box itself never keeps focus.
            .on_click(cx.listener(|bar, _event, window, cx| {
                let target = bar.restore_focus(window, cx);
                run_from(&target, Box::new(quick_open::Toggle), window, cx);
            }))
            .child(
                Icon::new(IconName::Search)
                    .size(IconSize::Small)
                    .color(Color::Muted),
            )
            .child(
                div().flex_1().min_w_0().child(
                    Label::new(SEARCH_LABEL)
                        .size(LabelSize::Small)
                        .color(Color::Muted)
                        .truncate(),
                ),
            )
            .children(keys.map(|keys| {
                div()
                    .flex_none()
                    .px(DynamicSpacing::Base02.rems(cx))
                    .rounded_xs()
                    .border_1()
                    .border_color(colors.border_variant)
                    .child(Label::new(keys).size(LabelSize::XSmall).color(Color::Muted))
            }))
    }

    /// A stretch of empty bar: in a client-decorated window it moves the window
    /// (drag), maximizes it (double-click) and shows the window menu (right-click).
    fn drag_region(
        &self,
        id: &'static str,
        client_decorated: bool,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let window_menu = window.window_controls().window_menu;
        h_flex()
            .id(id)
            .flex_1()
            .min_w(DynamicSpacing::Base16.rems(cx))
            .h_full()
            .when(client_decorated, |drag| {
                drag.on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _event, _window, _cx| this.should_move = true),
                )
                .on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|this, _event, _window, _cx| this.should_move = false),
                )
                .on_mouse_up_out(
                    MouseButton::Left,
                    cx.listener(|this, _event, _window, _cx| this.should_move = false),
                )
                .on_mouse_move(cx.listener(|this, _event, window, _cx| {
                    if this.should_move {
                        this.should_move = false;
                        window.start_window_move();
                    }
                }))
                .on_click(|event, window, _cx| {
                    if event.click_count() == 2 {
                        window.zoom_window();
                    }
                })
                .when(window_menu, |drag| {
                    drag.on_mouse_down(MouseButton::Right, |event, window, _cx| {
                        window.show_window_menu(event.position);
                    })
                })
            })
    }
}

fn window_control(
    selector: &'static str,
    icon: IconName,
    label: &'static str,
    action: fn(&mut Window, &mut App),
) -> impl IntoElement {
    div().debug_selector(|| selector.to_owned()).child(
        IconButton::new(selector, icon, Tooltip::text(label))
            .on_click(move |_event, window, cx| action(window, cx)),
    )
}

impl Render for TitleBar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors();
        let client_decorated = matches!(window.window_decorations(), Decorations::Client { .. });
        let controls = window.window_controls();
        let maximize_label = if window.is_maximized() {
            "Restore"
        } else {
            "Maximize"
        };
        let menu_names: Vec<SharedString> = in_window_menus(cx)
            .into_iter()
            .map(|menu| menu.name)
            .collect();

        let bar = h_flex()
            .id(APP_TITLE_BAR_SELECTOR)
            .debug_selector(|| APP_TITLE_BAR_SELECTOR.to_owned())
            .key_context(TITLE_BAR_CONTEXT)
            .track_focus(&self.focus_handle)
            // A press on the bar never moves the keyboard: menus, the switcher and the
            // search box act on the user's working context (see [`Self::restore_focus`]).
            .capture_any_mouse_down(|_event, window, _cx| window.prevent_default())
            .role(Role::MenuBar)
            .aria_label(SharedString::new_static("Pustman"))
            .w_full()
            .h(px_to_rems(TITLE_BAR_HEIGHT_PX))
            .flex_none()
            .bg(colors.title_bar_background)
            .border_b_1()
            .border_color(colors.border_variant)
            .pl(DynamicSpacing::Base12.rems(cx))
            .pr(DynamicSpacing::Base04.rems(cx))
            .gap(DynamicSpacing::Base02.rems(cx))
            .on_action(cx.listener(Self::select_next_menu))
            .on_action(cx.listener(Self::select_previous_menu))
            .child(
                // The app's mark: identity only, it runs nothing.
                div()
                    .flex_none()
                    .size(px_to_rems(MARK_SIZE_PX))
                    .rounded_sm()
                    .bg(colors.accent_background)
                    .text_color(colors.accent_foreground)
                    .flex()
                    .items_center()
                    .justify_center()
                    .mr(DynamicSpacing::Base06.rems(cx))
                    .child(
                        Label::new("P")
                            .size(LabelSize::Small)
                            .color(Color::Custom(colors.accent_foreground)),
                    ),
            )
            .children(
                menu_names
                    .into_iter()
                    .map(|name| self.render_menu_title(name, cx).into_any_element())
                    .collect::<Vec<_>>(),
            )
            .child(self.render_switcher(cx))
            .child(self.drag_region("title-bar-drag", client_decorated, window, cx))
            .child(self.render_search(cx))
            .child(self.drag_region("title-bar-drag-end", client_decorated, window, cx))
            .when(client_decorated && controls.minimize, |bar| {
                bar.child(window_control(
                    MINIMIZE_SELECTOR,
                    IconName::Minus,
                    "Minimize",
                    |window, _cx| window.minimize_window(),
                ))
            })
            .when(client_decorated && controls.maximize, |bar| {
                bar.child(window_control(
                    MAXIMIZE_SELECTOR,
                    IconName::Square,
                    maximize_label,
                    |window, _cx| window.zoom_window(),
                ))
            })
            .when(client_decorated, |bar| {
                bar.child(window_control(
                    CLOSE_SELECTOR,
                    IconName::X,
                    "Close",
                    // The app's own close button must not tear the window down directly:
                    // dispatching the close action makes the shell run the same
                    // save-then-close flush a platform close takes (issue #85).
                    |window, cx| window.dispatch_action(Box::new(CloseWindow), cx),
                ))
            });

        // The window-frame title bar — the strip that moves, maximizes and closes the
        // window — only exists when the platform leaves the frame to the app; its test
        // hook marks the same strip then.
        div()
            .id(TITLE_BAR_SELECTOR)
            .w_full()
            .flex_none()
            .when(client_decorated, |frame| {
                frame.debug_selector(|| TITLE_BAR_SELECTOR.to_owned())
            })
            .child(bar)
    }
}

#[cfg(test)]
#[path = "title_bar_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "title_bar_move_tests.rs"]
mod move_tests;

#[cfg(test)]
#[path = "title_bar_close_tests.rs"]
mod close_tests;

#[cfg(test)]
#[path = "title_bar_menu_tests.rs"]
mod menu_tests;

#[cfg(test)]
#[path = "title_bar_focus_tests.rs"]
mod focus_tests;
