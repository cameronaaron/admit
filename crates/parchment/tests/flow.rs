//! Sign-in and the order read against two mock hosts (portal, Keycloak),
//! serving the pages captured on 2026-10-10. Each POST body is pinned to
//! the one the browser sent.
use std::sync::{Arc, Mutex};

use admit_parchment::{Client, Endpoints, Error, Saved};
use wiremock::matchers::{body_string, header_regex, method, path};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

fn fixture(name: &str) -> String {
    std::fs::read_to_string(format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"))).expect("fixture")
}

fn html(body: String) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_raw(body, "text/html")
}

struct Hosts {
    www: MockServer,
    auth: MockServer,
}

impl Hosts {
    async fn start() -> Hosts {
        Hosts { www: MockServer::start().await, auth: MockServer::start().await }
    }

    fn endpoints(&self) -> Endpoints {
        Endpoints { www: self.www.uri(), auth: self.auth.uri() }
    }

    /// A captured Keycloak page, its form posting to the mock.
    fn kc(&self, name: &str) -> ResponseTemplate {
        html(fixture(name).replace("https://auth.parchment.com", &self.auth.uri()))
    }

    /// next-auth's half: csrf, the start, the callback, and the orders
    /// page behind the session cookie.
    async fn portal(&self) {
        let www = &self.www;
        Mock::given(method("GET"))
            .and(path("/lp/api/auth/csrf"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("Set-Cookie", "next-auth.csrf-token=c; Path=/lp")
                    .set_body_json(serde_json::json!({"csrfToken": "T"})),
            )
            .mount(www)
            .await;
        Mock::given(method("POST"))
            .and(path("/lp/api/auth/signin/keycloak"))
            .and(body_string("callbackUrl=%2Flp%2Forders&csrfToken=T&json=true"))
            .and(header_regex("cookie", "next-auth.csrf-token=c"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "url": format!("{}/auth/realms/parchment/protocol/openid-connect/auth?client_id=learner-portal-ui", self.auth.uri())
            })))
            .mount(www)
            .await;
        Mock::given(method("GET"))
            .and(path("/lp/api/auth/callback/keycloak"))
            .respond_with(
                ResponseTemplate::new(302)
                    .insert_header("Set-Cookie", "__Secure-next-auth.session-token=S; Path=/lp; HttpOnly")
                    .insert_header("Location", "/lp/orders"),
            )
            .mount(www)
            .await;
        Mock::given(method("GET"))
            .and(path("/lp/orders"))
            .and(header_regex("cookie", "session-token=S"))
            .respond_with(html(fixture("orders.html")))
            .mount(www)
            .await;
        Mock::given(method("GET"))
            .and(path("/lp/orders"))
            .respond_with(ResponseTemplate::new(307).insert_header("Location", "/lp/signIn?callbackUrl=%2Flp%2Forders"))
            .mount(www)
            .await;
        Mock::given(method("GET"))
            .and(path("/lp/signIn"))
            .respond_with(html("<html>sign in</html>".into()))
            .mount(www)
            .await;
    }

    fn back_to_portal(&self) -> ResponseTemplate {
        ResponseTemplate::new(302)
            .insert_header("Set-Cookie", "KEYCLOAK_TRUSTED_DEVICE=dev; Path=/auth/realms/parchment/; Max-Age=86400")
            .insert_header("Location", format!("{}/lp/api/auth/callback/keycloak?code=x", self.www.uri()))
    }

    async fn keycloak(&self, password_answer: ResponseTemplate, code_answer: ResponseTemplate) {
        let auth = &self.auth;
        Mock::given(method("GET"))
            .and(path("/auth/realms/parchment/protocol/openid-connect/auth"))
            .respond_with(self.kc("kc_login.html"))
            .mount(auth)
            .await;
        let at = "/auth/realms/parchment/login-actions/authenticate";
        Mock::given(method("POST"))
            .and(path(at))
            .and(body_string("username=student%40school.example&password=p%26w&credentialId=&login="))
            .respond_with(password_answer)
            .mount(auth)
            .await;
        Mock::given(method("POST"))
            .and(path(at))
            .and(body_string("otp=123456&submit=Submit"))
            .respond_with(code_answer)
            .mount(auth)
            .await;
        Mock::given(method("POST"))
            .and(path(at))
            .and(body_string("trusted-device-name=Chrome+on+Mac+OS+X&trusted-device=yes"))
            .respond_with(self.back_to_portal())
            .mount(auth)
            .await;
    }
}

