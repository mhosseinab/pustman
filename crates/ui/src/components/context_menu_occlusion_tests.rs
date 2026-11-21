//! An open menu owns the pointer (#205): a control drawn underneath it must not show its
//! hover tooltip through the menu, or the balloon covers the entries being read.

use std::time::Duration;

use gpui::{
    AppContext, Context, Entity, InteractiveElement, IntoElement, ParentElement, Render, Styled,
    TestAppContext, Window, div,
};

use crate::components::context_menu::ContextMenu;
use crate::components::tab::Tab;
use crate::components::tab_bar::TabBar;
use crate::components::tooltip::TOOLTIP_SELECTOR;
use crate::test_support::{draw_window, init_test};

/// Longer than GPUI's tooltip show delay (500 ms).
const PAST_TOOLTIP_DELAY: Duration = Duration::from_millis(600);

const MENU_SELECTOR: &str = "open-menu";

/// A tab with a tooltip and an open menu drawn over its top-left corner.
struct MenuOverTab {
    bar: Entity<TabBar>,
    menu: Entity<ContextMenu>,
}

impl Render for MenuOverTab {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().relative().size_full().child(self.bar.clone()).child(
            div()
                .debug_selector(|| MENU_SELECTOR.into())
                .absolute()
                .top_0()
                .left_0()
                .child(self.menu.clone()),
        )
    }
}

#[gpui::test]
fn test_an_open_menu_hides_the_tooltips_of_controls_under_it(cx: &mut TestAppContext) {
    cx.update(init_test);
    let (_view, cx) = cx.add_window_view(|_, cx| MenuOverTab {
        bar: cx.new(|cx| TabBar::new(cx).with_tabs(vec![Tab::new("draft", "Draft").dirty(true)])),
        menu: cx.new(|cx| {
            ContextMenu::new(cx)
                .entry("Open", |_, _, _| {})
                .entry("Duplicate", |_, _, _| {})
                .entry("Delete", |_, _, _| {})
        }),
    });
    draw_window(cx);
    let tab = cx.debug_bounds("draft").expect("the tab rendered");
    let menu = cx.debug_bounds(MENU_SELECTOR).expect("the menu rendered");
    assert!(
        menu.contains(&tab.center()),
        "the menu ({menu:?}) covers the tab's centre ({tab:?})"
    );

    cx.simulate_mouse_move(tab.center(), None, gpui::Modifiers::none());
    draw_window(cx);
    cx.executor().advance_clock(PAST_TOOLTIP_DELAY);
    cx.run_until_parked();
    draw_window(cx);
    assert!(
        cx.debug_bounds(TOOLTIP_SELECTOR).is_none(),
        "the tab under the open menu showed its tooltip through the menu"
    );
}
