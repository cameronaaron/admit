//! Portal and client branches the captured pages do not exercise.

use std::collections::BTreeMap;

use admit_slate::answer::{self, Answer};
use admit_slate::institutions::{Institution, InstitutionAdd};
use admit_slate::jobs::{self, Job};
use admit_slate::page::{self, ChecklistItem};
use admit_slate::{Client, Error, Plan, Portal, Status};
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[path = "../../liaison/tests/support/broken.rs"]
mod broken;

fn portal(s: &MockServer) -> Portal {
    Portal { name: "test".into(), client: Client::new(&s.uri(), "_uid=x".into()) }
}

fn job() -> Job {
    Job {
        employer: "Clinic".into(),
        title: "Care".into(),
        from: "2020-01".into(),
        to: None,
        hours: 10,
        description: "Care".into(),
        profile_title: None,
    }
}

fn school() -> Institution {
    Institution {
        name: "Connecticut College".into(),
        profile: None,
        key: None,
        city: "New London".into(),
        region: "CT".into(),
        country: "US".into(),
        from: "2017-08".into(),
        to: "2021-05".into(),
        level: "U".into(),
        degree: None,
        conferred: None,
        transcript: "t.pdf".into(),
        major: Some("Psychology".into()),
        gpa: None,
        gpa_scale: None,
    }
}

fn blank_school() -> Institution {
    Institution { from: String::new(), to: String::new(), major: None, gpa: None, gpa_scale: None, ..school() }
}

fn job_page(rows: &str) -> String {
    format!("<h1>Professional Experience</h1><div id=\"part_table\"><table><tbody>{rows}</tbody></table></div>")
}

fn menu(links: &str) -> String {
    format!("<div id=\"menu\"><ul>{links}</ul></div><h1>Review</h1>")
}

async fn get(s: &MockServer, p: &str, body: &str) {
    Mock::given(method("GET"))
        .and(path(format!("/apply/{p}")))
        .respond_with(ResponseTemplate::new(200).set_body_string(body))
        .mount(s)
        .await;
}

#[tokio::test]
async fn transport_failures_name_the_request_that_failed() {
    tokio::task::unconstrained(async move {
        let p = Portal { name: "t".into(), client: Client::new("invalid URL", String::new()) };
        let add = jobs::plan(&[], &[job()]).0.into_iter().next().expect("add");
        let inst = InstitutionAdd { institution: school(), fields: vec![("cmd".into(), "save".into())] };
        assert!(matches!(p.status().await, Err(Error::Transport { .. })));
        for err in [
            p.pages().await.expect_err("pages"),
            p.institutions().await.expect_err("schools"),
            p.plan_institutions(&mut Plan::default(), &[school()]).await.expect_err("plan schools"),
            p.coverage(&Default::default()).await.expect_err("coverage"),
            p.plan_jobs(&mut Plan::default(), &[job()]).await.expect_err("plan jobs"),
            p.add_job(&add).await.expect_err("add job"),
            p.add_institution(&inst, vec![], "a.pdf").await.expect_err("add school"),
            p.audit().await.expect_err("audit"),
        ] {
            assert!(matches!(err, Error::Transport { .. }), "{err}");
        }
        let info = page::page("frm?info", "<h1>Additional Academic Info</h1>");
        assert!(matches!(
            p.plan_school_details(&mut Plan::default(), &[info], &[school()]).await,
            Err(Error::Transport { .. })
        ));
        let c = Client::new("invalid URL", String::new());
        assert!(matches!(c.fetch_cities("US:CA").await, Err(Error::Transport { .. })));
        assert!(matches!(c.fetch_schools("US:CA", Some("Los Angeles")).await, Err(Error::Transport { .. })));
        assert!(matches!(c.fetch_schools("US:CA", None).await, Err(Error::Transport { .. })));
    })
    .await
}

