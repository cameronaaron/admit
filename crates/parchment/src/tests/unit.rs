use super::*;

fn fixture(name: &str) -> String {
    std::fs::read_to_string(format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"))).expect("fixture")
}

fn page(lines: &[serde_json::Value]) -> String {
    lines.iter().map(|l| format!("<script>self.__next_f.push({l})</script>")).collect()
}

#[test]
fn flight_data_joins_the_string_pushes_only() {
    let html = page(&[
        serde_json::json!([0]),
        serde_json::json!([1, "a:{\"x\""]),
        serde_json::json!([2, null]),
        serde_json::json!([1, ":1}\n"]),
        serde_json::json!(["1", "no"]),
    ]) + "<script>self.__next_f.push(not json)</script>";
    assert_eq!(flight(&html), "a:{\"x\":1}\n");
    assert_eq!(flight("no scripts"), "");
}

#[test]
fn the_captured_orders_page_reads_as_five_sent_transcripts() {
    let o = orders(&fixture("orders.html")).expect("orders");
    let got: Vec<(&str, &str, &str, Stage)> =
        o.iter().map(|o| (o.id.as_str(), o.from.as_str(), o.ordered.as_str(), o.stage)).collect();
    assert_eq!(
        got,
        vec![
            ("TR52R8UJ", "Illinois Institute of Technology", "2026-07-24", Stage::Sent),
            ("TR5G5BBF", "Los Angeles City College", "2026-07-24", Stage::Sent),
            ("TR5GUNYO", "Trinity Christian College", "2026-07-23", Stage::Sent),
            ("TR58V8CI", "California State University - Northridge", "2026-07-23", Stage::Sent),
            ("TR58V5RJ", "University of Massachusetts Amherst", "2026-07-23", Stage::Sent),
        ]
    );
    assert!(o.iter().all(|o| o.via == "parchment"
        && o.to == "NursingCAS - American Association of Colleges of Nursing"
        && o.status == "COMPLETE"
        && o.changed.is_none()));
}

fn one(item: serde_json::Value) -> Order {
    let list = serde_json::json!({"orders": [{"createDate": "2026-08-01T00:00:00Z", "orderLineItems": [item]}]});
    let html = page(&[serde_json::json!([1, format!("2:[\"$\",\"$Lb\",null,{list}]\n")])]);
    orders(&html).expect("orders").remove(0)
}

#[test]
fn a_hold_or_a_cancellation_is_held_and_any_other_status_open() {
    let base = serde_json::json!({"deliveryId": "TR1", "orderLineItemStatus": "COMPLETE", "holdType": "NONE", "cancelType": null});
    assert_eq!(one(base.clone()).stage, Stage::Sent);
    let mut held = base.clone();
    held["holdType"] = "TERM_END".into();
    let o = one(held);
    assert_eq!((o.stage, o.status.as_str()), (Stage::Held, "COMPLETE (hold TERM_END, cancel null)"));
    let mut cancelled = base.clone();
    cancelled["cancelType"] = "STUDENT".into();
    assert_eq!(one(cancelled).stage, Stage::Held);
    let mut open = base;
    open["orderLineItemStatus"] = "PROCESSING".into();
    open.as_object_mut().expect("object").remove("holdType");
    assert_eq!(one(open).stage, Stage::Open);
}

#[test]
fn a_page_without_a_readable_order_list_is_an_error() {
    for (html, what) in [
        (page(&[serde_json::json!([1, "nothing here"])]), "no order list"),
        (page(&[serde_json::json!([1, "{\"orders\":[{\"uuid\""])]), "not JSON"),
        (page(&[serde_json::json!([1, "{\"orders\":{}}"])]), "not a list"),
    ] {
        let err = orders(&html).expect_err(what);
        assert!(err.to_string().contains(what), "{err}");
    }
}

#[test]
fn keycloak_feedback_is_the_first_message_it_shows() {
    let html = r#"<span id="input-error" class="x" aria-live="polite">Invalid username or password.</span>"#;
    assert_eq!(feedback(html).as_deref(), Some("Invalid username or password."));
    let html = r#"<span id="input-error-otp-code"></span><div class="pf-c-alert__title kc-feedback-text">Invalid authenticator code &amp; more.</div>"#;
    assert_eq!(feedback(html).as_deref(), Some("Invalid authenticator code & more."));
    assert_eq!(feedback("<p>fine</p>"), None);
    assert_eq!(feedback("id=\"input-error"), None);
    // A marker that is not in a tag, or in one never closed, is skipped.
    assert_eq!(feedback("id=\"input-error <p class=\"kc-feedback-text\">Try again</p>").as_deref(), Some("Try again"));
    assert_eq!(feedback("<span id=\"input-error"), None);
}

#[test]
fn script_text_is_never_feedback() {
    // The live sign-in page's commonProps script names the theme's classes,
    // "pf-c-alert__title kc-feedback-text" among them: no message.
    assert_eq!(feedback(&fixture("kc_login.html")), None);
    let html = r#"<script>var p = {"kcAlertTitleClass": "pf-c-alert__title kc-feedback-text"};</script>
        <SCRIPT type="module">x("kc-feedback-text")</SCRIPT><span id="input-error">Invalid username or password.</span>"#;
    assert_eq!(feedback(html).as_deref(), Some("Invalid username or password."));
    assert_eq!(feedback(r#"<p>x</p><script>"pf-c-alert__title">not closed"#), None);
}

#[test]
fn the_live_sign_in_page_is_a_react_login_form() {
    let page = fixture("kc_login.html");
    // The execution id is the one the browser's password post went to.
    let action = "https://auth.parchment.com/auth/realms/parchment/login-actions/authenticate?session_code=SC&execution=fdb6f5d0-a8ca-4fe3-9ce5-5318f1d61f02&client_id=learner-portal-ui&tab_id=TAB&client_data=CD";
    assert_eq!(login_form(&page), Some(LoginForm { action: action.into(), error: String::new() }));
    let shown = page.replace(r#"data-input-error="""#, r#"data-input-error=" Invalid username or password. ""#);
    assert_eq!(login_form(&shown).expect("form").error, "Invalid username or password.");
    // Missing either half, or malformed, it is not the sign-in page.
    for html in [
        r#"<div data-react-component="LoginForm"></div>"#,
        r#"{"loginAction": "https://x"}"#,
        r#"<div data-react-component="LoginForm"></div><script>{"loginAction": 1}</script>"#,
        r#"data-react-component="LoginForm" {"loginAction": "https://x"}"#,
        r#"<div data-react-component="LoginForm" {"loginAction": "https://x"}"#,
        r#"<div data-react-component="LoginForm"> "loginAction": "#,
    ] {
        assert_eq!(login_form(html), None, "{html}");
    }
}

#[test]
fn saved_cookies_round_trip_to_their_hosts() {
    let ep = Endpoints { www: "https://w.example/".into(), auth: "https://a.example".into() };
    let c = Client::new(&ep, &Saved { session: Some("s=1; t=2;".into()), device: Some("d=3".into()) }).expect("client");
    // The jar's order is its own; a Cookie header's order carries nothing.
    let saved = c.saved();
    let mut session: Vec<&str> = saved.session.as_deref().expect("session").split("; ").collect();
    session.sort_unstable();
    assert_eq!((session, saved.device.as_deref()), (vec!["s=1", "t=2"], Some("d=3")));
    let empty = Client::new(&ep, &Saved::default()).expect("client");
    assert_eq!(empty.saved(), Saved::default());
    assert!(Client::new(&Endpoints { www: "not a url".into(), auth: "x".into() }, &Saved::default()).is_err());
    assert!(Client::new(&Endpoints { www: "https://w.example".into(), auth: "x".into() }, &Saved::default()).is_err());
    assert_eq!(Endpoints::default().www, "https://www.parchment.com");
}
