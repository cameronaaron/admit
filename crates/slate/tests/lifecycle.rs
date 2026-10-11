use admit_slate::institutions::{self, Institution};
use admit_slate::jobs::{self, Job};
use admit_slate::{Answer, Client, Plan, Portal, answer, page};
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const ACA: &str = include_str!("fixtures/aca.html");
const ADD: &str = include_str!("fixtures/aca_add.html");
const JOB: &str = include_str!("fixtures/job.html");
fn portal(s: &MockServer) -> Portal {
    Portal { name: "test".into(), client: Client::new(&s.uri(), "_uid=x".into()) }
}
async fn get(s: &MockServer, p: &str, body: &str) {
    static NEXT: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);
    Mock::given(method("GET"))
        .and(path(format!("/apply/{p}")))
        .respond_with(ResponseTemplate::new(200).set_body_string(body))
        .with_priority(200 - NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed))
        .mount(s)
        .await;
}
fn institution() -> Institution {
    Institution {
        name: "Connecticut College".into(),
        profile: None,
        key: Some("003284".into()),
        city: "New London".into(),
        region: "CT".into(),
        country: "US".into(),
        from: "2017-08".into(),
        to: "2021-05".into(),
        level: "U".into(),
        degree: Some("Bachelor of Arts".into()),
        conferred: Some("2021-05".into()),
        transcript: "transcript.pdf".into(),
        major: Some("Psychology".into()),
        gpa: Some("3.28".into()),
        gpa_scale: Some("4.0".into()),
    }
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

#[tokio::test]
async fn list_saves_verify_the_new_record_exists() {
    tokio::task::unconstrained(async move {
    let s = MockServer::start().await;
    let p = portal(&s);
    get(&s, "aca", ACA).await;
    Mock::given(method("GET"))
        .and(path("/apply/aca"))
        .and(query_param("id", ""))
        .respond_with(ResponseTemplate::new(200).set_body_string(ADD))
        .with_priority(1)
        .mount(&s)
        .await;
    get(&s, "job", JOB).await;
    Mock::given(method("POST")).respond_with(ResponseTemplate::new(200)).mount(&s).await;
    let mut plan = Plan::default();
    p.plan_institutions(&mut plan, &[institution()]).await.expect("plan schools");
    p.plan_jobs(&mut plan, &[job()]).await.expect("plan jobs");
    assert_eq!(plan.institutions.len(), 1);
    assert_eq!(plan.jobs.len(), 1);
    let err = p
        .add_institution(&plan.institutions[0], b"%PDF".to_vec(), "transcript.pdf")
        .await
        .expect_err("missing after save");
    assert!(err.to_string().contains("doesn't list it"));
    let err = p.add_job(&plan.jobs[0]).await.expect_err("missing after save");
    assert!(err.to_string().contains("doesn't show it"));
    // The same form returns an added row after the save.
    get(&s,"aca","<h1>Academic History</h1><div id=\"part_table\"><table><tbody><tr><td>Connecticut College</td></tr></tbody></table></div>").await;
    get(&s,"job","<h1>Professional Experience</h1><div id=\"part_table\"><table><tbody><tr><td>Clinic</td><td>01/2020–Present</td></tr></tbody></table></div>").await;
    // Newer mocks take precedence: institution can now be verified.
    p.add_institution(&plan.institutions[0], b"%PDF".to_vec(), "transcript.pdf").await.expect("added school");
    let requests = s.received_requests().await.expect("requests");
    assert!(requests.iter().any(|r| String::from_utf8_lossy(&r.body).contains("name=\"file1\"")));
})
    .await
}

#[tokio::test]
async fn malformed_pages_and_unreadable_lists_never_look_complete() {
    tokio::task::unconstrained(async move {
    let s = MockServer::start().await;
    let p = portal(&s);
    get(&s, "review", "<h1>Review</h1>").await;
    assert!(p.review().await.expect_err("menu").to_string().contains("no application menu"));
    get(&s, "review", "Wrong page").await;
    assert!(p.review().await.expect_err("review").to_string().contains("not the Review"));
    get(&s, "status", "Wrong page").await;
    match p.status().await {
        Err(e) => assert!(e.to_string().contains("no checklist")),
        Ok(_) => panic!("wrong page accepted"),
    }
    get(&s, "status", "<div id=\"part_checklist_Application\"><h3>Application</h3><table></table></div>").await;
    assert!(p.status().await.expect("empty checklist").update.is_none());
    get(&s, "aca", "Wrong page").await;
    assert!(p.institutions().await.expect_err("wrong page").to_string().contains("not the Academic"));
    get(&s, "aca", ACA).await;
    assert!(p.institutions().await.expect_err("no degrees").to_string().contains("no degree choices"));
    get(&s, "job", "Wrong page").await;
    assert!(
        p.plan_jobs(&mut Plan::default(), &[job()])
            .await
            .expect_err("wrong job page")
            .to_string()
            .contains("not the Professional")
    );
    p.plan_jobs(&mut Plan::default(), &[]).await.expect("empty");
    p.plan_institutions(&mut Plan::default(), &[]).await.expect("empty");
    let review = "<div id=\"menu\"><a href='frm?blank' onclick='return apply_navigate(this);'>Untitled form</a><a href='certify' onclick='return apply_navigate(this);'>Signature</a><a href='tes' onclick='return apply_navigate(this);'>Tests</a><a href='unknown' onclick='return apply_navigate(this);'>Unknown</a><a href='review' onclick='return apply_navigate(this);'>Review</a></div><h1>Review</h1>";
    get(&s, "review", review).await;
    get(&s, "frm", "").await;
    get(&s, "tes", "<table></table>").await;
    get(&s, "unknown", "unknown").await;
    let pages = p.pages().await.expect("pages");
    assert_eq!(pages[0].title, "Untitled form");
    let findings = p.coverage(&Default::default()).await.expect("coverage");
    assert!(findings.iter().any(|f| f.to_string().contains("UNCHECKED")));
    assert!(findings.iter().any(|f| f.to_string().contains("signed")));
})
    .await
}

#[tokio::test]
async fn school_details_are_planned_as_whole_widget_pages() {
    tokio::task::unconstrained(async move {
    let s = MockServer::start().await;
    let p = portal(&s);
    let mut plan = Plan::default();
    let i = institution();
    let mut blank = i.clone();
    blank.from.clear();
    blank.to.clear();
    blank.major = None;
    blank.gpa = None;
    blank.gpa_scale = None;
    p.plan_school_details(&mut plan, &[], std::slice::from_ref(&blank)).await.expect("blank school");
    p.plan_school_details(&mut plan, &[], std::slice::from_ref(&i)).await.expect("missing page");
    assert!(plan.findings[0].to_string().contains("no Additional"));
    let pages = vec![page::page("frm?additional", "<h1>Additional Academic Info</h1>")];
    get(&s, "frm", include_str!("fixtures/addl_info.html")).await;
    Mock::given(method("GET")).and(path("/apply/frm")).and(query_param("cmd","widget"))
        .respond_with(ResponseTemplate::new(200).set_body_string("<div class=\"form_question\" data-id='major' data-type='text' data-export='sys:school:major'><input name='form_major' value='History'></div>")).with_priority(1).mount(&s).await;
    let mut missing = i.clone();
    missing.name = "Missing".into();
    let mut empty = i.clone();
    empty.major = None;
    empty.gpa = None;
    empty.gpa_scale = None;
    empty.degree = None; // date details still exist
    p.plan_school_details(&mut plan, &pages, &[blank, i, missing, empty]).await.expect("details");
    assert!(!plan.changes.is_empty());
    assert!(!plan.school_pages.is_empty());
    assert!(plan.findings.iter().any(|f| f.to_string().contains("not on Additional")));
    Mock::given(method("POST")).respond_with(ResponseTemplate::new(200)).mount(&s).await;
    p.apply(&plan, &[]).await.expect("widget save");
    let posts: Vec<_> =
        s.received_requests().await.expect("requests").into_iter().filter(|r| r.method.as_str() == "POST").collect();
    assert!(String::from_utf8_lossy(&posts[0].body).contains("name=\"cmd\"\r\n\r\nsave"));
})
    .await
}

#[tokio::test]
async fn client_errors_preserve_status_and_request_context() {
    tokio::task::unconstrained(async move {
        use admit_slate::Error;
        let s = MockServer::start().await;
        for (p, status, location) in
            [("bad", 500, ""), ("redirect", 302, "/elsewhere"), ("expired", 302, "/account/login")]
        {
            Mock::given(path(format!("/apply/{p}")))
                .respond_with(
                    ResponseTemplate::new(status).insert_header("location", location).set_body_string("failure"),
                )
                .mount(&s)
                .await;
        }
        let c = Client::new(&s.uri(), "_uid=x".into());
        for p in ["bad", "redirect"] {
            let e = c.get(p).await.expect_err("http");
            assert!(matches!(e, Error::Http { .. }));
            assert!(e.to_string().contains(p));
            assert!(matches!(c.get_bytes(p).await, Err(Error::Http { .. })));
        }
        assert!(matches!(c.get("expired").await, Err(Error::Expired { .. })));
        assert!(matches!(c.get_bytes("expired").await, Err(Error::Expired { .. })));
        Mock::given(method("GET"))
            .and(path("/apply/file"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"%PDF"))
            .mount(&s)
            .await;
        assert_eq!(c.get_bytes("file").await.expect("pdf"), b"%PDF");
        Mock::given(method("GET"))
            .and(path("/manage/service/lookup"))
            .respond_with(ResponseTemplate::new(200).set_body_string("{}"))
            .mount(&s)
            .await;
        assert_eq!(c.lookup("o1", "Yale").await.expect("lookup"), "{}");
        let c = Client::new("invalid URL", "".into());
        for e in [
            c.get("x").await.expect_err("get"),
            c.get_bytes("x").await.expect_err("bytes"),
            c.lookup("o1", "q").await.expect_err("lookup"),
            c.save("x", &[], &[]).await.expect_err("save"),
            c.save_form("x", &[]).await.expect_err("form"),
            c.save_file("x", &[], "f", "x.pdf", vec![]).await.expect_err("file"),
        ] {
            assert!(matches!(e, Error::Transport { .. }));
            assert!(e.to_string().contains("transport"));
        }
        assert!(matches!(Client::login("invalid URL", "u", "p").await, Err(Error::Transport { .. })));
        let mut c = Client::new("invalid URL", "".into());
        assert!(matches!(c.open("app").await, Err(Error::Transport { .. })));
        let mut c = Client::new(&s.uri(), "_uid=x".into());
        Mock::given(method("POST")).and(path("/apply/")).respond_with(ResponseTemplate::new(200)).mount(&s).await;
        assert!(c.open("app").await.expect_err("no cookie").to_string().contains("did not switch"));
        Mock::given(method("GET")).and(path("/account/login")).respond_with(ResponseTemplate::new(200)).mount(&s).await;
        Mock::given(method("POST"))
            .and(path("/account/login"))
            .respond_with(ResponseTemplate::new(302))
            .mount(&s)
            .await;
        assert!(matches!(Client::login(&s.uri(), "u", "p").await, Err(Error::BadLogin { status: 302, .. })));
    })
    .await
}

#[test]
fn unknown_apply_pages_and_absent_collateral_are_explicit() {
    assert!(admit_slate::collateral(None, &page::page("x", ""), &Default::default()).is_empty());
    let before = vec![page::page(
        "frm?x",
        "<div class=\"form_question\" data-id='q' data-type='text' data-export='q'><input name='form_q' value='old'></div>",
    )];
    let want = std::collections::BTreeMap::from([("q".to_string(), Answer::One("new".into()))]).into();
    let plan = answer::plan(&before, &want);
    assert_eq!(plan.count(), 1);
    assert!(plan.render().contains("new"));
    let d = page::degree_choices(ADD);
    assert_eq!(institutions::plan(&[], &d, &[institution()]).0.len(), 1);
    assert_eq!(jobs::plan(&[], &[job()]).0.len(), 1);
}

#[test]
fn plans_render_list_additions_and_profile_coverage_checks_aliases() {
    let mut plan = Plan::default();
    plan.institutions = institutions::plan(&[], &page::degree_choices(ADD), &[institution()]).0;
    plan.jobs = jobs::plan(&[], &[job()]).0;
    let page = page::page(
        "frm?x",
        "<h1>Biographical</h1><div class=\"form_question\" data-id=\"q\" data-type=\"text\" data-export=\"q\"><input name=\"form_q\" value=\"a\"></div>",
    );
    let want =
        admit_slate::AnswersFile { answers: [("q".into(), Answer::One("b".into()))].into(), ..Default::default() };
    plan.changes = answer::plan(std::slice::from_ref(&page), &want).changes;
    plan.findings.push(admit_domain::Finding::Note("see the note".into()));
    let text = plan.render();
    assert!(text.contains("Biographical"), "{text}");
    assert!(text.contains("see the note"), "{text}");
    assert!(text.contains("Professional Experience (job)"));
    assert!(text.contains("Academic History (aca)"));
    let x = admit_domain::Experience {
        kind: admit_domain::ExperienceKind::Employment,
        organization: "Clinic".into(),
        title: "Patient Care".into(),
        state: "California".into(),
        country: "United States".into(),
        start: "2020-01-01".into(),
        end: None,
        current: true,
        status: "Part time".into(),
        hours_per_week: 10,
        weeks: 4,
        total_hours: 40,
        compensated: true,
        volunteer: false,
        description: "Care".into(),
        supervisor: Default::default(),
        ids: Default::default(),
    };
    let profile = admit_domain::Profile {
        experiences: admit_domain::ExperiencesFile { experiences: vec![x] },
        education: admit_domain::EducationFile {
            colleges: vec![admit_domain::College { name: "School".into(), degrees: vec![] }],
        },
        ..Default::default()
    };
    let mut file = admit_slate::AnswersFile::default();
    assert_eq!(admit_slate::coverage::profile_gaps(&profile, &file).len(), 2);
    let mut j = job();
    j.profile_title = Some("Patient Care".into());
    file.jobs.push(j);
    let mut i = institution();
    i.profile = Some("School".into());
    file.institutions.push(i);
    assert!(admit_slate::coverage::profile_gaps(&profile, &file).is_empty());
    let p = page::page(
        "frm?x",
        "<div class=\"form_question\" data-id='required' data-type='text' data-required='1'><input name='form_required'></div><div class=\"form_question\" data-id='file' data-type='plugin:material'><input type='file' name='material_file'></div><div class=\"form_question\" data-id='widget' data-type='plugin:widget'></div>",
    );
    let lines = admit_slate::coverage::report(
        &[admit_slate::coverage::Entry {
            title: "Questions".into(),
            href: "frm?x".into(),
            section: admit_slate::coverage::Section::Questions(p),
        }],
        &[],
        &Default::default(),
    );
    let text = lines[0].to_string();
    for key in ["upload slot", "widget(s)", "required but not flagged"] {
        assert!(text.contains(key), "{text}");
    }
    let mut long = job();
    long.description = "é".repeat(1001);
    assert!(jobs::plan(&[], &[long]).1[0].is_violation());
}

#[tokio::test]
async fn failed_reads_and_saves_stop_the_plan() {
    tokio::task::unconstrained(async move {
        use admit_slate::Error;
        let s = MockServer::start().await;
        let p = portal(&s);
        Mock::given(method("GET")).respond_with(ResponseTemplate::new(500)).mount(&s).await;
        assert!(matches!(p.institutions().await, Err(Error::Http { .. })));
        assert!(matches!(p.plan_institutions(&mut Plan::default(), &[institution()]).await, Err(Error::Http { .. })));
        assert!(matches!(p.plan_jobs(&mut Plan::default(), &[job()]).await, Err(Error::Http { .. })));
        let info = vec![page::page("frm?additional", "<h1>Additional Academic Info</h1>")];
        assert!(matches!(
            p.plan_school_details(&mut Plan::default(), &info, &[institution()]).await,
            Err(Error::Http { .. })
        ));
        let add =
            institutions::plan(&[], &page::degree_choices(ADD), &[institution()]).0.into_iter().next().expect("add");
        assert!(matches!(p.add_institution(&add, b"%PDF".to_vec(), "t.pdf").await, Err(Error::Http { .. })));

        let s = MockServer::start().await;
        let p = portal(&s);
        get(&s, "aca", "<h1>Academic History</h1><div id=\"part_table\"><table><tbody></tbody></table></div>").await;
        Mock::given(method("GET"))
            .and(path("/apply/aca"))
            .and(query_param("id", ""))
            .respond_with(ResponseTemplate::new(500))
            .with_priority(1)
            .mount(&s)
            .await;
        assert!(matches!(p.institutions().await, Err(Error::Http { .. })));
        Mock::given(method("POST")).respond_with(ResponseTemplate::new(200)).mount(&s).await;
        assert!(matches!(p.add_institution(&add, b"%PDF".to_vec(), "t.pdf").await, Err(Error::Http { .. })));

        let s = MockServer::start().await;
        let p = portal(&s);
        Mock::given(method("GET"))
            .and(path("/apply/frm"))
            .and(query_param("cmd", "widget"))
            .respond_with(ResponseTemplate::new(500))
            .with_priority(1)
            .mount(&s)
            .await;
        let info_html = include_str!("fixtures/addl_info.html");
        // The info page itself is served by the frm mock above only when cmd is absent.
        // Replace it with the captured page so the school row is found, then the popup 500s.
        let _ = info_html;
        Mock::given(method("GET"))
            .and(path("/apply/frm"))
            .respond_with(ResponseTemplate::new(200).set_body_string(info_html))
            .with_priority(2)
            .mount(&s)
            .await;
        assert!(matches!(
            p.plan_school_details(&mut Plan::default(), &info, &[institution()]).await,
            Err(Error::Http { .. })
        ));
    })
    .await
}

#[tokio::test]
async fn apply_reports_each_way_a_save_can_stop() {
    tokio::task::unconstrained(async move {
    use admit_slate::{AnswersFile, Error};
    let question = "<div class=\"form_question\" data-id=\"q\" data-type=\"text\" data-export=\"q\"><input name=\"form_q\" value=\"a\"></div>";
    let other = "<div class=\"form_question\" data-id=\"r\" data-type=\"text\" data-export=\"r\"><input name=\"form_r\" value=\"keep\"></div>";
    let page = page::page("frm?x", &format!("{question}{other}"));
    let want = AnswersFile { answers: [("q".into(), Answer::One("b".into()))].into(), ..Default::default() };
    let plan = answer::plan(std::slice::from_ref(&page), &want);
    let s = MockServer::start().await;
    let p = portal(&s);
    assert!(p.apply(&plan, &[]).await.expect_err("missing").to_string().contains("not among"));
    let bad = page::page(
        "frm?x",
        "<div class=\"form_question\" data-id=\"q\" data-type=\"text\" data-export=\"q\"><input name=\"form_q\" value=\"a\"></div><div class=\"form_question\" data-id=\"f\" data-type=\"text\" data-readonly=\"1\"><input name=\"form_f\" data-formula=\"1 +\" value=\"1\"></div>",
    );
    let bad_plan = answer::plan(std::slice::from_ref(&bad), &want);
    assert!(p.apply(&bad_plan, &[bad]).await.expect_err("formula").to_string().contains("does not parse"));

    Mock::given(method("POST")).and(path("/apply/frm")).respond_with(ResponseTemplate::new(500)).mount(&s).await;
    assert!(matches!(p.apply(&plan, std::slice::from_ref(&page)).await, Err(Error::Http { .. })));

    let s = MockServer::start().await;
    let p = portal(&s);
    Mock::given(method("POST")).respond_with(ResponseTemplate::new(200)).mount(&s).await;
    Mock::given(method("GET")).respond_with(ResponseTemplate::new(500)).mount(&s).await;
    assert!(matches!(p.apply(&plan, std::slice::from_ref(&page)).await, Err(Error::Http { .. })));

    let s = MockServer::start().await;
    let p = portal(&s);
    let moved =
        format!("{question}{other}").replace("value=\"keep\"", "value=\"gone\"").replace("value=\"a\"", "value=\"b\"");
    Mock::given(method("POST")).respond_with(ResponseTemplate::new(200)).mount(&s).await;
    Mock::given(method("GET")).respond_with(ResponseTemplate::new(200).set_body_string(moved)).mount(&s).await;
    assert!(
        p.apply(&plan, std::slice::from_ref(&page))
            .await
            .expect_err("collateral")
            .to_string()
            .contains("changed answers")
    );

    let s = MockServer::start().await;
    let p = portal(&s);
    let saved = format!("{question}{other}").replace("value=\"a\"", "value=\"b\"");
    Mock::given(method("POST")).respond_with(ResponseTemplate::new(200)).mount(&s).await;
    Mock::given(method("GET")).respond_with(ResponseTemplate::new(200).set_body_string(saved)).mount(&s).await;
    p.apply(&plan, &[page]).await.expect("saved");

    let before = {
        let mut b = page::page(
            "frm?basic",
            "<a class=\"widget_add\" data-href=\"/apply/frm?cmd=widget&form=1\">Insert</a><div class=\"form_question\" data-id=\"n\" data-type=\"text\" data-export=\"n\"><input name=\"form_n\" value=\"Ada\"></div>",
        );
        b.addresses.as_mut().expect("widget").form = page::page(
            "w",
            "<div class=\"form_question\" data-id=\"ty\" data-type=\"select\" data-export=\"sys:address:type\"><select name=\"form_ty\"><option value=\"mailing\">Mailing</option></select></div><div class=\"form_question\" data-id=\"b\" data-type=\"text\" data-export=\"sys:address_block\"><input name=\"form_b\"></div>",
        )
        .questions;
        b
    };
    let file = AnswersFile {
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
    let s = MockServer::start().await;
    Mock::given(method("POST")).respond_with(ResponseTemplate::new(500)).mount(&s).await;
    assert!(matches!(portal(&s).apply(&plan, std::slice::from_ref(&before)).await, Err(Error::Http { .. })));
    let s = MockServer::start().await;
    Mock::given(method("POST")).respond_with(ResponseTemplate::new(200)).mount(&s).await;
    Mock::given(method("GET")).respond_with(ResponseTemplate::new(500)).mount(&s).await;
    assert!(matches!(portal(&s).apply(&plan, std::slice::from_ref(&before)).await, Err(Error::Http { .. })));
    let listed = "<a class=\"widget_add\" data-href=\"/apply/frm?cmd=widget&form=1\">Insert</a><tr class=\"widget_table_row\"><td>Mailing</td><td>1 St</td><td>Town</td><td>CA</td><td>United States</td><td>00000</td></tr><div class=\"form_question\" data-id=\"n\" data-type=\"text\" data-export=\"n\"><input name=\"form_n\" value=\"Ada\"></div>";
    let s = MockServer::start().await;
    Mock::given(method("POST")).respond_with(ResponseTemplate::new(200)).mount(&s).await;
    Mock::given(method("GET")).respond_with(ResponseTemplate::new(200).set_body_string("no row")).mount(&s).await;
    assert!(
        portal(&s)
            .apply(&plan, std::slice::from_ref(&before))
            .await
            .expect_err("unlisted")
            .to_string()
            .contains("doesn't list")
    );
    let s = MockServer::start().await;
    Mock::given(method("POST")).respond_with(ResponseTemplate::new(200)).mount(&s).await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_string(listed.replace("value=\"Ada\"", "value=\"moved\"")))
        .mount(&s)
        .await;
    assert!(
        portal(&s)
            .apply(&plan, std::slice::from_ref(&before))
            .await
            .expect_err("address collateral")
            .to_string()
            .contains("changed")
    );
    let s = MockServer::start().await;
    Mock::given(method("POST")).respond_with(ResponseTemplate::new(200)).mount(&s).await;
    Mock::given(method("GET")).respond_with(ResponseTemplate::new(200).set_body_string(listed)).mount(&s).await;
    portal(&s).apply(&plan, &[before]).await.expect("address saved");
})
    .await
}

#[path = "../../liaison/tests/support/broken.rs"]
mod broken;
#[tokio::test]
async fn truncated_html_and_pdf_bodies_are_transport_failures() {
    tokio::task::unconstrained(async move {
        use admit_slate::{Client, Error};
        let url = broken::responses(vec![broken::TRUNCATED]);
        assert!(matches!(Client::new(&url, "_uid=test".into()).get("review").await, Err(Error::Transport { .. })));
        let url = broken::responses(vec![broken::TRUNCATED]);
        assert!(matches!(
            Client::new(&url, "_uid=test".into()).get_bytes("update").await,
            Err(Error::Transport { .. })
        ));
        let url = broken::responses(vec![broken::EMPTY, broken::TRUNCATED]);
        assert!(matches!(Client::login(&url, "user", "password").await, Err(Error::Transport { .. })));
    })
    .await
}
