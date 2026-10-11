mod support;
use admit_domain::*;
use admit_liaison::{Client, Plan, Portal, Section};
use serde_json::json;
use std::collections::BTreeSet;
use std::path::Path;

#[tokio::test]
async fn pull_plan_and_apply_preserve_ids_and_unowned_records() {
    tokio::task::unconstrained(async move {
        let server = support::portal().await;
        let portal = Portal { name: "test".into(), client: Client::new(&server.uri(), "SESSION=test".into()) };
        let lk = portal.lookups().await.expect("lookups");
        let mut p = portal.pull(&lk).await.expect("pull");
        assert_eq!(p.achievements.achievements[0].ids["test"], 1);
        assert_eq!(p.licenses.licenses[0].ids["test"], 3);
        assert_eq!(p.experiences.experiences[0].ids["test"], 5);
        assert_eq!(p.high_school.high_schools[0].from, "2013-09");
        let only = BTreeSet::new();
        let original = portal.plan(&p, Path::new("."), &only, &lk).await.expect("plan");
        assert_eq!(original.change_count(), 0, "{}", original.render());
        for s in Section::ALL {
            assert_eq!(Section::parse(s.name()), Some(s));
        }
        assert_eq!(Section::parse("unknown"), None);
        assert!(original.render().contains("achievements: 0 change(s)"));
        p.achievements.achievements[0].description = Some("Changed".into());
        let mut a = p.achievements.achievements[1].clone();
        a.name = "New honor".into();
        a.ids.clear();
        p.achievements.achievements.push(a);
        p.licenses.licenses[0].organization = "New board".into();
        let mut l = p.licenses.licenses[1].clone();
        l.title = "New registration".into();
        l.ids.clear();
        p.licenses.licenses.push(l);
        p.experiences.experiences[0].description = "Changed work".into();
        let mut x = p.experiences.experiences[2].clone();
        x.title = "New role".into();
        x.ids.clear();
        p.experiences.experiences.push(x);
        p.education.colleges[0].degrees[0].year = 2026;
        let mut d = p.education.colleges[0].degrees[1].clone();
        d.major = "Music".into();
        d.note = Some("Not sent".into());
        p.education.colleges[0].degrees.push(d);
        p.education.colleges.push(College { name: "Missing school".into(), degrees: vec![] });
        p.high_school.high_schools[0].to = "2017-06".into();
        p.high_school.high_schools[0].diploma_date = "2017-06".into();
        let mut hs = p.high_school.high_schools[0].clone();
        hs.name = "Missing HS".into();
        p.high_school.high_schools.push(hs);
        let mut plan = portal.plan(&p, Path::new("."), &only, &lk).await.expect("plan");
        plan.experiences.as_mut().expect("experiences").extras.push("Kept role".into());
        assert_eq!(plan.change_count(), 9, "{}", plan.render());
        let text = plan.render();
        assert!(text.contains("Kept role") && text.contains("left alone"), "{text}");
        for expected in
            ["New honor", "New registration", "New role", "year", "high school", "Missing school", "Missing HS"]
        {
            assert!(text.contains(expected), "{expected}: {text}");
        }
        portal.apply(&plan, &lk).await.expect("apply");
        let requests = server.received_requests().await.expect("requests");
        assert!(!requests.iter().any(|r| r.method.as_str() == "DELETE"));
        let college = requests
            .iter()
            .find(|r| r.method.as_str() == "PUT" && r.url.path().ends_with("/colleges/8"))
            .expect("college write");
        let body: serde_json::Value = serde_json::from_slice(&college.body).expect("body");
        assert_eq!(body["degrees"].as_array().expect("degrees").len(), 3);
        assert_eq!(body["degrees"][0]["id"], 11, "unchanged degree keeps its id");
        let diploma = requests
            .iter()
            .find(|r| r.method.as_str() == "POST" && r.url.path().ends_with("/highSchoolInfo"))
            .expect("diploma write");
        let body: serde_json::Value = serde_json::from_slice(&diploma.body).expect("body");
        assert_eq!(body["questionBlocks"][1]["data"], json!({"keep":true}));
        let scoped =
            portal.plan(&p, Path::new("."), &[Section::Licenses].into_iter().collect(), &lk).await.expect("scoped");
        assert_eq!(scoped.change_count(), 2);
        assert!(scoped.achievements.is_none());
        assert!(portal.deadlines().await.expect("deadlines").is_empty());
        let f = portal.audit(&p, time::macros::date!(2026 - 10 - 06)).await.expect("audit");
        assert!(f.iter().any(|f| f.to_string().contains("recommendation")), "{f:?}");
        assert_eq!(Plan::default().render(), "");
    })
    .await
}

