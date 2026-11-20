use super::*;

#[test]
fn test_window_icon_is_the_bundled_square_artwork_with_a_transparent_margin() {
    let icon = window_icon().expect("the bundled icon decodes");

    assert_eq!((icon.width(), icon.height()), (256, 256));
    // The pack's tile sits inside a transparent outer margin; the centre is opaque artwork.
    assert_eq!(icon.get_pixel(0, 0).0[3], 0);
    assert_eq!(icon.get_pixel(128, 128).0[3], 255);
}

#[test]
fn test_app_id_matches_the_desktop_entry_the_install_script_writes() {
    let script = include_str!("../../../scripts/install-desktop-entry.sh");

    assert!(script.contains(&format!("APP_ID={APP_ID}\n")));
}
