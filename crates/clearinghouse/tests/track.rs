//! Tracking against the responses captured live on 2026-10-10 (personal
//! fields removed): the request shapes, and what is read from them.
use admit_clearinghouse::{Client, Error, orders};
use admit_domain::orders::Stage;
use serde_json::{Value, json};
use wiremock::matchers::{body_json, header, header_regex, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn fixture(name: &str) -> Value {
    let p = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    serde_json::from_str(&std::fs::read_to_string(p).expect("fixture")).expect("json")
}

#[test]
fn the_captured_order_reads_as_sent_to_azusa_pacific() {
    let o = orders("97852158", &fixture("trackorder.json"), &fixture("recipient.json")).expect("orders");
    assert_eq!(o.len(), 1);
    let o = &o[0];
    assert_eq!((o.via, o.id.as_str()), ("clearinghouse", "97852158"));
    assert_eq!((o.from.as_str(), o.to.as_str()), ("Connecticut College", "AZUSA PACIFIC UNIVERSITY"));
    assert_eq!((o.status.as_str(), o.stage), ("Electronic Transcript Sent", Stage::Sent));
    assert_eq!((o.ordered.as_str(), o.changed.as_deref()), ("2026-10-10", Some("2026-10-10")));
}

#[test]
fn a_hold_outranks_any_status_and_unknown_codes_stay_open() {
    let mut r = fixture("recipient.json");
    r["recipients"][0]["statusInfo"]["current"]["status"] = json!("V");
    let o = orders("1", &fixture("trackorder.json"), &r).expect("orders");
    assert_eq!(o[0].stage, Stage::Open);
    r["recipients"][0]["statusInfo"]["current"]["multipleHoldInfo"] = json!([{"holdTxt": "Financial"}]);
    r["recipients"][0]["statusInfo"]["current"]["status"] = json!("ETS");
    assert_eq!(orders("1", &fixture("trackorder.json"), &r).expect("orders")[0].stage, Stage::Held);
}

#[test]
fn a_changed_shape_is_an_error_not_an_empty_answer() {
    let summary = fixture("trackorder.json");
    let err = orders("1", &summary, &json!({})).expect_err("no list");
    assert!(err.to_string().contains("no recipients list"), "{err}");
    let err = orders("1", &json!({}), &fixture("recipient.json")).expect_err("no date");
    assert!(err.to_string().contains("orderDate"), "{err}");
}

#[tokio::test]
async fn tracking_posts_the_email_and_uses_the_returned_bearer() {
    tokio::task::unconstrained(async {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/tsorder/trackorder"))
            .and(header("Origin", server.uri().as_str()))
            .and(header_regex("user-agent", "^admit/"))
            .and(body_json(json!({"email": "a@b.c", "toOrderId": "97852158"})))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("Authorization-Token", "Bearer t0k")
                    .set_body_json(fixture("trackorder.json")),
            )
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/tsorder/trackorder/97852158/recipient"))
            .and(header("Authorization", "Bearer t0k"))
            .respond_with(ResponseTemplate::new(200).set_body_json(fixture("recipient.json")))
            .expect(1)
            .mount(&server)
            .await;
        let o = Client::new(&format!("{}/", server.uri())).track("a@b.c", "97852158").await.expect("track");
        assert_eq!(o[0].stage, Stage::Sent);
    })
    .await
}

#[tokio::test]
async fn a_refusal_carries_the_clearinghouse_message() {
    tokio::task::unconstrained(async {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/tsorder/trackorder"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                json!({"status": "fail", "message": "The entered information does not match. Please try again."}),
            ))
            .mount(&server)
            .await;
        let err = Client::new(&server.uri()).track("x@y.z", "97852158").await.expect_err("refused");
        assert!(matches!(&err, Error::Refused { order, .. } if order == "97852158"), "{err}");
        assert!(err.to_string().ends_with("does not match. Please try again."), "{err}");
    })
    .await
}

#[tokio::test]
async fn each_failing_step_is_named() {
    tokio::task::unconstrained(async {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/tsorder/trackorder"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"status": "success"})))
            .up_to_n_times(1)
            .mount(&server)
            .await;
        let c = Client::new(&server.uri());
        let err = c.track("a@b.c", "1").await.expect_err("no token");
        assert!(err.to_string().contains("no Authorization-Token header"), "{err}");
        Mock::given(method("POST"))
            .and(path("/api/tsorder/trackorder"))
            .respond_with(ResponseTemplate::new(503))
            .up_to_n_times(1)
            .mount(&server)
            .await;
        assert!(matches!(c.track("a@b.c", "1").await, Err(Error::Http { step: "track order", status: 503 })));
        Mock::given(method("POST"))
            .and(path("/api/tsorder/trackorder"))
            .respond_with(ResponseTemplate::new(200).set_body_string("<html>"))
            .up_to_n_times(1)
            .mount(&server)
            .await;
        assert!(matches!(c.track("a@b.c", "1").await, Err(Error::Transport { step: "track order", .. })));
        Mock::given(method("POST"))
            .and(path("/api/tsorder/trackorder"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("Authorization-Token", "Bearer t")
                    .set_body_json(json!({"status": "success"})),
            )
            .mount(&server)
            .await;
        Mock::given(method("GET")).respond_with(ResponseTemplate::new(401)).mount(&server).await;
        assert!(matches!(c.track("a@b.c", "1").await, Err(Error::Http { step: "order recipients", status: 401 })));
    })
    .await
}

#[tokio::test]
async fn an_unreachable_host_is_a_transport_error() {
    let err = Client::new("http://127.0.0.1:9").track("a@b.c", "1").await.expect_err("closed port");
    assert!(matches!(err, Error::Transport { step: "track order", .. }), "{err}");
}

/// The bytes on the wire, which wiremock normalises away: the live server
/// refuses `content-type` (415) and a request with no User-Agent (403).
#[tokio::test]
async fn header_names_go_out_capitalised_with_a_user_agent() {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let base = format!("http://{}", listener.local_addr().expect("addr"));
    let seen = std::thread::spawn(move || {
        let (mut sock, _) = listener.accept().expect("accept");
        let mut buf = vec![0; 8192];
        let n = sock.read(&mut buf).expect("read");
        let body = r#"{"status":"fail","message":"no"}"#;
        let _ = write!(
            sock,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        String::from_utf8_lossy(&buf[..n]).into_owned()
    });
    let err = Client::new(&base).track("a@b.c", "1").await.expect_err("refused");
    assert!(matches!(err, Error::Refused { .. }), "{err}");
    let request = seen.join().expect("listener");
    assert!(request.contains("\r\nContent-Type: application/json\r\n"), "{request}");
    assert!(request.contains("\r\nUser-Agent: admit/"), "{request}");
}