#[tokio::test]
async fn lookup_errors_are_explicit_and_do_not_write() {
    tokio::task::unconstrained(async move {
        let server = support::portal().await;
        let portal = Portal { name: "test".into(), client: Client::new(&server.uri(), "SESSION=x".into()) };
        let lk = portal.lookups().await.expect("lookups");
        assert!(lk.state_id("Canada", "Ontario").expect_err("unsupported").to_string().contains("only United States"));
        assert!(lk.state_id("Unknown", "California").is_err());
        assert!(lk.state_id("United States", "Unknown").is_err());
        assert_eq!(lk.countries.text(-1), "");
        let p = portal.pull(&lk).await.expect("pull");
        let mut a = p.achievements.achievements[0].clone();
        a.country = "Canada".into();
        assert!(admit_liaison::achievements::create(&portal.client, &lk, &a).await.is_err());
    })
    .await
}

#[tokio::test]
async fn http_errors_empty_bodies_and_transport_failures_keep_their_context() {
    tokio::task::unconstrained(async move {
        use admit_liaison::ApiError;
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let s = MockServer::start().await;
        for (endpoint, status, body) in [("bad", 500, "server failure"), ("json", 200, "invalid"), ("empty", 204, "")] {
            Mock::given(path(format!("/applicant/api/{endpoint}")))
                .respond_with(ResponseTemplate::new(status).set_body_string(body))
                .mount(&s)
                .await;
        }
        let c = Client::new(&s.uri(), "SESSION=x".into());
        let err = c.get("/bad").await.expect_err("HTTP error");
        assert!(matches!(err, ApiError::Http { status: 500, .. }));
        assert!(err.to_string().contains("GET /bad: HTTP 500: server failure"));
        let err = c.get("/json").await.expect_err("invalid JSON");
        assert!(matches!(err, ApiError::Json { .. }));
        assert!(err.to_string().contains("response is not JSON"));
        assert_eq!(c.get("/empty").await.expect("204"), serde_json::Value::Null);
        assert_eq!(c.delete("/empty").await.expect("delete"), serde_json::Value::Null);
        let c = Client::new("not a URL", "".into());
        for err in [
            c.get("/x").await.expect_err("get"),
            c.post("/x", &json!({})).await.expect_err("post"),
            c.put("/x", &json!({})).await.expect_err("put"),
            c.post_model("/x", &json!({})).await.expect_err("model"),
        ] {
            assert!(matches!(err, ApiError::Transport { .. }));
            assert!(err.to_string().contains("transport"));
        }
        assert!(matches!(Client::login("not a URL", "u", "p").await, Err(ApiError::Transport { .. })));
        Mock::given(method("GET"))
            .and(path("/applicant/initialize"))
            .respond_with(
                ResponseTemplate::new(200)
                    .append_header("set-cookie", "cas=old; path=/")
                    .append_header("set-cookie", "invalid"),
            )
            .mount(&s)
            .await;
        Mock::given(method("POST"))
            .and(path("/applicant/authenticate"))
            .respond_with(
                ResponseTemplate::new(200)
                    .append_header("set-cookie", "cas=new; path=/")
                    .append_header("set-cookie", "formId=; Max-Age=0")
                    .set_body_string("No session"),
            )
            .mount(&s)
            .await;
        let err = Client::login(&s.uri(), "u", "p").await.expect_err("no session cookie");
        assert!(matches!(err, ApiError::BadLogin { status: 200, .. }));
    })
    .await
}

