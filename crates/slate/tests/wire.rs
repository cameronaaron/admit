//! The client against a mock Slate: expiry, the save request, and apply's
//! check that a save moved nothing else.

use admit_slate::answer::{self, Answer};
use admit_slate::{Client, Error, Portal, page};
use wiremock::matchers::{header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const PAGE: &str = include_str!("fixtures/page.html");
const REVIEW: &str = include_str!("fixtures/review.html");
const ID: &str = "7946b795-1470-4fc2-bc93-35f853dc8846";

/// Slate answers a POST without `Origin` with a redirect to sign-in.
/// Mounted first, so it only answers what the specific mocks don't.
async fn without_origin_is_bounced(server: &MockServer) {
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(302).insert_header("location", "/account/login?r=x"))
        .with_priority(10)
        .mount(server)
        .await;
}

fn portal(server: &MockServer) -> Portal {
    Portal { name: "ucla".into(), client: Client::new(&server.uri(), "_uid=x".into()) }
}

#[tokio::test]
async fn a_redirect_to_sign_in_is_expiry() {
    tokio::task::unconstrained(async move {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/apply/review"))
            .respond_with(
                ResponseTemplate::new(302).insert_header("location", "https://x/account/login?r=%2fapply%2freview"),
            )
            .mount(&server)
            .await;
        let err = portal(&server).review().await.expect_err("expired");
        assert!(matches!(err, Error::Expired { .. }), "{err}");
    })
    .await
}

#[tokio::test]
async fn review_reads_menu_and_issues() {
    tokio::task::unconstrained(async move {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/apply/review"))
            .respond_with(ResponseTemplate::new(200).set_body_string(REVIEW))
            .mount(&server)
            .await;
        let (nav, issues) = portal(&server).review().await.expect("review");
        assert_eq!(nav.iter().filter(|n| n.is_form()).count(), 19);
        let findings = portal(&server).audit().await.expect("audit");
        assert_eq!(findings.len(), issues.len());
        assert_eq!(findings.iter().filter(|f| f.is_violation()).count(), 14);
    })
    .await
}

#[tokio::test]
async fn save_posts_the_whole_page_with_the_change() {
    tokio::task::unconstrained(async move {
        let server = MockServer::start().await;
        without_origin_is_bounced(&server).await;
        Mock::given(method("POST"))
            .and(path("/apply/frm"))
            .and(query_param(ID, ""))
            .and(header("origin", server.uri().as_str()))
            .respond_with(
                ResponseTemplate::new(200).set_body_string("<script>parent.FW.Navigate('frm?next');</script>"),
            )
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/apply/frm"))
            .respond_with(ResponseTemplate::new(200).set_body_string(PAGE))
            .mount(&server)
            .await;
        let href = format!("frm?{ID}");
        let before = vec![page::page(&href, PAGE)];
        let want = std::collections::BTreeMap::from([(
            "sys:field:gd_lived_name_diff_legal_name".to_string(),
            Answer::One("Yes".into()),
        )])
        .into();
        let plan = answer::plan(&before, &want);
        assert_eq!(plan.changes.len(), 1);
        // The mock page doesn't change, so the re-read shows nothing moved.
        portal(&server).apply(&plan, &before).await.expect("applies");
        let req = &server.received_requests().await.expect("recorded")[0];
        let body = String::from_utf8_lossy(&req.body);
        assert!(
            req.headers
                .get("content-type")
                .is_some_and(|v| v.to_str().unwrap_or_default().starts_with("multipart/form-data"))
        );
        let posted = parts(&body);
        let get = |name: &str| posted.iter().filter(|(n, _)| n == name).map(|(_, v)| v.as_str()).collect::<Vec<_>>();
        assert_eq!(get("dirty"), ["1"]);
        // Slate clears what a save leaves out (live, 2026-10-06), so the post
        // carries every answer on the page, the change swapped in.
        assert_eq!(get("form_23b97894-f8ed-4d54-8746-27edd1d2b133"), ["1"], "the change, once");
        let page = &before[0];
        let id = |export: &str| {
            page.questions
                .iter()
                .find(|q| q.export.as_deref() == Some(export) && !q.readonly)
                .map(|q| q.id.clone())
                .expect(export)
        };
        assert_eq!(get(&format!("form_{}", id("sys:first"))), ["Pat"]);
        assert_eq!(get(&format!("form_{}", id("sys:email"))), ["pat@example.com"]);
        assert_eq!(
            get(&format!("form_{}", id("sys:field:gd_race_ethnicity"))),
            ["5c5b0b7a-73a6-4797-8d82-562aa4e31107"]
        );
        let birth = id("sys:birthdate");
        assert_eq!(
            [get(&format!("form_{birth}_m")), get(&format!("form_{birth}_d")), get(&format!("form_{birth}_y"))],
            [["01"], ["03"], ["2000"]]
        );
        assert_eq!(get(&format!("form_{}", id("sys:app:gd_parent1_highest_edu"))), [""], "a blank select posts empty");
        let copy = page
            .questions
            .iter()
            .find(|q| q.readonly && q.export.as_deref() == Some("sys:field:gd_lived_first"))
            .expect("formula copy");
        assert_eq!(get(&format!("form_{}", copy.id)), ["Pat"], "formula @sys:first computed");
    })
    .await
}

