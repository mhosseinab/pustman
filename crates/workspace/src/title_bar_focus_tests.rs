//! The title bar and the user's working context (S48u2 fix 1): a menu entry is enabled
//! and runs against the element the user was working in — however the menu was opened —
//! focus goes back there when a menu closes or the search box hands over to quick open,
//! an outside press never swallows the next click on a title, Alt+letter works while a
//! text field has the keyboard, and File › Open recent drills into its list.
//!
//! The "content" here is a request-editor stand-in in the center pane: a focusable view
//! that handles `request::Send` (as the real editor does) and holds a real text field.

use std::cell::Cell;
use std::rc::Rc;

use gpui::{
    App, Context, Entity, FocusHandle, Focusable, Menu, MenuItem, Modifiers, TestAppContext,
    VisualTestContext, Window, point, px,
};
use pustman_actions::quick_open;
use pustman_actions::request::Send;
use pustman_actions::workspace::{OpenWorkspace, ToggleConsole};
use text_input::TextInput;
use ui::components::context_menu::MENU_SELECTOR;
use ui::prelude::*;
use ui::test_support::{draw_window, init_test, press_key};

use crate::item::Item;
use crate::title_bar::{SEARCH_SELECTOR, menu_title_selector};
use crate::{Workspace, init, install_app_menus};

fn leak(selector: String) -> &'static str {
    Box::leak(selector.into_boxed_str())
}

/// A stand-in for the request editor: it handles Send where it has focus.
struct Editor {
    focus_handle: FocusHandle,
    field: Entity<TextInput>,
    sent: Rc<Cell<usize>>,
}

impl Render for Editor {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let sent = self.sent.clone();
        v_flex()
            .id("test-editor")
            .size_full()
            .key_context("TestEditor")
            .track_focus(&self.focus_handle)
            .on_action(move |_: &Send, _window, _cx| sent.set(sent.get() + 1))
            .child(self.field.clone())
            .child(Label::new("editor"))
    }
}

impl Focusable for Editor {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Item for Editor {
    fn tab_title(&self, _cx: &App) -> SharedString {
        SharedString::new_static("Editor")
    }
}

fn test_menus() -> Vec<Menu> {
    vec![
        Menu::new("File").items([
            MenuItem::action("Open workspace…", OpenWorkspace),
            MenuItem::submenu(
                Menu::new("Open recent").items([MenuItem::action("Toggle console", ToggleConsole)]),
            ),
        ]),
        Menu::new("Request").items([MenuItem::action("Send", Send)]),
    ]
}

struct Harness {
    workspace: Entity<Workspace>,
    editor: Entity<Editor>,
    sent: Rc<Cell<usize>>,
}

fn workspace_with_editor(cx: &mut TestAppContext) -> (Harness, &mut VisualTestContext) {
    cx.update(init_test);
    cx.update(|cx| {
        text_input::init(cx);
        init(cx);
        install_app_menus(test_menus(), cx);
    });
    let (workspace, cx) = cx.add_window_view(Workspace::new);
    cx.update(|window, _cx| window.activate_window());
    let sent = Rc::new(Cell::new(0));
    let editor = cx.update(|window, cx| {
        let sent = sent.clone();
        cx.new(|cx| Editor {
            focus_handle: cx.focus_handle(),
            field: cx.new(|cx| TextInput::new(window, cx)),
            sent,
        })
    });
    cx.update(|_window, cx| {
        let pane = workspace.read(cx).center_pane().clone();
        pane.update(cx, |pane, cx| pane.add_tab_item(editor.clone(), cx));
    });
    draw_window(cx);
    focus_editor(&editor, cx);
    (
        Harness {
            workspace,
            editor,
            sent,
        },
        cx,
    )
}

fn focus_editor(editor: &Entity<Editor>, cx: &mut VisualTestContext) {
    let handle = cx.read(|cx| editor.read(cx).focus_handle.clone());
    cx.update(|window, cx| window.focus(&handle, cx));
    draw_window(cx);
}

fn editor_has_focus(editor: &Entity<Editor>, cx: &mut VisualTestContext) -> bool {
    cx.update(|window, cx| editor.read(cx).focus_handle.is_focused(window))
}

fn click_at(cx: &mut VisualTestContext, selector: &'static str) {
    let bounds = cx
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("{selector} is on screen"));
    cx.simulate_click(bounds.center(), Modifiers::none());
    cx.run_until_parked();
    draw_window(cx);
}

fn keys(cx: &mut VisualTestContext, keystrokes: &str) {
    cx.simulate_keystrokes(keystrokes);
    cx.run_until_parked();
    draw_window(cx);
}

fn menu_is_open(cx: &mut VisualTestContext) -> bool {
    cx.debug_bounds(MENU_SELECTOR).is_some()
}

#[gpui::test]
fn test_escape_returns_focus_to_the_element_the_user_was_working_in(cx: &mut TestAppContext) {
    let (harness, cx) = workspace_with_editor(cx);
    assert!(editor_has_focus(&harness.editor, cx));

    keys(cx, "alt-f");
    assert!(menu_is_open(cx));
    press_key(cx, "escape");
    cx.run_until_parked();
    draw_window(cx);

    assert!(!menu_is_open(cx));
    assert!(
        editor_has_focus(&harness.editor, cx),
        "the editor, not the workspace root, has the keyboard back"
    );
}

