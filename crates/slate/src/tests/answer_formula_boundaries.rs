use super::*;
#[test]
fn arithmetic_handles_unicode_whitespace_and_rejects_invalid_tokens() {
    let p = crate::page::page("frm?test", "");
    for (f, want) in [
        ("1 +\u{2003} 2", "3"),
        ("-2 * (3 + 4)", "-14"),
        ("3/2", "1.5"),
        ("1 - 2", "-1"),
        ("1000000000000000", "1000000000000000"),
    ] {
        assert_eq!(formula(&p, &[], f).expect("arithmetic"), want);
    }
    for f in ["1 a", "1 2", "1..2", "+1", "(1+2", "", "@missing + 1", "1 +", "1 *", "(+)", "()", "@ghost_text"] {
        assert!(formula(&p, &[], f).is_err(), "accepted {f}");
    }
    let bare = crate::page::page(
        "frm?t",
        "<h1>T</h1><div class=\"form_question\" data-id=\"q\" data-type=\"select\" data-export=\"sys:color\"></div>",
    );
    assert!(formula(&bare, &[], "@sys:color_text").is_err(), "a choice text with no control");
}
