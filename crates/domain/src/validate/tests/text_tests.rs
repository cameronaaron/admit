use super::slip;

#[test]
fn slips_are_found_and_normal_text_is_not() {
    assert_eq!(
        slip("ensure public and property security.. Credential ID: 6859700").map(|s| s.0),
        Some("doubled period")
    );
    assert_eq!(slip("a,, b").map(|s| s.0), Some("doubled comma"));
    assert_eq!(slip("first , second").map(|s| s.0), Some("space before punctuation"));
    assert_eq!(slip("reviewing the the questions").map(|s| s.0), Some("repeated word"));
    for ok in [
        "Wait... then go.",
        "GPA 3.74 (A.A.).",
        "•Created tools; reviewed 2,000 items",
        "B. B. King",
        "Spring 2026: 4.0",
    ] {
        assert_eq!(slip(ok), None, "{ok}");
    }
}
