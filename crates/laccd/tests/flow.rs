//! The sign-in relay and transcript request, against pages captured from the
//! live portal on 2026-10-06 (credentials and tokens redacted) and against a
//! mock portal that enforces what the real one does: cookies carried across
//! hosts, `ICStateNum`/`ICSID` echoed, the PDF behind the window.open link.

use admit_laccd::{Credentials, Endpoints, FetchError, Portal, fetch_unofficial_transcript, html};
use wiremock::matchers::{body_string_contains, header_regex, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const SIGNIN_FAILED: &str = include_str!("fixtures/adfs_signin_failed.html");
const SAML_RELAY: &str = include_str!("fixtures/adfs_saml_relay.html");
const PRINCIPAL: &str = include_str!("fixtures/ps_principal_request.html");
const TRANSCRIPT_PAGE: &str = include_str!("fixtures/ps_transcript_page.xml");
const REPORT_REQUESTED: &str = include_str!("fixtures/ps_report_requested.xml");

#[test]
fn captured_sign_in_page_yields_the_form_and_the_error() {
    let f = html::form_with(SIGNIN_FAILED, "UserName").expect("login form");
    assert!(f.action.starts_with("/adfs/ls/idpinitiatedsignon.aspx?loginToRp=csprd.laccd.edu"), "{}", f.action);
    assert!(f.has("Password"));
    assert_eq!(
        html::login_error(SIGNIN_FAILED).as_deref(),
        Some("Incorrect user ID or password. Type the correct user ID and password, and try again.")
    );
    assert_eq!(html::login_error(SAML_RELAY), None);
}

#[test]
fn captured_relay_forms_are_found_by_their_fields() {
    let saml = html::form_with(SAML_RELAY, "SAMLResponse").expect("SAML form");
    assert_eq!(saml.action, "https://csprd.laccd.edu:443/saml2/sp/acs/post");
    let start = html::form_with(PRINCIPAL, "sso_ticket").expect("PeopleSoft start form");
    assert_eq!(start.action, "/psp/csprd/?cmd=start");
    let names: Vec<&str> = start.fields.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, ["RemoteUser", "RelayState", "session", "sso_ticket", "sso_uuid", "client_ip"]);
}

#[test]
fn captured_peoplesoft_pages_yield_state_and_report_link() {
    let st = html::ic_state(TRANSCRIPT_PAGE).expect("state");
    assert_eq!(st.state_num, 5);
    assert_eq!(st.sid, "<redacted>");
    assert_eq!(
        html::report_url(REPORT_REQUESTED).as_deref(),
        Some("https://csprd.laccd.edu/psc/csprd_5/view/%7bV2%7dREDACTED/LAC_SR_TSRPT.pdf")
    );
}

const COMPONENT: &str = "/psc/csprd_5/EMPLOYEE/SA/c/SSR_STUDENT_ACAD_REC_FL.SSR_VW_UNOFF_TS_FL.GBL";

