//! Section tabs carry a count (Headers 8) and a "set" dot (Auth ●): the count paints as
//! text beside the label, the dot never carries its meaning by colour alone — the tab's
//! accessible label spells both out.

use gpui::TestAppContext;

use crate::components::tab::Tab;
use crate::components::tab_bar::TabBar;
use crate::test_support::{draw_window, init_test};

#[gpui::test]
fn test_a_tab_with_a_count_paints_the_count_beside_its_label(cx: &mut TestAppContext) {
    cx.update(init_test);
    let tabs = vec![
        Tab::new("params", "Params"),
        Tab::new("headers", "Headers").count(Some(3)),
    ];
    let (_bar, cx) = cx.add_window_view(|_, cx| TabBar::new(cx).section().with_tabs(tabs));
    draw_window(cx);

    let tab = cx.debug_bounds("headers").expect("the tab rendered");
    let count = cx
        .debug_bounds("headers-count")
        .expect("a counted tab paints its count");
    assert!(
        tab.contains(&count.center()),
        "the count sits inside its tab"
    );
    assert!(
        cx.debug_bounds("params-count").is_none(),
        "a tab without a count paints none"
    );
}

#[gpui::test]
fn test_a_tab_with_an_indicator_paints_a_dot_and_names_it(cx: &mut TestAppContext) {
    cx.update(init_test);
    let tab = Tab::new("auth", "Auth").indicator(Some("set".into()));
    assert_eq!(
        tab.accessible_label().as_ref(),
        "Auth, set",
        "the dot's meaning is in the accessible label, not only its colour"
    );
    let counted = Tab::new("headers", "Headers").count(Some(2));
    assert_eq!(counted.accessible_label().as_ref(), "Headers, 2");
    let (_bar, cx) = cx.add_window_view(|_, cx| TabBar::new(cx).section().with_tabs(vec![tab]));
    draw_window(cx);

    assert!(
        cx.debug_bounds("auth-indicator").is_some(),
        "the indicator dot is painted"
    );
}