#[tokio::test]
async fn personal_reads_resolve_counties_and_missing_sections_fail_closed() {
    tokio::task::unconstrained(async move {
    let s = support::portal().await;
    let portal = Portal { name: "test".into(), client: Client::new(&s.uri(), "SESSION=x".into()) };
    let lk = portal.lookups().await.expect("lookups");
    // A navigation without the personal-information sections.
    s.reset().await;
    support::get(&s, "/navigation", json!({"sections":[]})).await;
    assert!(
        admit_liaison::personal::read(&portal.client, &lk)
            .await
            .expect_err("missing sections")
            .to_string()
            .contains("biographicInfo")
    );
    s.reset().await;
    support::get(
        &s,
        "/navigation",
        json!({"personalInfo":{"sections":[{"name":"biographicInfo","id":1},{"name":"contactInfo","id":2}]}}),
    )
    .await;
    support::get(&s, "/authenticatedUser", json!({"email":"test@example.com"})).await;
    support::get(
        &s,
        "/applicationFormSubSectionBlocks/subSection/1",
        json!({"questionBlocks":[{"questions":[
            {"shortFieldName":"first_name","value":["Test"]},{"shortFieldName":"last_name","value":["User"]},
            {"shortFieldName":"birth_state","value":["339"]},{"shortFieldName":"birth_county","value":["1"]},
            {"shortFieldName":"pronounsPreferred","value":["99"]},{}
        ]}]}),
    )
    .await;
    support::get(&s, "/lookup/counties", json!([{"id":1,"text":"County"}])).await;
    support::get(&s,"/applicationFormSubSectionBlocks/subSection/2",json!({"questionBlocks":[{"questions":[
        {"shortFieldName":"street_address_1","value":["1 Street"]},{"shortFieldName":"street_address_2","value":["Apt 1"]},
        {"shortFieldName":"pstreet_address_1","value":["2 Street"]}
    ]}]})).await;
    let st = admit_liaison::personal::read(&portal.client, &lk).await.expect("read");
    assert_eq!(st.fields["legal_name"], "Test User");
    assert_eq!(st.fields["birth_county"], "County");
    assert_eq!(st.fields["pronouns"], "99");
    assert_eq!(st.addresses.len(), 2);
    assert_eq!(st.addresses[0].street, "1 Street Apt 1");
    s.reset().await;
    support::get(
        &s,
        "/navigation",
        json!({"personalInfo":{"sections":[{"name":"biographicInfo","id":1},{"name":"contactInfo","id":2}]}}),
    )
    .await;
    support::get(&s, "/authenticatedUser", json!({})).await;
    for id in [1, 2] {
        support::get(&s, &format!("/applicationFormSubSectionBlocks/subSection/{id}"), json!({})).await;
    }
    assert!(admit_liaison::personal::read(&portal.client, &lk).await.expect("empty").fields.is_empty());
})
    .await
}

fn course(code: &str) -> admit_sources::TranscriptCourse {
    admit_sources::TranscriptCourse {
        year: "2026".into(),
        season: "Fall".into(),
        code: code.into(),
        college: "COL".into(),
        title: "Science".into(),
        credits: "3.00".into(),
        grade: "A".into(),
        noncredit: false,
        term_months: Some((9, 12)),
    }
}