#[tokio::test]
async fn apply_stops_when_a_save_clears_another_answer() {
    tokio::task::unconstrained(async move {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/apply/frm"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;
        let cleared = PAGE.replace("value=\"Pat\"", "value=\"\"");
        Mock::given(method("GET"))
            .and(path("/apply/frm"))
            .respond_with(ResponseTemplate::new(200).set_body_string(cleared))
            .mount(&server)
            .await;
        let href = format!("frm?{ID}");
        let before = vec![page::page(&href, PAGE)];
        let want = std::collections::BTreeMap::from([(
            "sys:app:gd_parent1_highest_edu".to_string(),
            Answer::One("Some High School".into()),
        )])
        .into();
        let plan = answer::plan(&before, &want);
        let err = portal(&server).apply(&plan, &before).await.expect_err("collateral change");
        assert!(err.to_string().contains("sys:first \"Pat\" -> (blank)"), "{err}");
    })
    .await
}

const APPLICATIONS: &str = include_str!("fixtures/applications.html");

#[test]
fn the_apply_list_names_every_application() {
    let apps = page::applications(APPLICATIONS);
    let rows: Vec<(&str, &str, &str)> =
        apps.iter().map(|a| (a.id.as_str(), a.name.as_str(), a.status.as_str())).collect();
    assert_eq!(
        rows,
        [
            ("c7c162d5-c56f-4433-9065-aaeeb1334c14", "2027-2028 Graduate Application", "In Progress"),
            ("a9c437e5-af93-4920-8eff-3846b8f4bc68", "2026-2027 Graduate Application", "In Progress"),
        ]
    );
}

