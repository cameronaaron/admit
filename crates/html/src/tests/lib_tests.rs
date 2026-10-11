use super::*;

#[test]
fn attr_matches_whole_names_only() {
    let t = r#"<input data-name="x" name='real' value="a&amp;b">"#;
    assert_eq!(attr(t, "name").as_deref(), Some("real"));
    assert_eq!(attr(t, "value").as_deref(), Some("a&b"));
    assert_eq!(attr(t, "missing"), None);
}

#[test]
fn quoted_values_are_never_read_as_attributes() {
    let t = r#"<input title="a value=wrong b" data-note='was checked' value="right">"#;
    assert_eq!(attr(t, "value").as_deref(), Some("right"));
    assert!(!has_attr(t, "checked"), "a word inside a quoted value is not an attribute");
    assert!(has_attr(t, "TITLE"));
}

#[test]
fn boolean_attributes_have_no_value() {
    for t in
        ["<input type=radio value=1 checked>", "<input checked/>", "<input CHECKED value=x>", "<input checked=checked>"]
    {
        assert!(has_attr(t, "checked"), "{t}");
    }
    assert_eq!(attr("<input checked value=x>", "checked"), None, "bare: present but no value");
    assert!(!has_attr("<input type=radio value=1>", "checked"));
}

#[test]
fn unescape_handles_numeric_and_stray_ampersands() {
    assert_eq!(unescape("a&#39;b&#x2F;c & d&amp;e"), "a'b/c & d&e");
}

#[test]
fn forms_keep_their_own_inputs() {
    let html =
        "<form id=a action='/x'><input name=p value='1>2'></form><form action=\"/y?a=1&amp;b=2\"><input name=q></form>";
    let f = forms(html);
    assert_eq!(f.len(), 2);
    assert_eq!(f[0].fields, vec![("p".to_string(), "1>2".to_string())], "'>' inside a quoted value");
    assert_eq!(f[1].action, "/y?a=1&b=2");
    assert_eq!(f[1].fields, vec![("q".to_string(), String::new())]);
}
