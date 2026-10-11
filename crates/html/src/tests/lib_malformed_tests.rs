use super::*;
#[test]
fn truncated_markup_and_unknown_entities_preserve_what_is_readable() {
    assert_eq!(tag_end("<input value='unterminated", 0), None);
    assert!(forms("<form action='unterminated").is_empty());
    assert_eq!(forms("<form><input value='x'><input name=last")[0].fields, vec![("last".into(), "".into())]);
    assert_eq!(attr("<input name=", "name"), None);
    assert_eq!(attr("<input name=unquoted>", "name").as_deref(), Some("unquoted"));
    assert_eq!(
        unescape("&lt;&gt;&quot;&apos;&#X41; &unknown; &#x110000; &unterminated"),
        "<>\"'A &unknown; &#x110000; &unterminated"
    );
    assert_eq!(form_with("<form><input name=x value=y></form>", "x").expect("form").action, "");
    assert!(form_with("<form></form>", "missing").is_none());
}

#[test]
fn a_stray_equals_sign_and_its_value_are_skipped() {
    let t = r#"<input = "x" name=real =y checked>"#;
    assert_eq!(attr(t, "name").as_deref(), Some("real"));
    assert!(has_attr(t, "checked"));
    assert_eq!(attributes(t), vec![("name", Some("real")), ("checked", None)]);
}
