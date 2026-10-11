use super::*;

#[test]
fn absorb_replaces_same_named_cookies() {
    let mut h = reqwest::header::HeaderMap::new();
    h.append("set-cookie", ".application=new; path=/; secure; HttpOnly".parse().expect("header"));
    h.append("set-cookie", "_uid=u; path=/".parse().expect("header"));
    let mut jar = "_node=n; .application=old".to_string();
    absorb(&mut jar, &h);
    assert_eq!(jar, "_node=n; .application=new; _uid=u");
    assert_eq!(cookie_value(&jar, ".application"), Some("new"));
    assert!(!has(&jar, "_hash"));
    h.append("set-cookie", "nosep; path=/".parse().expect("header"));
    absorb(&mut jar, &h);
    assert_eq!(jar, "_node=n; .application=new; _uid=u");
}