#[tokio::test]
async fn sign_in_passes_the_cookie_test_then_posts_the_form() {
    tokio::task::unconstrained(async move {
        use wiremock::matchers::body_string_contains;
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/account/login"))
            .and(query_param("cookie", "1"))
            .respond_with(ResponseTemplate::new(200).set_body_string("<h1>Login</h1>"))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/account/login"))
            .respond_with(
                ResponseTemplate::new(307)
                    .insert_header("location", "/account/login?r=x&cookie=1")
                    .append_header("set-cookie", "_cookie_test=1; path=/")
                    .append_header("set-cookie", "_node=n; path=/"),
            )
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/account/login"))
            .and(header("origin", server.uri().as_str()))
            .and(body_string_contains("email=a%40b.c"))
            .and(body_string_contains("password=right"))
            .respond_with(
                ResponseTemplate::new(302)
                    .insert_header("location", "/apply/")
                    .append_header("set-cookie", "_uid=signed-in; path=/; secure; HttpOnly"),
            )
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/account/login"))
            .respond_with(ResponseTemplate::new(200).set_body_string("<p class=\"error\">Invalid</p>"))
            .mount(&server)
            .await;
        let cookie = Client::login(&server.uri(), "a@b.c", "right").await.expect("signs in");
        assert_eq!(cookie, "_cookie_test=1; _node=n; _uid=signed-in");
        let posts: Vec<_> = server
            .received_requests()
            .await
            .expect("recorded")
            .into_iter()
            .filter(|r| r.method.as_str() == "POST")
            .collect();
        let q = posts[0].url.query().unwrap_or_default();
        assert!(q.starts_with("r=") && q.ends_with("%2fapply%2f"), "posted to the login URL, back to /apply/: {q}");
        assert!(posts[0].url.query().is_some_and(|q| !q.contains("cookie=1")), "not the cookie-test URL");
        assert!(
            posts[0].headers.get("cookie").is_some_and(|c| c.to_str().unwrap_or_default().contains("_cookie_test=1"))
        );
        let err = Client::login(&server.uri(), "a@b.c", "wrong").await.expect_err("rejected");
        assert!(matches!(&err, Error::BadLogin { status: 200, message } if message == "Invalid"), "{err}");
        assert!(err.to_string().starts_with("sign-in rejected (HTTP 200): \"Invalid\": check"), "{err}");
    })
    .await
}

#[tokio::test]
async fn opening_an_application_switches_the_cookie() {
    tokio::task::unconstrained(async move {
        use wiremock::matchers::body_string;
        let server = MockServer::start().await;
        without_origin_is_bounced(&server).await;
        Mock::given(method("POST"))
            .and(path("/apply/"))
            .and(query_param("id", "app-2027"))
            .and(header("origin", server.uri().as_str()))
            .and(query_param("cmd", "detail"))
            .and(body_string("cmd=save"))
            .respond_with(
                ResponseTemplate::new(200)
                    .append_header("set-cookie", ".application=app-2027; path=/; secure; HttpOnly")
                    .set_body_string("<script>parent.FW.Navigate('frm?x');</script>"),
            )
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/apply/"))
            .respond_with(ResponseTemplate::new(200).set_body_string("<script></script>"))
            .mount(&server)
            .await;
        let mut c = Client::new(&server.uri(), "_uid=u; .application=app-2026".into());
        c.open("app-2027").await.expect("opens");
        assert_eq!(c.cookie(), "_uid=u; .application=app-2027");
        let err = c.open("someone-elses").await.expect_err("no switch");
        assert!(err.to_string().contains("did not switch"), "{err}");
        assert_eq!(c.cookie(), "_uid=u; .application=app-2027", "a failed open keeps the old cookie");
    })
    .await
}

/// `(name, value)` pairs of a multipart body.
fn parts(body: &str) -> Vec<(String, String)> {
    body.split("Content-Disposition: form-data; name=\"")
        .skip(1)
        .filter_map(|p| {
            let (name, rest) = p.split_once('"')?;
            let value = rest.split_once("\r\n\r\n")?.1;
            let value = &value[..value.find("\r\n--").unwrap_or(value.len())];
            Some((name.to_string(), value.to_string()))
        })
        .collect()
}

const UPLOAD: &str = include_str!("fixtures/upload_question.html");