async fn mock_portal() -> MockServer {
    let s = MockServer::start().await;
    let uri = s.uri();
    let signin = "/adfs/ls/idpinitiatedsignon.aspx";
    let login_form = format!(
        "<form method=\"post\" id=\"loginForm\" action=\"{signin}?loginToRp=csprd.laccd.edu&amp;client-request-id=1\">\
         <input name=\"UserName\" value=\"\"><input name=\"Password\" type=\"password\"></form>"
    );
    Mock::given(method("GET"))
        .and(path(signin))
        .and(header_regex("cookie", "MSISAuth=ok"))
        .respond_with(ResponseTemplate::new(200).set_body_string(format!(
            "<form method=\"POST\" name=\"hiddenform\" action=\"{uri}/saml2/sp/acs/post\">\
             <input type=\"hidden\" name=\"SAMLResponse\" value=\"PHNhbWw+\" /></form>"
        )))
        .with_priority(1)
        .mount(&s)
        .await;
    Mock::given(method("GET"))
        .and(path(signin))
        .respond_with(ResponseTemplate::new(200).set_body_string(login_form.clone()))
        .mount(&s)
        .await;
    Mock::given(method("POST"))
        .and(path(signin))
        .and(body_string_contains("Password=right+pass"))
        .and(body_string_contains("AuthMethod=FormsAuthentication"))
        .and(body_string_contains("UserName=escsso%5C123"))
        .respond_with(
            ResponseTemplate::new(302)
                .insert_header("Location", format!("{signin}?loginToRp=csprd.laccd.edu"))
                .insert_header("Set-Cookie", "MSISAuth=ok; path=/"),
        )
        .with_priority(1)
        .mount(&s)
        .await;
    Mock::given(method("POST"))
        .and(path(signin))
        .respond_with(ResponseTemplate::new(200).set_body_string(format!(
            "{login_form}<span id=\"errorText\" role=\"alert\">Incorrect user ID or password.</span>"
        )))
        .mount(&s)
        .await;
    Mock::given(method("POST"))
        .and(path("/saml2/sp/acs/post"))
        .and(body_string_contains("SAMLResponse=PHNhbWw%2B"))
        .respond_with(
            ResponseTemplate::new(302)
                .insert_header("Location", "/psoftsso/PrincipalRequest")
                .insert_header("Set-Cookie", "JSESSIONID=j1; path=/"),
        )
        .mount(&s)
        .await;
    Mock::given(method("GET"))
        .and(path("/psoftsso/PrincipalRequest"))
        .and(header_regex("cookie", "JSESSIONID=j1"))
        .respond_with(ResponseTemplate::new(200).set_body_string(PRINCIPAL))
        .mount(&s)
        .await;
    Mock::given(method("POST"))
        .and(path("/psp/csprd/"))
        .and(body_string_contains("sso_ticket=%3Credacted%3E"))
        .respond_with(
            ResponseTemplate::new(200).insert_header("Set-Cookie", "PS_TOKEN=t; path=/").set_body_string("home"),
        )
        .mount(&s)
        .await;
    Mock::given(method("GET"))
        .and(path(COMPONENT))
        .and(header_regex("cookie", "PS_TOKEN=t"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            "<input type='hidden' name='ICSID' id='ICSID' value='S1/x=' /><input id='ICStateNum' value='5'/>",
        ))
        .mount(&s)
        .await;
    Mock::given(method("POST"))
        .and(path(COMPONENT))
        .and(body_string_contains("ICStateNum=5&"))
        .and(body_string_contains("ICSID=S1%2Fx%3D"))
        .and(body_string_contains("ICAction=DERIVED_SSTSRPT_REPORT_REQUEST_PB"))
        .respond_with(ResponseTemplate::new(200).set_body_string(format!(
            "<GENSCRIPT><![CDATA[window.open('{uri}/psc/csprd_5/view/%7bV2%7dabc/LAC_SR_TSRPT.pdf','','');]]></GENSCRIPT>"
        )))
        .mount(&s)
        .await;
    Mock::given(method("GET"))
        .and(path("/psc/csprd_5/view/%7bV2%7dabc/LAC_SR_TSRPT.pdf"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("Content-Type", "application/pdf")
                .set_body_bytes(b"%PDF-1.4 x".to_vec()),
        )
        .mount(&s)
        .await;
    s
}

fn ep(s: &MockServer) -> Endpoints {
    Endpoints { sso: s.uri(), campus: s.uri() }
}

#[tokio::test]
async fn signs_in_through_the_relays_and_downloads_the_pdf() {
    tokio::task::unconstrained(async move {
        let s = mock_portal().await;
        let cred = Credentials { user: "escsso\\123".into(), password: "right pass".into() };
        let pdf = fetch_unofficial_transcript(ep(&s), &cred).await.expect("fetch");
        assert_eq!(pdf, b"%PDF-1.4 x");
    })
    .await
}

#[tokio::test]
async fn a_wrong_password_is_reported_as_such() {
    tokio::task::unconstrained(async move {
        let s = mock_portal().await;
        let cred = Credentials { user: "escsso\\123".into(), password: "wrong".into() };
        match fetch_unofficial_transcript(ep(&s), &cred).await {
            Err(FetchError::BadLogin(m)) => assert_eq!(m, "Incorrect user ID or password."),
            other => panic!("expected BadLogin, got {other:?}"),
        }
    })
    .await
}

#[tokio::test]
async fn bad_report_responses_and_incomplete_sign_in_flows_are_rejected() {
    tokio::task::unconstrained(async move {
        use admit_laccd::Portal;
        let cred = Credentials { user: "u".into(), password: "p".into() };
        let s = MockServer::start().await;
        let signin = "/adfs/ls/idpinitiatedsignon.aspx";
        Mock::given(method("GET"))
            .and(path(signin))
            .respond_with(
                ResponseTemplate::new(200).set_body_string("<form action='/login'><input name='UserName'></form>"),
            )
            .mount(&s)
            .await;
        Mock::given(method("POST"))
            .and(path("/login"))
            .respond_with(ResponseTemplate::new(200).set_body_string("unknown step"))
            .mount(&s)
            .await;
        let p = Portal::new(ep(&s));
        assert!(p.sign_in(&cred).await.expect_err("missing SAML").to_string().contains("no SAMLResponse"));
        assert!(
            Portal::new(Endpoints { sso: "invalid URL".into(), campus: s.uri() })
                .sign_in(&cred)
                .await
                .expect_err("bad URL")
                .to_string()
                .contains("transport")
        );
        let s = mock_portal().await;
        for (status, body, ctype) in
            [(500, b"error".to_vec(), "application/pdf"), (200, b"not a PDF".to_vec(), "text/html")]
        {
            Mock::given(method("GET"))
                .and(path("/psc/csprd_5/view/%7bV2%7dabc/LAC_SR_TSRPT.pdf"))
                .respond_with(ResponseTemplate::new(status).insert_header("Content-Type", ctype).set_body_bytes(body))
                .with_priority(if status == 500 { 2 } else { 1 })
                .mount(&s)
                .await;
            let err = fetch_unofficial_transcript(
                ep(&s),
                &Credentials { user: "escsso\\123".into(), password: "right pass".into() },
            )
            .await
            .expect_err("bad download");
            if status == 500 {
                assert!(matches!(err, FetchError::Http { status: 500, .. }));
            } else {
                assert!(matches!(err, FetchError::NotPdf(_)));
            }
        }
        assert_eq!(html::report_url("<script>window.open('other.pdf')</script>").as_deref(), Some("other.pdf"));
        assert!(html::report_url("no report link").is_none());
        assert!(html::ic_state("<input name='ICSID' value='sid'>").is_none());
        assert_eq!(
            html::ic_state(
                "<form><input name='x' value='1'><input name='ICSID' value='s'></form><input id='ICStateNum' value='4'>"
            ),
            Some(html::IcState { sid: "s".into(), state_num: 4 })
        );
    })
    .await
}

#[test]
fn default_endpoints_are_the_live_hosts() {
    let ep = Endpoints::default();
    assert_eq!(ep.sso, "https://sso.laccd.edu");
    assert_eq!(ep.campus, "https://csprd.laccd.edu");
}

const SIGNIN_PATH: &str = "/adfs/ls/idpinitiatedsignon.aspx";
const STATE_PAGE: &str = "<input type='hidden' name='ICSID' id='ICSID' value='S1' /><input id='ICStateNum' value='5'/>";

fn cred() -> Credentials {
    Credentials { user: "u".into(), password: "p".into() }
}

fn portal_at(s: &MockServer, campus: &str) -> Portal {
    Portal::new(Endpoints { sso: s.uri(), campus: campus.to_string() })
}

fn login_form(action: &str) -> String {
    format!("<form method=\"post\" action=\"{action}\"><input name=\"UserName\" value=\"\"></form>")
}

fn saml_form(action: &str) -> String {
    format!("<form action=\"{action}\"><input name=\"SAMLResponse\" value=\"eA==\"></form>")
}

fn start_form(action: &str) -> String {
    format!("<form action=\"{action}\"><input name=\"sso_ticket\" value=\"t\"></form>")
}

async fn mount(s: &MockServer, verb: &str, p: &str, status: u16, body: &str) {
    Mock::given(method(verb))
        .and(path(p))
        .respond_with(ResponseTemplate::new(status).set_body_string(body))
        .mount(s)
        .await;
}

async fn transcript_failure(get_status: u16, get_body: &[u8], post: Option<(u16, Vec<u8>)>) -> FetchError {
    let s = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(COMPONENT))
        .respond_with(ResponseTemplate::new(get_status).set_body_bytes(get_body.to_vec()))
        .mount(&s)
        .await;
    if let Some((status, body)) = post {
        Mock::given(method("POST"))
            .and(path(COMPONENT))
            .respond_with(ResponseTemplate::new(status).set_body_bytes(body))
            .mount(&s)
            .await;
    }
    portal_at(&s, &s.uri()).unofficial_transcript().await.expect_err("transcript")
}