fn counting(answer: Option<&str>) -> (Arc<Mutex<u32>>, impl FnMut() -> Option<String>) {
    let asked = Arc::new(Mutex::new(0));
    let seen = asked.clone();
    let answer = answer.map(str::to_string);
    (asked, move || {
        *seen.lock().expect("lock") += 1;
        answer.clone()
    })
}

#[tokio::test]
async fn password_then_emailed_code_then_trust_signs_in_and_reads_orders() {
    tokio::task::unconstrained(async {
        let h = Hosts::start().await;
        h.portal().await;
        h.keycloak(h.kc("kc_otp.html"), h.kc("kc_trusted_device.html")).await;
        let c = Client::new(&h.endpoints(), &Saved::default()).expect("client");
        assert!(matches!(c.orders().await, Err(Error::Expired)));
        let (asked, mut code) = counting(Some(" 123456\n"));
        c.sign_in("student@school.example", "p&w", &mut code).await.expect("signed in");
        assert_eq!(*asked.lock().expect("lock"), 1);
        assert_eq!(c.orders().await.expect("orders").len(), 5);
        let saved = c.saved();
        assert!(
            saved.session.as_deref().is_some_and(|s| s.contains("__Secure-next-auth.session-token=S")),
            "{saved:?}"
        );
        assert_eq!(saved.device.as_deref(), Some("KEYCLOAK_TRUSTED_DEVICE=dev"));
        // The kept cookies are a signed-in client next time.
        let again = Client::new(&h.endpoints(), &saved).expect("client");
        assert_eq!(again.orders().await.expect("orders").len(), 5);
    })
    .await
}

#[tokio::test]
async fn a_live_keycloak_session_signs_in_without_asking() {
    tokio::task::unconstrained(async {
        let h = Hosts::start().await;
        h.portal().await;
        Mock::given(method("GET"))
            .and(path("/auth/realms/parchment/protocol/openid-connect/auth"))
            .respond_with(h.back_to_portal())
            .mount(&h.auth)
            .await;
        let c = Client::new(&h.endpoints(), &Saved::default()).expect("client");
        let (asked, mut code) = counting(None);
        c.sign_in("u", "p", &mut code).await.expect("signed in");
        assert_eq!(*asked.lock().expect("lock"), 0);
    })
    .await
}

#[tokio::test]
async fn a_trusted_device_skips_the_code() {
    tokio::task::unconstrained(async {
        let h = Hosts::start().await;
        h.portal().await;
        h.keycloak(h.kc("kc_trusted_device.html"), ResponseTemplate::new(500)).await;
        let c = Client::new(&h.endpoints(), &Saved::default()).expect("client");
        let (asked, mut code) = counting(None);
        c.sign_in("student@school.example", "p&w", &mut code).await.expect("signed in");
        assert_eq!(*asked.lock().expect("lock"), 0);
    })
    .await
}