#[tokio::test]
async fn coursework_creates_one_term_for_multiple_courses_and_rejects_missing_ids() {
    tokio::task::unconstrained(async move {
        use admit_liaison::coursework::{self, CourseAdd, TermTarget};
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let s = support::portal().await;
        let portal = Portal { name: "test".into(), client: Client::new(&s.uri(), "SESSION=x".into()) };
        let lk = portal.lookups().await.expect("lookups");
        let add = CourseAdd {
            college_id: 8,
            college: "School".into(),
            college_code: "008".into(),
            target: TermTarget::New {
                term: "Fall".into(),
                year: "2026".into(),
                in_progress: false,
                academic_status: "Freshman".into(),
            },
            course: course("SCI 1"),
            subject: "Other Science".into(),
        };
        let second = CourseAdd { course: course("SCI 2"), ..add.clone() };
        let existing = CourseAdd {
            target: TermTarget::Existing { id: 10, label: "Fall 2026".into() },
            course: course("SCI 3"),
            ..add.clone()
        };
        let plan = Plan { courses: vec![add.clone(), second, existing], ..Default::default() };
        assert_eq!(plan.change_count(), 3);
        assert!(plan.render().contains("NEW Fall 2026"));
        portal.apply(&plan, &lk).await.expect("apply courses");
        let reqs = s.received_requests().await.expect("requests");
        assert_eq!(reqs.iter().filter(|r| r.url.path().contains("saveOrUpdateTermAndCourses")).count(), 1);
        let writes: Vec<_> = reqs.iter().filter(|r| r.url.path().contains("/courses/collegeId/")).collect();
        assert_eq!(writes.len(), 3);
        let body: serde_json::Value = serde_json::from_slice(&writes[0].body).expect("body");
        assert_eq!(body["gradeEnabled"], true);
        assert_eq!(body["termId"], "99");
        assert!(body["courseClassification"].as_object().expect("flags").values().all(|v| v == false));
        let failed = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
            .mount(&failed)
            .await;
        let c = Client::new(&failed.uri(), "SESSION=x".into());
        assert!(
            coursework::apply(&c, &lk, &[add]).await.expect_err("no term id").to_string().contains("returned no id")
        );
        Mock::given(method("GET"))
            .and(path("/applicant/api/academicHistory/courseWork"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!([{"id":8,"college":"School","collegeCode":"008"}])),
            )
            .mount(&failed)
            .await;
        Mock::given(method("GET"))
            .and(path("/applicant/api/academicHistory/courseWork/summary/8"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"terms":[{"id":10,"term":"Fall","year":"2026","courses":[]}]})),
            )
            .mount(&failed)
            .await;
        let live = coursework::live(&c).await.expect("live");
        assert_eq!(live.len(), 1);
        assert_eq!(live[0].terms.len(), 1);
        let src = TranscriptSource {
            path: "missing.pdf".into(),
            parser: "unknown".into(),
            colleges: [("COL".into(), "School".into())].into_iter().collect(),
            summer_term: "Summer".into(),
            academic_status: "Freshman".into(),
            skip: vec![],
            subjects: Default::default(),
            fix_titles: vec![],
            create_missing_colleges: false,
        };
        assert!(coursework::read(&src, Path::new(".")).expect_err("unknown parser").contains("missing.pdf"));
        let profile = Profile { coursework: CourseworkFile { transcripts: vec![src] }, ..Default::default() };
        let portal = Portal { name: "test".into(), client: c };
        let plan = portal
            .plan(&profile, Path::new("."), &[Section::Coursework].into_iter().collect(), &lk)
            .await
            .expect("unreadable transcript plan");
        assert!(plan.findings[0].is_violation());
    })
    .await
}

#[tokio::test]
async fn extras_and_all_plan_sections_remain_visible() {
    tokio::task::unconstrained(async move {
        use admit_liaison::{coursework, education, experiences};
        let s = support::portal().await;
        let portal = Portal { name: "test".into(), client: Client::new(&s.uri(), "SESSION=x".into()) };
        let lk = portal.lookups().await.expect("lookups");
        let p = portal.pull(&lk).await.expect("profile");
        let live = education::live(&portal.client).await.expect("colleges");
        let mut desired = p.education.colleges.clone();
        desired[0].degrees.remove(1);
        desired[0].degrees[0].year = 2026;
        let (updates, f) = education::plan(&desired, &live);
        assert!(f.iter().any(|f| f.to_string().contains("kept")));
        let b = education::college_body(&updates[0], &lk).expect("college body");
        assert_eq!(b["degrees"][0]["secondMajorId"], 12);
        assert_eq!(b["degrees"][0]["minorId"], 10);
        let mut x = p.experiences.experiences[1].clone();
        x.title = "Community role".into();
        experiences::create(&portal.client, &lk, &x).await.expect("community create");
        let cc = coursework::CollegeCreate {
            name: "New College".into(),
            first: ("Fall".into(), "2026".into()),
            last: ("Fall".into(), "2026".into()),
            months: (9, 12),
            still_attending: true,
        };
        support::get(&s, "/lookup/colleges", json!([{"id":1,"text":"New College","code":"001"}])).await;
        let mut plan = Plan {
            new_colleges: vec![cc.clone(), coursework::CollegeCreate { still_attending: false, ..cc }],
            attendance: vec![coursework::AttendanceFix {
                college_id: 8,
                college: "School".into(),
                still_attending: true,
                last: ("Fall".into(), "2026".into()),
                end_month: 12,
                was: "Ended".into(),
            }],
            course_edits: vec![coursework::CourseEdit {
                college_id: 8,
                college: "School".into(),
                term_id: 1,
                term: "Fall 2026".into(),
                course_id: 1,
                from: ("SCI".into(), "Old title".into()),
                to: ("SCI 1".into(), "New title".into()),
                subject: Some(("Art".into(), "Other Science".into())),
                body: json!({"code":"SCI 1"}),
            }],
            ..Default::default()
        };
        plan.attendance.push(coursework::AttendanceFix { still_attending: false, ..plan.attendance[0].clone() });
        assert_eq!(plan.change_count(), 5);
        let text = plan.render();
        for expected in ["still attending", "ended", "subject Art -> Other Science", "New title", "New College"] {
            assert!(text.contains(expected), "{text}");
        }
        plan.new_colleges.truncate(1);
        portal.apply(&plan, &lk).await.expect("apply all sections");
    })
    .await
}