/// Answers one GET with headers and a chunked body that ends early, so
/// `bytes()` fails after `send()` has already succeeded.
fn spawn_truncated_pdf() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    std::thread::spawn(move || {
        let (mut sock, _) = listener.accept().expect("accept");
        let mut buf = [0u8; 4096];
        let _ = std::io::Read::read(&mut sock, &mut buf);
        let msg = b"HTTP/1.1 200 OK\r\nContent-Type: application/pdf\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n4\r\n%PDF";
        let _ = std::io::Write::write_all(&mut sock, msg);
    });
    port
}

#[tokio::test]
async fn sign_in_rejects_each_broken_step() {
    tokio::task::unconstrained(async move {
        // `Response::text` only fails when the body itself cannot be read.
        let port = spawn_truncated_pdf();
        let err =
            Portal::new(Endpoints { sso: format!("http://127.0.0.1:{port}"), campus: "http://campus.test".into() })
                .sign_in(&cred())
                .await
                .expect_err("truncated page");
        match err {
            FetchError::Transport { step: "sign-in page", source } => {
                let msg = source.to_string();
                assert!(msg.contains("body") || msg.contains("decode"), "{msg}");
            }
            other => panic!("expected a truncated sign-in page, got {other}"),
        }

        let s = MockServer::start().await;
        mount(&s, "GET", SIGNIN_PATH, 200, "<html></html>").await;
        let err = portal_at(&s, &s.uri()).sign_in(&cred()).await.expect_err("no form");
        assert!(matches!(err, FetchError::Shape { step: "sign-in page", what: "no UserName form" }), "{err}");

        let s = MockServer::start().await;
        mount(&s, "GET", SIGNIN_PATH, 200, &login_form("http://127.0.0.1:99999")).await;
        let err = portal_at(&s, &s.uri()).sign_in(&cred()).await.expect_err("bad action");
        assert!(matches!(err, FetchError::Url { step: "sign-in", .. }), "{err}");

        let s = MockServer::start().await;
        mount(&s, "GET", SIGNIN_PATH, 200, &login_form("/login")).await;
        mount(&s, "POST", "/login", 503, "down").await;
        let err = portal_at(&s, &s.uri()).sign_in(&cred()).await.expect_err("http");
        assert!(matches!(err, FetchError::Http { step: "sign-in", status: 503 }), "{err}");

        let s = MockServer::start().await;
        mount(&s, "GET", SIGNIN_PATH, 200, &login_form("/login")).await;
        mount(&s, "POST", "/login", 200, &saml_form("/saml")).await;
        mount(&s, "POST", "/saml", 502, "relay down").await;
        let err = portal_at(&s, &s.uri()).sign_in(&cred()).await.expect_err("saml http");
        assert!(matches!(err, FetchError::Http { step: "SAML relay", status: 502 }), "{err}");

        let s = MockServer::start().await;
        mount(&s, "GET", SIGNIN_PATH, 200, &login_form("/login")).await;
        mount(&s, "POST", "/login", 200, &saml_form("/saml")).await;
        mount(&s, "POST", "/saml", 200, "<html>no ticket</html>").await;
        let err = portal_at(&s, &s.uri()).sign_in(&cred()).await.expect_err("no ticket");
        assert!(matches!(err, FetchError::Shape { step: "SAML relay", what: "no PeopleSoft start form" }), "{err}");

        let s = MockServer::start().await;
        mount(&s, "GET", SIGNIN_PATH, 200, &login_form("/login")).await;
        mount(&s, "POST", "/login", 200, &saml_form("/saml")).await;
        mount(&s, "POST", "/saml", 200, &start_form("/start")).await;
        mount(&s, "POST", "/start", 500, "start down").await;
        let err = portal_at(&s, &s.uri()).sign_in(&cred()).await.expect_err("start http");
        assert!(matches!(err, FetchError::Http { step: "PeopleSoft start", status: 500 }), "{err}");

        let s = MockServer::start().await;
        mount(&s, "GET", SIGNIN_PATH, 200, &login_form("/login")).await;
        mount(&s, "POST", "/login", 200, &saml_form("/saml")).await;
        mount(&s, "POST", "/saml", 200, &start_form("/start")).await;
        mount(&s, "POST", "/start", 200, "home").await;
        let err = portal_at(&s, "http://campus.test").sign_in(&cred()).await.expect_err("wrong host");
        assert!(
            matches!(err, FetchError::Shape { step: "PeopleSoft start", what: "did not land on the student portal" }),
            "{err}"
        );
    })
    .await
}

