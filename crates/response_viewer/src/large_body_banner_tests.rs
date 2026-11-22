//! The large-body banner's wording (#182): the notice stays on screen after the user
//! moves the mode bar — the design keeps the reveal button on it — so the message may
//! name the limit the body went over but no view mode. A mode in the wording goes stale
//! the moment the user picks Pretty or opens Hex/Preview (`large_body`'s module docs).

use super::LargeBodyNotice;

#[test]
fn test_the_large_body_banner_names_the_limit_and_no_view_mode() {
    let notice = LargeBodyNotice::new(1, None);
    let message = notice.message();

    assert!(
        message.contains("1 MB"),
        "the banner names the limit the body went over: {message}"
    );
    for mode in ["Raw", "Pretty", "Hex", "Preview"] {
        assert!(
            !message.contains(mode),
            "the banner names no view mode — it stays up while the user switches: {message}"
        );
    }
}