#[gpui::test]
fn test_enter_runs_a_content_handled_entry_and_focus_returns_to_the_content(
    cx: &mut TestAppContext,
) {
    let (harness, cx) = workspace_with_editor(cx);

    keys(cx, "alt-r");
    press_key(cx, "enter");
    cx.run_until_parked();
    draw_window(cx);

    assert_eq!(harness.sent.get(), 1, "Send ran in the editor, once");
    assert!(editor_has_focus(&harness.editor, cx));
}

#[gpui::test]
fn test_a_menu_opened_from_inside_the_title_bar_still_enables_what_the_content_handles(
    cx: &mut TestAppContext,
) {
    let (harness, cx) = workspace_with_editor(cx);

    // The key press that moves focus into the bar (Tab, in a view that walks tab stops)
    // reaches the shell while the editor still has the keyboard; then focus lands on the
    // bar, where the menu is opened from.
    press_key(cx, "tab");
    let bar_focus = cx.read(|cx| {
        harness
            .workspace
            .read(cx)
            .title_bar
            .read(cx)
            .focus_handle
            .clone()
    });
    cx.update(|window, cx| window.focus(&bar_focus, cx));
    draw_window(cx);
    assert!(!editor_has_focus(&harness.editor, cx));

    keys(cx, "alt-r");
    press_key(cx, "enter");
    cx.run_until_parked();
    draw_window(cx);

    assert_eq!(
        harness.sent.get(),
        1,
        "Send is enabled and runs in the editor the user was working in"
    );
    assert!(editor_has_focus(&harness.editor, cx));
}

#[gpui::test]
fn test_an_outside_press_does_not_swallow_the_next_click_on_a_title(cx: &mut TestAppContext) {
    let (_harness, cx) = workspace_with_editor(cx);
    let file = leak(menu_title_selector("File"));

    click_at(cx, file);
    assert!(menu_is_open(cx));
    // A press and release somewhere in the content closes the menu.
    cx.simulate_click(point(px(600.), px(400.)), Modifiers::none());
    cx.run_until_parked();
    draw_window(cx);
    assert!(!menu_is_open(cx), "an outside press closes the menu");

    click_at(cx, file);
    assert!(menu_is_open(cx), "the next click on File opens it at once");

    click_at(cx, file);
    assert!(
        !menu_is_open(cx),
        "a click on the open menu's title closes it"
    );
}

#[gpui::test]
fn test_the_search_box_hands_over_to_quick_open_from_the_content(cx: &mut TestAppContext) {
    let (harness, cx) = workspace_with_editor(cx);
    let toggled = Rc::new(Cell::new(0));
    cx.update(|_window, cx| {
        let toggled = toggled.clone();
        cx.on_action(move |_: &quick_open::Toggle, _cx| toggled.set(toggled.get() + 1));
    });

    click_at(cx, SEARCH_SELECTOR);

    assert!(
        editor_has_focus(&harness.editor, cx),
        "the search box never keeps the keyboard; quick open returns it to the editor"
    );
    assert_eq!(toggled.get(), 1, "the click asked for quick open once");
}

#[gpui::test]
fn test_alt_f_opens_file_while_a_text_field_has_the_keyboard(cx: &mut TestAppContext) {
    let (harness, cx) = workspace_with_editor(cx);
    let field = cx.read(|cx| harness.editor.read(cx).field.clone());
    let field_focus = cx.read(|cx| field.read(cx).focus_handle(cx));
    cx.update(|window, cx| window.focus(&field_focus, cx));
    draw_window(cx);

    keys(cx, "alt-f");

    assert!(
        menu_is_open(cx),
        "Alt+F opens File from inside a text field"
    );
    assert_eq!(
        cx.read(|cx| field.read(cx).text()),
        "",
        "and types nothing into the field"
    );
    press_key(cx, "escape");
    cx.run_until_parked();
    draw_window(cx);
    assert!(
        cx.update(|window, _cx| field_focus.is_focused(window)),
        "Escape gives the field its keyboard back"
    );
}

#[gpui::test]
fn test_open_recent_drills_into_its_list_and_runs_the_chosen_entry(cx: &mut TestAppContext) {
    let (harness, cx) = workspace_with_editor(cx);
    assert!(cx.read(|cx| harness.workspace.read(cx).bottom_dock().read(cx).is_open()));

    click_at(cx, leak(menu_title_selector("File")));
    click_at(cx, "Open recent");
    assert!(menu_is_open(cx), "the submenu replaces the menu in place");
    assert!(
        cx.debug_bounds("Open workspace…").is_none(),
        "the File entries made way for the submenu's"
    );
    click_at(cx, "Toggle console");

    assert!(
        !cx.read(|cx| harness.workspace.read(cx).bottom_dock().read(cx).is_open()),
        "the submenu entry ran"
    );
    assert!(!menu_is_open(cx));
}