#[tokio::test]
async fn transcript_rejects_each_broken_step() {
    tokio::task::unconstrained(async move {
        let http = transcript_failure(500, b"no", None).await;
        assert!(matches!(http, FetchError::Http { step: "transcript page", status: 500 }), "{http}");

        let shape = transcript_failure(200, b"<html></html>", None).await;
        assert!(matches!(shape, FetchError::Shape { step: "transcript page", .. }), "{shape}");

        let post = transcript_failure(200, STATE_PAGE.as_bytes(), Some((502, b"no".to_vec()))).await;
        assert!(matches!(post, FetchError::Http { step: "transcript request", status: 502 }), "{post}");

        let link = transcript_failure(200, STATE_PAGE.as_bytes(), Some((200, b"no link".to_vec()))).await;
        assert!(matches!(link, FetchError::Shape { step: "transcript request", .. }), "{link}");

        let bad = transcript_failure(
            200,
            STATE_PAGE.as_bytes(),
            Some((200, b"window.open('http://127.0.0.1:99999/x.pdf')".to_vec())),
        )
        .await;
        assert!(matches!(bad, FetchError::Url { step: "transcript download", .. }), "{bad}");

        let refused = transcript_failure(
            200,
            STATE_PAGE.as_bytes(),
            Some((200, b"window.open('http://127.0.0.1:1/no.pdf')".to_vec())),
        )
        .await;
        match refused {
            FetchError::Transport { step: "transcript download", source } => {
                let msg = source.to_string();
                assert!(msg.contains("sending") || msg.contains("connect") || msg.contains("refused"), "{msg}");
            }
            other => panic!("expected a refused download, got {other}"),
        }

        let port = spawn_truncated_pdf();
        let body = format!("window.open('http://127.0.0.1:{port}/x.pdf')");
        let cut = transcript_failure(200, STATE_PAGE.as_bytes(), Some((200, body.into_bytes()))).await;
        match cut {
            FetchError::Transport { step: "transcript download", source } => {
                let msg = source.to_string();
                assert!(msg.contains("body") || msg.contains("decode"), "{msg}");
            }
            other => panic!("expected a truncated download, got {other}"),
        }
    })
    .await
}
