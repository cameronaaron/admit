use super::*;
#[test]
fn scalar_values_do_not_assume_string_typed_json() {
    assert_eq!(Client::api_prefix("https://example.test/"), "https://example.test/applicant/api/");
    assert_eq!(Client::api_prefix("https://example.test"), "https://example.test/applicant/api/");
    assert_eq!(s(&serde_json::json!(42)), "42");
    assert_eq!(int(&serde_json::json!("42")), 42);
    assert_eq!(int(&serde_json::json!("invalid")), 0);
    assert_eq!(opt(&serde_json::json!("None")), None);
    let mut headers = reqwest::header::HeaderMap::new();
    headers.append(reqwest::header::SET_COOKIE, reqwest::header::HeaderValue::from_bytes(b"x=\xff").expect("header"));
    let mut jar = vec![("old".into(), "cookie".into())];
    absorb_cookies(&mut jar, &headers);
    assert_eq!(jar, vec![("old".into(), "cookie".into())]);
}