#[tokio::test]
async fn login_follows_an_absolute_redirect_and_reports_a_failed_post() {
    tokio::task::unconstrained(async move {
        let server = MockServer::start().await;
        let next = format!("{}/account/login?cookie=1", server.uri());
        Mock::given(method("GET"))
            .and(path("/account/login"))
            .and(query_param("cookie", "1"))
            .respond_with(ResponseTemplate::new(200))
            .with_priority(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/account/login"))
            .respond_with(ResponseTemplate::new(307).insert_header("location", next))
            .with_priority(2)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/account/login"))
            .respond_with(ResponseTemplate::new(302).append_header("set-cookie", "_uid=ok; path=/"))
            .mount(&server)
            .await;
        let cookie = Client::login(&server.uri(), "a@b.c", "pw").await.expect("signed in");
        assert!(cookie.contains("_uid=ok"), "{cookie}");

        let url = broken::responses(vec![broken::EMPTY]);
        let err = Client::login(&url, "u", "p").await.expect_err("post");
        assert!(matches!(err, Error::Transport { method: "POST", .. }), "{err}");

        assert!(matches!(Client::login("invalid URL", "u", "p").await, Err(Error::Transport { .. })));
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/account/login"))
            .and(query_param("cookie", "1"))
            .respond_with(ResponseTemplate::new(200))
            .with_priority(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/account/login"))
            .respond_with(ResponseTemplate::new(307).insert_header("location", "/account/login?cookie=1"))
            .with_priority(2)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/account/login"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;
        assert!(matches!(Client::login(&server.uri(), "a@b.c", "pw").await, Err(Error::BadLogin { .. })));
        let url = broken::responses(vec![broken::EMPTY, broken::TRUNCATED]);
        assert!(matches!(Client::login(&url, "u", "p").await, Err(Error::Transport { .. })));
    })
    .await
}

#[tokio::test]
async fn a_redirect_without_a_location_is_an_http_error() {
    tokio::task::unconstrained(async move {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/apply/gone"))
            .respond_with(ResponseTemplate::new(302))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/apply/file"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"%PDF"))
            .mount(&server)
            .await;
        let c = Client::new(&server.uri(), "_uid=x".into());
        let err = c.get("gone").await.expect_err("redirect");
        assert!(err.to_string().contains("redirect to"), "{err}");
        assert!(matches!(c.get_bytes("gone").await, Err(Error::Http { .. })));
        assert_eq!(c.get_bytes("file").await.expect("pdf"), b"%PDF");
    })
    .await
}

#[tokio::test]
async fn school_lookups_read_cities_and_schools() {
    tokio::task::unconstrained(async move {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/apply/aca"))
            .and(query_param("cmd", "fetch_cities"))
            .respond_with(ResponseTemplate::new(200).set_body_string("[\"Los Angeles\"]"))
            .with_priority(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/apply/aca"))
            .and(query_param("cmd", "fetch_schools"))
            .respond_with(ResponseTemplate::new(200).set_body_string("[]"))
            .with_priority(1)
            .mount(&server)
            .await;
        let c = Client::new(&server.uri(), "_uid=x".into());
        assert!(c.fetch_cities("US:CA").await.expect("cities").contains("Los Angeles"));
        assert_eq!(c.fetch_schools("US:CA", None).await.expect("schools"), "[]");
        assert_eq!(c.fetch_schools("US:CA", Some("Los Angeles")).await.expect("city"), "[]");
    })
    .await
}

#[tokio::test]
async fn pages_fill_an_address_widget_and_stop_when_a_fetch_fails() {
    tokio::task::unconstrained(async move {
        let server = MockServer::start().await;
        let p = portal(&server);
        get(&server, "review", "not review").await;
        assert!(p.pages().await.is_err());

        let review = menu("<a href=\"frm?basic\" onclick=\"return apply_navigate(this);\">Basic</a>");
        Mock::given(method("GET"))
            .and(path("/apply/review"))
            .respond_with(ResponseTemplate::new(200).set_body_string(review))
            .with_priority(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/apply/frm"))
            .respond_with(ResponseTemplate::new(500).set_body_string("no"))
            .mount(&server)
            .await;
        assert!(matches!(p.pages().await, Err(Error::Http { .. })));
    })
    .await
}

#[tokio::test]
async fn pages_read_the_address_popup() {
    tokio::task::unconstrained(async move {
    let server = MockServer::start().await;
    let review = menu("<a href=\"frm?basic\" onclick=\"return apply_navigate(this);\">Basic</a>");
    get(&server, "review", &review).await;
    let form = "<a class=\"widget_add\" data-href=\"/apply/frm?cmd=widget&form=1\">Insert</a>";
    Mock::given(method("GET"))
        .and(path("/apply/frm"))
        .and(query_param("cmd", "widget"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            "<div class=\"form_question\" data-id=\"ty\" data-type=\"select\" data-export=\"sys:address:type\"><select name=\"form_ty\"><option value=\"mailing\">Mailing</option></select></div>",
        ))
        .with_priority(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/apply/frm"))
        .respond_with(ResponseTemplate::new(200).set_body_string(form))
        .mount(&server)
        .await;
    let pages = portal(&server).pages().await.expect("pages");
    assert_eq!(pages[0].title, "Basic");
    assert_eq!(pages[0].addresses.as_ref().expect("widget").form.len(), 1);

    let server = MockServer::start().await;
    get(&server, "review", &review).await;
    Mock::given(method("GET"))
        .and(path("/apply/frm"))
        .and(query_param("cmd", "widget"))
        .respond_with(ResponseTemplate::new(500))
        .with_priority(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/apply/frm"))
        .respond_with(ResponseTemplate::new(200).set_body_string(form))
        .mount(&server)
        .await;
    assert!(matches!(portal(&server).pages().await, Err(Error::Http { .. })));
})
    .await
}

#[tokio::test]
async fn apply_stops_on_a_missing_page_a_bad_formula_or_a_failed_save() {
    tokio::task::unconstrained(async move {
    let page = page::page(
        "frm?x",
        "<div class=\"form_question\" data-id=\"q\" data-type=\"text\" data-export=\"q\"><input name=\"form_q\" value=\"a\"></div><div class=\"form_question\" data-id=\"f\" data-type=\"text\" data-readonly=\"1\"><input name=\"form_f\" data-formula=\"1 +\" value=\"1\"></div>",
    );
    let want = BTreeMap::from([("q".to_string(), Answer::One("b".into()))]).into();
    let plan = answer::plan(std::slice::from_ref(&page), &want);
    let server = MockServer::start().await;
    let err = portal(&server).apply(&plan, &[]).await.expect_err("missing");
    assert!(err.to_string().contains("not among the pages"), "{err}");
    let err = portal(&server).apply(&plan, &[page]).await.expect_err("formula");
    assert!(err.to_string().contains("does not parse"), "{err}");

    let page = page::page(
        "frm?x",
        "<div class=\"form_question\" data-id=\"q\" data-type=\"text\" data-export=\"q\"><input name=\"form_q\" value=\"a\"></div>",
    );
    let plan = answer::plan(std::slice::from_ref(&page), &want);
    Mock::given(method("POST")).and(path("/apply/frm")).respond_with(ResponseTemplate::new(500)).mount(&server).await;
    assert!(matches!(portal(&server).apply(&plan, std::slice::from_ref(&page)).await, Err(Error::Http { .. })));
    let server = MockServer::start().await;
    Mock::given(method("POST")).and(path("/apply/frm")).respond_with(ResponseTemplate::new(200)).mount(&server).await;
    Mock::given(method("GET")).and(path("/apply/frm")).respond_with(ResponseTemplate::new(500)).mount(&server).await;
    assert!(matches!(portal(&server).apply(&plan, &[page]).await, Err(Error::Http { .. })));
})
    .await
}

#[tokio::test]
async fn an_address_insert_reports_a_failed_reread() {
    tokio::task::unconstrained(async move {
    let server = MockServer::start().await;
    let before = {
        let mut p =
            page::page("frm?basic", "<a class=\"widget_add\" data-href=\"/apply/frm?cmd=widget&form=1\">Insert</a>");
        p.addresses.as_mut().expect("widget").form = page::page(
            "w",
            "<div class=\"form_question\" data-id=\"ty\" data-type=\"select\" data-export=\"sys:address:type\"><select name=\"form_ty\"><option value=\"mailing\">Mailing</option></select></div><div class=\"form_question\" data-id=\"b\" data-type=\"text\" data-export=\"sys:address_block\"><input name=\"form_b\"></div>",
        )
        .questions;
        p
    };
    let file = admit_slate::AnswersFile {
        addresses: vec![admit_slate::Address {
            kind: "mailing".into(),
            street: "1 St".into(),
            city: "Town".into(),
            region: "CA".into(),
            country: "US".into(),
            postal: "00000".into(),
        }],
        ..Default::default()
    };
    let plan = answer::plan(std::slice::from_ref(&before), &file);
    assert_eq!(plan.addresses.len(), 1, "{:?}", plan.findings);
    Mock::given(method("POST")).respond_with(ResponseTemplate::new(500)).mount(&server).await;
    assert!(matches!(portal(&server).apply(&plan, std::slice::from_ref(&before)).await, Err(Error::Http { .. })));
    let server = MockServer::start().await;
    Mock::given(method("POST")).respond_with(ResponseTemplate::new(200)).mount(&server).await;
    Mock::given(method("GET")).respond_with(ResponseTemplate::new(500)).mount(&server).await;
    assert!(matches!(portal(&server).apply(&plan, &[before]).await, Err(Error::Http { .. })));
})
    .await
}

#[tokio::test]
async fn status_reads_an_update_and_reports_a_failed_one() {
    tokio::task::unconstrained(async move {
    let posted = "<div id=\"part_checklist_App\"><h3>Application</h3><table><tr class=\"plain\"><td></td><td>Awaiting</td><td>Essay</td><td></td></tr></table></div> was last posted June 2, 2026.";
    let server = MockServer::start().await;
    get(&server, "status", posted).await;
    get(&server, "update", "<a id=\"pager_download\" data-href=\"update?cmd=stream&id=abc\">").await;
    let status = portal(&server).status().await.expect("status");
    assert_eq!(status.update.as_ref().expect("update").pdf.as_deref(), Some("update?cmd=stream&id=abc"));

    let server = MockServer::start().await;
    get(&server, "status", posted).await;
    Mock::given(method("GET")).and(path("/apply/update")).respond_with(ResponseTemplate::new(500)).mount(&server).await;
    assert!(matches!(portal(&server).status().await, Err(Error::Http { .. })));

    let quiet = Status {
        items: vec![ChecklistItem {
            list: "L".into(),
            status: "Received".into(),
            detail: "d".into(),
            date: String::new(),
        }],
        update: None,
    };
    assert!(quiet.findings("p").is_empty());
})
    .await
}

#[tokio::test]
async fn institution_and_coverage_fetches_report_the_page_that_failed() {
    tokio::task::unconstrained(async move {
    let server = MockServer::start().await;
    let p = portal(&server);
    get(&server, "aca", "<h1>Academic History</h1><div id=\"part_table\"><table><tbody></tbody></table></div>").await;
    Mock::given(method("GET"))
        .and(path("/apply/aca"))
        .and(query_param("id", ""))
        .respond_with(ResponseTemplate::new(500))
        .with_priority(1)
        .mount(&server)
        .await;
    assert!(matches!(p.institutions().await, Err(Error::Http { .. })));

    let review = menu(
        "<a href=\"frm?x\" onclick=\"return apply_navigate(this);\">Form</a><a href=\"tes\" onclick=\"return apply_navigate(this);\">Tests</a><a href=\"review\" onclick=\"return apply_navigate(this);\">Review</a>",
    );
    get(&server, "review", &review).await;
    Mock::given(method("GET")).and(path("/apply/frm")).respond_with(ResponseTemplate::new(500)).mount(&server).await;
    assert!(matches!(p.coverage(&Default::default()).await, Err(Error::Http { .. })));

    let server = MockServer::start().await;
    let review = menu(
        "<a href=\"job\" onclick=\"return apply_navigate(this);\">Jobs</a><a href=\"review\" onclick=\"return apply_navigate(this);\">Review</a>",
    );
    get(&server, "review", &review).await;
    Mock::given(method("GET")).and(path("/apply/job")).respond_with(ResponseTemplate::new(500)).mount(&server).await;
    assert!(matches!(portal(&server).coverage(&Default::default()).await, Err(Error::Http { .. })));

    let server = MockServer::start().await;
    let review = menu(
        "<a href=\"tes\" onclick=\"return apply_navigate(this);\">Tests</a><a href=\"review\" onclick=\"return apply_navigate(this);\">Review</a>",
    );
    get(&server, "review", &review).await;
    get(&server, "tes", "<div id=\"part_table\"><table><tbody><tr><td>GRE</td></tr></tbody></table></div>").await;
    let lines = portal(&server).coverage(&Default::default()).await.expect("coverage");
    assert!(lines.iter().any(|f| f.to_string().contains("1 row")), "{lines:?}");
})
    .await
}

#[tokio::test]
async fn adding_a_job_checks_the_new_row_and_a_wrong_page_afterwards() {
    tokio::task::unconstrained(async move {
        let add = jobs::plan(&[], &[job()]).0.into_iter().next().expect("add");
        let before = job_page("<tr><td>Clinic</td><td>01/2019–Present</td></tr>");
        let after = job_page(
            "<tr><td>Clinic</td><td>01/2019–Present</td></tr><tr><td>Clinic</td><td>01/2020–Present</td></tr>",
        );
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/apply/job"))
            .respond_with(ResponseTemplate::new(200).set_body_string(before))
            .up_to_n_times(1)
            .with_priority(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/apply/job"))
            .respond_with(ResponseTemplate::new(200).set_body_string(after))
            .with_priority(2)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/apply/job"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;
        portal(&server).add_job(&add).await.expect("added");

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/apply/job"))
            .respond_with(ResponseTemplate::new(200).set_body_string(job_page("")))
            .up_to_n_times(1)
            .with_priority(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/apply/job"))
            .respond_with(ResponseTemplate::new(200).set_body_string("Wrong"))
            .with_priority(2)
            .mount(&server)
            .await;
        Mock::given(method("POST")).respond_with(ResponseTemplate::new(200)).mount(&server).await;
        let err = portal(&server).add_job(&add).await.expect_err("not the page");
        assert!(err.to_string().contains("not the Professional"), "{err}");

        let server = MockServer::start().await;
        get(&server, "job", &job_page("")).await;
        Mock::given(method("POST"))
            .and(path("/apply/job"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;
        assert!(matches!(portal(&server).add_job(&add).await, Err(Error::Http { .. })));

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/apply/job"))
            .respond_with(ResponseTemplate::new(200).set_body_string(job_page("")))
            .up_to_n_times(1)
            .with_priority(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/apply/job"))
            .respond_with(ResponseTemplate::new(500))
            .with_priority(2)
            .mount(&server)
            .await;
        Mock::given(method("POST")).respond_with(ResponseTemplate::new(200)).mount(&server).await;
        assert!(matches!(portal(&server).add_job(&add).await, Err(Error::Http { .. })));
    })
    .await
}

#[tokio::test]
async fn adding_an_institution_stops_when_the_list_cannot_be_reread() {
    tokio::task::unconstrained(async move {
        let add = InstitutionAdd { institution: school(), fields: vec![("cmd".into(), "save".into())] };
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/apply/aca"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/apply/aca"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;
        assert!(matches!(
            portal(&server).add_institution(&add, b"%PDF".to_vec(), "t.pdf").await,
            Err(Error::Http { .. })
        ));
    })
    .await
}

#[tokio::test]
async fn school_details_skip_a_blank_school_and_a_failed_popup() {
    tokio::task::unconstrained(async move {
        let server = MockServer::start().await;
        let p = portal(&server);
        let mut plan = Plan::default();
        p.plan_school_details(&mut plan, &[], &[blank_school()]).await.expect("nothing to plan");
        assert!(plan.findings.is_empty());

        let info = vec![page::page("frm?info", "<h1>Additional Academic Info</h1>")];
        get(&server, "frm", "<tr><a data-href=\"/apply/frm?cmd=widget&id=1\">Connecticut College</a></tr>").await;
        Mock::given(method("GET"))
            .and(path("/apply/frm"))
            .and(query_param("cmd", "widget"))
            .respond_with(ResponseTemplate::new(500))
            .with_priority(1)
            .mount(&server)
            .await;
        assert!(matches!(
            p.plan_school_details(&mut plan, &info, &[blank_school(), school()]).await,
            Err(Error::Http { .. })
        ));
    })
    .await
}

#[tokio::test]
async fn pages_keep_a_title_and_skip_a_page_without_addresses() {
    tokio::task::unconstrained(async move {
    let server = MockServer::start().await;
    let review = menu("<a href=\"frm?titled\" onclick=\"return apply_navigate(this);\">Basic</a>");
    get(&server, "review", &review).await;
    get(
        &server,
        "frm",
        "<h1>Basic</h1><div class=\"form_question\" data-id=\"q\" data-type=\"text\"><input name=\"form_q\" value=\"a\"></div>",
    )
    .await;
    let pages = portal(&server).pages().await.expect("pages");
    assert_eq!(pages[0].title, "Basic");
    assert!(pages[0].addresses.is_none());
})
    .await
}

#[tokio::test]
async fn review_without_a_menu_is_unreadable() {
    tokio::task::unconstrained(async move {
        let server = MockServer::start().await;
        get(&server, "review", "<div id=\"menu\"><ul></ul></div><h1>Review</h1>").await;
        let err = portal(&server).review().await.expect_err("menu");
        assert!(err.to_string().contains("no application menu"), "{err}");
    })
    .await
}

#[tokio::test]
async fn status_without_a_checklist_or_an_update_is_explicit() {
    tokio::task::unconstrained(async move {
    let server = MockServer::start().await;
    get(&server, "status", "nope").await;
    let err = match portal(&server).status().await {
        Err(e) => e,
        Ok(_) => panic!("a page with no checklist was accepted"),
    };
    assert!(err.to_string().contains("no checklist"), "{err}");
    let server = MockServer::start().await;
    get(
        &server,
        "status",
        "<div id=\"part_checklist_App\"><h3>Application</h3><table><tr class=\"plain\"><td></td><td>Received</td><td>Essay</td><td></td></tr></table></div>",
    )
    .await;
    let status = portal(&server).status().await.expect("status");
    assert!(status.update.is_none());
})
    .await
}

#[tokio::test]
async fn adding_a_job_that_does_not_show_up_is_an_error() {
    tokio::task::unconstrained(async move {
        let add = jobs::plan(&[], &[job()]).0.into_iter().next().expect("add");
        let same = job_page("<tr><td>Clinic</td><td>01/2019–Present</td></tr>");
        let server = MockServer::start().await;
        get(&server, "job", &same).await;
        Mock::given(method("POST")).respond_with(ResponseTemplate::new(200)).mount(&server).await;
        let err = portal(&server).add_job(&add).await.expect_err("unchanged");
        assert!(err.to_string().contains("doesn't show it"), "{err}");
    })
    .await
}

#[tokio::test]
async fn coverage_reads_a_signature_a_form_and_both_lists() {
    tokio::task::unconstrained(async move {
    let server = MockServer::start().await;
    let review = menu(
        "<a href=\"certify\" onclick=\"return apply_navigate(this);\">Signature</a><a href=\"frm?x\" onclick=\"return apply_navigate(this);\">Form</a><a href=\"aca\" onclick=\"return apply_navigate(this);\">Academic</a><a href=\"job\" onclick=\"return apply_navigate(this);\">Jobs</a><a href=\"mystery\" onclick=\"return apply_navigate(this);\">Other</a><a href=\"review\" onclick=\"return apply_navigate(this);\">Review</a>",
    );
    get(&server, "review", &review).await;
    get(
        &server,
        "frm",
        "<h1>Form</h1><div class=\"form_question\" data-id=\"q\" data-type=\"text\"><input name=\"form_q\"></div>",
    )
    .await;
    get(&server, "aca", "<div id=\"part_table\"><table><tbody><tr><td>College</td></tr></tbody></table></div>").await;
    get(&server, "job", &job_page("<tr><td>Clinic</td><td>01/2020–Present</td></tr>")).await;
    get(&server, "mystery", "not a list").await;
    let file = admit_slate::AnswersFile { institutions: vec![school()], jobs: vec![job()], ..Default::default() };
    let lines = portal(&server).coverage(&file).await.expect("coverage");
    let text = lines.iter().map(|f| f.to_string()).collect::<Vec<_>>().join("\n");
    assert!(text.contains("Form"), "{text}");
    assert!(text.contains("Academic") || text.contains("1 row") || text.contains("row"), "{text}");
})
    .await
}

#[test]
fn malformed_markup_is_skipped_on_the_library_the_portals_use() {
    use admit_slate::page::{
        address_widget, institutions_listed, jobs_listed, nav, page, review, update_pdf, update_posted, widget_rows,
    };
    assert!(
        address_widget(
            "<a class=\"widget_add\" data-href=\"/apply/frm?add\"><tr class=\"widget_table_row\"><td>Mailing</td></tr>"
        )
        .expect("widget")
        .rows
        .is_empty()
    );
    assert_eq!(nav("<div id=\"menu\"><a href=\"frm?ok\">Ok</a><a href=\"frm?bad\"").len(), 1);
    assert_eq!(
        page("frm?x", "<div class=\"form_question\" data-id=\"ok\" data-type=\"text\"><input name=\"form_ok\" value=\"a\"></div><div class=\"form_question\"").questions.len(),
        1
    );
    assert!(page("frm?m", "<div class=\"form_question\" data-type=\"text\"></div>").questions.is_empty());
    let labeled = page(
        "frm?l",
        "<div class=\"form_question\" data-id=\"q\" data-type=\"text\"><label class=\"form_label\"<input name=\"form_q\"",
    );
    assert_eq!(labeled.questions[0].label, "");
    let words = page(
        "frm?w",
        "<div class=\"form_question\" data-id=\"t\" data-type=\"textarea\"><textarea name=\"form_t\" data-maxlength=\"chars:10\"></textarea></div>",
    );
    assert_eq!(words.questions[0].max_words, None);
    assert!(widget_rows("<tr data-href=\"/apply/frm?cmd=widget&id=1\"").is_empty());
    assert!(widget_rows("<tr <a data-href=\"/apply/frm?cmd=widget&id=1\"").is_empty());
    assert!(widget_rows("<tr><br> data-href=\"/apply/frm?cmd=widget&id=1\"").is_empty());
    assert_eq!(jobs_listed("not the job page"), None);
    assert_eq!(institutions_listed("<h1>Academic History</h1>"), None);
    assert_eq!(
        institutions_listed("<h1>Academic History</h1><div id=\"part_table\"><table></table></div>"),
        Some(vec![])
    );
    assert_eq!(update_posted("was last posted June 2, 2026"), None);
    assert_eq!(update_pdf("no such link"), None);
    assert_eq!(update_pdf("id=\"pager_download\""), None);
    assert_eq!(update_pdf("<id=\"pager_download\""), None);
    assert_eq!(update_pdf("<a id=\"pager_download\""), None);
    assert_eq!(update_pdf("<a id=\"pager_download\">"), None);
    let issues = review("<h1>Review</h1><p class=\"error\"></p><tbody><tr><td>Only</td></tr></tbody>").expect("review");
    assert!(issues.is_empty());
}

/// A question tag whose quoted attribute holds the next question's marker
/// (fuzz, 2026-10-07: panicked slicing that question's body). The marker
/// is not a question, and the first question reads its body. An
/// integration test so the `admit` build of `page` covers it too.
#[test]
fn a_question_marker_inside_a_quoted_attribute_is_not_a_question() {
    let nested = page::page(
        "frm?n",
        "<div class=\"form_question\" data-id=\"a\" data-type=\"text\" data-x='<div class=\"form_question\" data-id=\"b\" data-type=\"text\">'><input name=\"form_a\" value=\"kept\"></div>",
    );
    assert_eq!(nested.questions.iter().map(|q| q.id.as_str()).collect::<Vec<_>>(), ["a"]);
    assert_eq!(answer::current(&nested.questions[0]), Some(Answer::One("kept".into())));
}
