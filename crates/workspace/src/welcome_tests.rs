//! The Welcome empty state at the design system's empty-state scale (#234): an icon, a
//! title, readable supporting copy and the call to action — not one small muted line.
//! The bounds assertions are the "readable size" proof: the title and the copy must
//! paint at least at the design system's empty-state text sizes
//! (`docs/postman_redesign_pack/01_specs/design_system.md`, S02/R02).

use gpui::{TestAppContext, px};
use ui::test_support::{draw_window, init_test};

use super::Welcome;

#[gpui::test]
fn welcome_empty_state_is_an_icon_a_title_and_a_call_to_action(cx: &mut TestAppContext) {
    cx.update(init_test);
    let (_welcome, cx) = cx.add_window_view(|_window, cx| Welcome::new(cx));
    draw_window(cx);

    assert!(
        cx.debug_bounds("welcome-icon").is_some(),
        "the empty state leads with an icon (design_system.md, Feedback › EmptyState)"
    );
    let title = cx
        .debug_bounds("welcome-title")
        .expect("the empty-state title renders");
    assert!(
        title.size.height >= px(24.),
        "the title reads at the empty-state scale (16 px text and up paints a ≥24 px line \
         box; 12 px captions paint 19.5 px), got {title:?}"
    );
    let description = cx
        .debug_bounds("welcome-description")
        .expect("the empty-state copy renders");
    assert!(
        description.size.height >= px(24.),
        "the copy reads at the empty-state scale (16 px text paints a 26 px line box, \
         14 px only 22.75 px), got {description:?}"
    );
    assert!(
        cx.debug_bounds("welcome-new-request").is_some(),
        "the call to action renders with the empty state"
    );
}