#[tokio::test]
async fn plan_reads_can_fail_and_a_parsed_transcript_is_planned() {
    tokio::task::unconstrained(async move {
        let server = support::portal().await;
        let lk = Portal { name: "test".into(), client: Client::new(&server.uri(), "SESSION=x".into()) }
            .lookups()
            .await
            .expect("lookups");
        let dead = Portal { name: "test".into(), client: Client::new("not a URL", String::new()) };
        for section in [Section::Achievements, Section::Licenses, Section::Experiences, Section::Education] {
            assert!(
                dead.plan(&Profile::default(), Path::new("."), &BTreeSet::from([section]), &lk).await.is_err(),
                "{section:?}"
            );
        }
        let pdf = Path::new(env!("CARGO_MANIFEST_DIR")).join("../laccd/tests/fixtures/transcript.pdf");
        let src = TranscriptSource {
            path: pdf.display().to_string(),
            parser: "laccd".into(),
            colleges: [("LAVC".into(), "Valley".into())].into_iter().collect(),
            summer_term: "Summer".into(),
            academic_status: "Freshman".into(),
            skip: vec![],
            subjects: Default::default(),
            fix_titles: vec![],
            create_missing_colleges: false,
        };
        let mut profile = Profile { coursework: CourseworkFile { transcripts: vec![src] }, ..Profile::default() };
        assert!(dead.plan(&profile, Path::new("."), &BTreeSet::from([Section::Coursework]), &lk).await.is_err());
        profile.high_school.high_schools.push(HighSchool {
            name: "HS".into(),
            from: "2013-09".into(),
            to: "2017-05".into(),
            diploma: "Diploma".into(),
            diploma_date: String::new(),
            term_type: None,
        });
        assert!(dead.plan(&profile, Path::new("."), &BTreeSet::from([Section::HighSchool]), &lk).await.is_err());
        let live = Portal { name: "test".into(), client: Client::new(&server.uri(), "SESSION=x".into()) };
        let planned =
            live.plan(&profile, Path::new("."), &BTreeSet::from([Section::Coursework]), &lk).await.expect("plan");
        assert!(planned.findings.iter().any(|f| f.to_string().contains("Valley")), "{}", planned.render());
    })
    .await
}

#[path = "support/broken.rs"]
mod broken;
#[tokio::test]
async fn truncated_response_bodies_are_transport_failures() {
    tokio::task::unconstrained(async move {
        let url = broken::responses(vec![broken::TRUNCATED]);
        let client = admit_liaison::Client::new(&url, "SESSION=test".into());
        assert!(matches!(
            client.get("/navigation").await,
            Err(admit_liaison::ApiError::Transport { method: "GET", .. })
        ));
        let url = broken::responses(vec![broken::EMPTY, broken::TRUNCATED]);
        assert!(matches!(
            admit_liaison::Client::login(&url, "user", "password").await,
            Err(admit_liaison::ApiError::Transport { method: "POST", .. })
        ));
    })
    .await
}