#[tokio::test]
async fn refusals_say_which_step_and_why() {
    tokio::task::unconstrained(async {
        // The sign-in page again: its React form's message, else a generic
        // one. (A refused password was never captured; the message's place
        // is the form's data-input-error.)
        for (shown, want) in [
            ("Invalid username or password.", "Invalid username or password."),
            ("", "username or password not accepted"),
        ] {
            let h = Hosts::start().await;
            h.portal().await;
            let page = fixture("kc_login.html")
                .replace("https://auth.parchment.com", &h.auth.uri())
                .replace(r#"data-input-error="""#, &format!(r#"data-input-error="{shown}""#));
            h.keycloak(html(page), ResponseTemplate::new(500)).await;
            let c = Client::new(&h.endpoints(), &Saved::default()).expect("client");
            let err = c.sign_in("student@school.example", "p&w", &mut || None).await.expect_err("bad password");
            assert_eq!(err.to_string(), format!("Parchment sign-in rejected: {want}"));
        }

        let h = Hosts::start().await;
        h.portal().await;
        h.keycloak(h.kc("kc_otp.html"), h.kc("kc_otp.html")).await;
        let c = Client::new(&h.endpoints(), &Saved::default()).expect("client");
        assert!(matches!(c.sign_in("student@school.example", "p&w", &mut || None).await, Err(Error::CodeNeeded)));
        let err =
            c.sign_in("student@school.example", "p&w", &mut || Some("123456".into())).await.expect_err("bad code");
        assert!(matches!(&err, Error::Rejected(m) if m == "the code was not accepted"), "{err}");
        // A code page again with Keycloak's message: that message.
        let h = Hosts::start().await;
        h.portal().await;
        let refused = fixture("kc_otp.html").replace("https://auth.parchment.com", &h.auth.uri())
            + r#"<span id="input-error-otp-code" aria-live="polite">Invalid authenticator code.</span>"#;
        h.keycloak(h.kc("kc_otp.html"), html(refused)).await;
        let c = Client::new(&h.endpoints(), &Saved::default()).expect("client");
        let err =
            c.sign_in("student@school.example", "p&w", &mut || Some("123456".into())).await.expect_err("bad code");
        assert_eq!(err.to_string(), "Parchment sign-in rejected: Invalid authenticator code.");

        let h = Hosts::start().await;
        h.portal().await;
        h.keycloak(
            html("<div class=\"pf-c-alert__title\">Account is disabled</div>".into()),
            ResponseTemplate::new(500),
        )
        .await;
        let c = Client::new(&h.endpoints(), &Saved::default()).expect("client");
        let err = c.sign_in("student@school.example", "p&w", &mut || None).await.expect_err("disabled");
        assert_eq!(err.to_string(), "Parchment sign-in rejected: Account is disabled");

        let h = Hosts::start().await;
        h.portal().await;
        h.keycloak(html("<p>new page</p>".into()), ResponseTemplate::new(500)).await;
        let c = Client::new(&h.endpoints(), &Saved::default()).expect("client");
        let err = c.sign_in("student@school.example", "p&w", &mut || None).await.expect_err("unknown");
        assert!(err.to_string().contains("no sign-in, code or trust form"), "{err}");
    })
    .await
}

/// Trust forever: the loop gives up instead of posting without end.
struct Trust(String);

impl Respond for Trust {
    fn respond(&self, _: &Request) -> ResponseTemplate {
        html(fixture("kc_trusted_device.html").replace("https://auth.parchment.com", &self.0))
    }
}

#[tokio::test]
async fn sign_in_gives_up_after_four_keycloak_pages() {
    tokio::task::unconstrained(async {
        let h = Hosts::start().await;
        h.portal().await;
        Mock::given(wiremock::matchers::any()).respond_with(Trust(h.auth.uri())).expect(5).mount(&h.auth).await;
        let c = Client::new(&h.endpoints(), &Saved::default()).expect("client");
        let err = c.sign_in("u", "p", &mut || None).await.expect_err("loop");
        assert!(err.to_string().contains("did not finish after four pages"), "{err}");
    })
    .await
}

#[tokio::test]
async fn broken_portal_answers_are_named() {
    tokio::task::unconstrained(async {
        for (csrf, start, want) in [
            (ResponseTemplate::new(200).set_body_string("<html>"), None, "csrf: not JSON"),
            (ResponseTemplate::new(200).set_body_json(serde_json::json!({})), None, "no csrfToken"),
            (
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"csrfToken": "T"})),
                Some(ResponseTemplate::new(200).set_body_string("x")),
                "sign-in start: not JSON",
            ),
            (
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"csrfToken": "T"})),
                Some(ResponseTemplate::new(200).set_body_json(serde_json::json!({}))),
                "sign-in start: no url",
            ),
            (
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"csrfToken": "T"})),
                Some(ResponseTemplate::new(200).set_body_json(serde_json::json!({"url": "::"}))),
                "Keycloak: bad URL",
            ),
            (ResponseTemplate::new(502), None, "csrf: HTTP 502"),
        ] {
            let www = MockServer::start().await;
            Mock::given(method("GET")).and(path("/lp/api/auth/csrf")).respond_with(csrf).mount(&www).await;
            if let Some(s) = start {
                Mock::given(method("POST")).respond_with(s).mount(&www).await;
            }
            let c = Client::new(&Endpoints { www: www.uri(), auth: www.uri() }, &Saved::default()).expect("client");
            let err = c.sign_in("u", "p", &mut || None).await.expect_err(want);
            assert!(err.to_string().contains(want), "{want}: {err}");
        }
        // A sign-in start that fails, a Keycloak that fails on the first
        // page or on a post.
        let h = Hosts::start().await;
        Mock::given(method("GET"))
            .and(path("/lp/api/auth/csrf"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"csrfToken": "T"})))
            .mount(&h.www)
            .await;
        Mock::given(method("POST")).respond_with(ResponseTemplate::new(500)).up_to_n_times(1).mount(&h.www).await;
        let c = Client::new(&h.endpoints(), &Saved::default()).expect("client");
        assert!(matches!(
            c.sign_in("u", "p", &mut || None).await,
            Err(Error::Http { step: "sign-in start", status: 500 })
        ));
        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"url": format!("{}/kc", h.auth.uri())})),
            )
            .mount(&h.www)
            .await;
        Mock::given(method("GET")).respond_with(ResponseTemplate::new(503)).up_to_n_times(1).mount(&h.auth).await;
        assert!(matches!(c.sign_in("u", "p", &mut || None).await, Err(Error::Http { step: "Keycloak", status: 503 })));
        Mock::given(method("GET")).respond_with(h.kc("kc_login.html")).mount(&h.auth).await;
        Mock::given(method("POST")).respond_with(ResponseTemplate::new(502)).mount(&h.auth).await;
        assert!(matches!(c.sign_in("u", "p", &mut || None).await, Err(Error::Http { step: "Keycloak", status: 502 })));

        let www = MockServer::start().await;
        Mock::given(method("GET")).respond_with(ResponseTemplate::new(500)).mount(&www).await;
        let c = Client::new(&Endpoints { www: www.uri(), auth: www.uri() }, &Saved::default()).expect("client");
        assert!(matches!(c.orders().await, Err(Error::Http { step: "orders", status: 500 })));
        let c = Client::new(&Endpoints { www: "http://127.0.0.1:9".into(), auth: www.uri() }, &Saved::default())
            .expect("client");
        assert!(matches!(c.orders().await, Err(Error::Transport { step: "orders", .. })));
    })
    .await
}

