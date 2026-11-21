//! Section tabs (Params…Settings, Body/Headers/Cookies/Timeline, Pretty/Raw/Hex): the
//! blueprint draws them with room around each label and a 2 px accent underline under
//! the active one, not as boxes on a grey band (#216). Pane tabs keep the default look.

use gpui::{TestAppContext, px};

use crate::components::tab::Tab;
use crate::components::tab_bar::TabBar;
use crate::test_support::{draw_window, init_test};

fn tabs() -> Vec<Tab> {
    vec![Tab::new("params", "Params"), Tab::new("headers", "Headers")]
}

#[gpui::test]
fn test_the_active_section_tab_is_underlined_along_its_bottom_edge(cx: &mut TestAppContext) {
    cx.update(init_test);
    let (_bar, cx) = cx.add_window_view(|_, cx| TabBar::new(cx).section().with_tabs(tabs()));
    draw_window(cx);

    let tab = cx.debug_bounds("params").expect("the active tab rendered");
    let underline = cx
        .debug_bounds("params-underline")
        .expect("the active section tab is underlined");
    assert_eq!(underline.size.height, px(2.), "the underline is 2 px");
    assert_eq!(
        underline.bottom(),
        tab.bottom(),
        "it sits on the tab's bottom edge"
    );
    assert_eq!(
        underline.size.width, tab.size.width,
        "it spans the whole tab"
    );
    assert!(
        cx.debug_bounds("headers-underline").is_none(),
        "an inactive section tab has no underline"
    );
}

#[gpui::test]
fn test_section_tabs_give_their_labels_more_room_than_pane_tabs(cx: &mut TestAppContext) {
    cx.update(init_test);
    let (_bar, cx) = cx.add_window_view(|_, cx| TabBar::new(cx).with_tabs(tabs()));
    draw_window(cx);
    let pane_tab = cx.debug_bounds("headers").expect("pane tab rendered");
    assert!(
        cx.debug_bounds("params-underline").is_none(),
        "pane tabs keep the filled active state, no underline"
    );

    let (_bar, cx) = cx.add_window_view(|_, cx| TabBar::new(cx).section().with_tabs(tabs()));
    draw_window(cx);
    let section_tab = cx.debug_bounds("headers").expect("section tab rendered");
    assert!(
        section_tab.size.width > pane_tab.size.width,
        "section tab {:?} is wider than pane tab {:?}",
        section_tab.size.width,
        pane_tab.size.width
    );
}