#[tokio::test]
async fn an_upload_slot_is_posted_empty_like_a_browser_with_no_file_chosen() {
    tokio::task::unconstrained(async move {
    // Personal Statement's upload question as captured 2026-10-06, plus a
    // text question to change.
    let html = format!(
        "{UPLOAD}<div class=\"form_question\" data-id=\"q2\" data-type=\"text\" data-export=\"sys:x\"><input type=\"text\" name=\"form_q2\" value=\"a\" /></div>"
    );
    let p = page::page("frm?ps", &html);
    let want = std::collections::BTreeMap::from([("sys:x".to_string(), Answer::One("b".into()))]).into();
    let plan = answer::plan(std::slice::from_ref(&p), &want);
    let changes: Vec<&answer::Change> = plan.changes.iter().collect();
    let post = answer::page_post(&p, &changes).expect("posts");
    let slot = "material_1f434701-674e-4cd8-92bd-592b51ae8ef7";
    assert_eq!(post.empty_files, [slot]);
    assert!(post.fields.contains(&(format!("{slot}_required"), "1".to_string())), "{:?}", post.fields);
    assert!(post.fields.contains(&("form_q2".to_string(), "b".to_string())));

    let server = MockServer::start().await;
    Mock::given(method("POST")).respond_with(ResponseTemplate::new(200)).mount(&server).await;
    let c = Client::new(&server.uri(), "_uid=x".into());
    c.save("frm?ps", &post.fields, &post.empty_files).await.expect("saved");
    let body = String::from_utf8_lossy(&server.received_requests().await.expect("recorded")[0].body).to_string();
    assert!(
        body.contains(&format!("name=\"{slot}\"; filename=\"\"\r\nContent-Type: application/octet-stream\r\n\r\n\r\n")),
        "an empty file part: {body}"
    );
})
    .await
}

const FEE_WAIVER: &str = include_str!("fixtures/fee_waiver.html");

#[tokio::test]
async fn a_failed_open_save_or_audit_is_reported() {
    tokio::task::unconstrained(async move {
        let mut client = Client::new("invalid URL", String::new());
        assert!(matches!(client.open("id").await, Err(Error::Transport { .. })));
        assert!(matches!(client.save("frm?x", &[], &[]).await, Err(Error::Transport { .. })));
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/apply/review"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;
        assert!(matches!(portal(&server).audit().await, Err(Error::Http { .. })));
    })
    .await
}

#[path = "../../liaison/tests/support/broken.rs"]
mod broken;

#[tokio::test]
async fn opening_an_application_reports_a_truncated_answer() {
    tokio::task::unconstrained(async move {
        let _ = broken::EMPTY;
        let url = broken::responses(vec![broken::TRUNCATED]);
        let mut client = Client::new(&url, "_uid=x".into());
        assert!(matches!(client.open("id").await, Err(Error::Transport { .. })));
    })
    .await
}

#[test]
fn the_fee_waiver_income_limit_is_computed_as_the_browser_does() {
    // Captured 2026-10-06. The page lists the limits: 1 person $29,526,
    // 3 persons $50,542; its hidden field computes them from the household.
    let p = page::page("frm?fw", FEE_WAIVER);
    let limit = p.questions.iter().find(|q| q.inputs.iter().any(|i| i.formula.is_some())).expect("formula field");
    let post_for = |household: &str| {
        let want = std::collections::BTreeMap::from([(
            "sys:app:gd_fee_waiver_household_number".to_string(),
            Answer::One(household.into()),
        )])
        .into();
        let plan = answer::plan(std::slice::from_ref(&p), &want);
        assert!(plan.findings.is_empty(), "{:?}", plan.findings);
        let changes: Vec<&answer::Change> = plan.changes.iter().collect();
        let post = answer::page_post(&p, &changes).expect("posts");
        post.fields.iter().find(|(n, _)| *n == format!("form_{}", limit.id)).map(|(_, v)| v.clone())
    };
    assert_eq!(post_for("1").as_deref(), Some("29526"));
    assert_eq!(post_for("3").as_deref(), Some("50542"));
    // Nothing chosen: the reference is blank, so is the result.
    let post = answer::page_post(&p, &[]).expect("posts");
    assert_eq!(post.fields.iter().find(|(n, _)| *n == format!("form_{}", limit.id)).map(|(_, v)| v.as_str()), Some(""));
}