#[tokio::test]
async fn an_unresolvable_form_action_is_named() {
    tokio::task::unconstrained(async {
        let h = Hosts::start().await;
        h.portal().await;
        Mock::given(method("GET"))
            .respond_with(html(r#"<form action="http://[bad" method="post"><input name="otp"></form>"#.into()))
            .mount(&h.auth)
            .await;
        let c = Client::new(&h.endpoints(), &Saved::default()).expect("client");
        let err = c.sign_in("u", "p", &mut || Some("1".into())).await.expect_err("bad action");
        assert!(err.to_string().contains("Keycloak: bad URL"), "{err}");
    })
    .await
}

/// A page whose body stops short of its Content-Length is a transport
/// error, not a shorter page.
#[tokio::test]
async fn a_truncated_page_is_a_transport_error() {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let www = format!("http://{}", listener.local_addr().expect("addr"));
    let server = std::thread::spawn(move || {
        let (mut sock, _) = listener.accept().expect("accept");
        let _ = sock.read(&mut [0; 4096]);
        let _ = sock.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\nshort");
    });
    let c = Client::new(&Endpoints { www, auth: "http://127.0.0.1:9".into() }, &Saved::default()).expect("client");
    let err = c.orders().await.expect_err("truncated");
    assert!(matches!(err, Error::Transport { step: "orders", .. }), "{err}");
    server.join().expect("server");
}
