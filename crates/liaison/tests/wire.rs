//! Request shapes against a mock portal.

use admit_domain::{Experience, ExperienceKind, License, LicenseKind, Supervisor};
use admit_liaison::client::{ApiError, Client};
use admit_liaison::coursework::{CollegeCreate, create_college};
use admit_liaison::lookup::{Lookups, Table};
use admit_liaison::{experiences, licenses};
use serde_json::{Value, json};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[path = "support/broken.rs"]
mod broken;

fn table(name: &str, rows: &[(i64, &str)]) -> Table {
    Table::from_rows(name, &Value::Array(rows.iter().map(|(id, t)| json!({"id": id, "text": t})).collect()))
}

fn lookups() -> Lookups {
    Lookups {
        countries: table("country", &[(222, "United States")]),
        us_states: table("US state", &[(339, "California")]),
        achievement_types: table("achievement type", &[(18893, "Awards"), (18892, "Honors")]),
        experience_types: table("experience type", &[(18894, "Community Enrichment"), (18895, "Employment")]),
        employment_status: table("employment status", &[(5378, "Part time"), (5377, "Full time")]),
        license_types: table("license type", &[(18897, "Certification"), (18898, "Licensure")]),
        degree_types: table("degree type", &[]),
        majors: table("major", &[]),
        months: table("month", &[]),
        years: table("year", &[(12216, "2026")]),
        terms: table("term", &[(473, "Fall"), (471, "Spring")]),
        term_types: table("term type", &[(466, "Semester")]),
        academic_status: table("academic status", &[]),
        subjects: table("subject", &[]),
    }
}

async fn last_body(server: &MockServer) -> Vec<u8> {
    server.received_requests().await.unwrap_or_default().last().map(|r| r.body.clone()).unwrap_or_default()
}

#[tokio::test]
async fn experience_edit_resends_untouched_blocks() {
    tokio::task::unconstrained(async move {
        let server = MockServer::start().await;
        Mock::given(method("PUT"))
            .and(path("/applicant/api/questionBlock/supportingInfo/experiences/42"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
            .mount(&server)
            .await;
        let c = Client::new(&server.uri(), "SESSION=x".into());
        let live_blocks = [
            ("experienceDomain".to_string(), json!({"domains": [1, 2]})),
            ("importantExperience".to_string(), json!({"mostImportant": true, "mostImportantExperienceReason": "why"})),
            ("employmentDetails".to_string(), json!({"jobTitle": "Old", "receivedAutomaticCredit": false})),
        ]
        .into_iter()
        .collect();
        let x = Experience {
            kind: ExperienceKind::Employment,
            organization: "BardoVR".into(),
            title: "Virtual Reality Health & Wellness Developer".into(),
            state: "California".into(),
            country: "United States".into(),
            start: "2020-04-01".into(),
            end: Some("2020-08-01".into()),
            current: false,
            status: "Part time".into(),
            hours_per_week: 20,
            weeks: 17,
            total_hours: 340,
            compensated: false,
            volunteer: true,
            description: "Built VR".into(),
            supervisor: Supervisor::default(),
            ids: Default::default(),
        };
        experiences::update(&c, &lookups(), 42, &x, &live_blocks).await.expect("update");
        let body: Value = serde_json::from_slice(&last_body(&server).await).unwrap_or_default();
        let blocks: std::collections::BTreeMap<String, Value> = body["questionBlocks"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|b| (b["id"].as_str().unwrap_or("").to_string(), b["data"].clone()))
            .collect();
        assert_eq!(blocks["experienceDomain"], json!({"domains": [1, 2]}), "unmodelled block kept as-is");
        assert_eq!(blocks["importantExperience"]["mostImportantExperienceReason"], "why");
        assert_eq!(blocks["employmentDetails"]["jobTitle"], "Virtual Reality Health & Wellness Developer");
        assert_eq!(blocks["employmentDetails"]["averageWeeklyHours"], 20);
        assert_eq!(blocks["experienceType"]["experienceTypeId"], 18895);
        assert_eq!(blocks["organization"]["stateId"], 339);
    })
    .await
}

#[tokio::test]
async fn license_edit_is_a_multipart_model_carrying_the_id() {
    tokio::task::unconstrained(async move {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/applicant/api/supportingInfo/licenses"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": 9})))
            .mount(&server)
            .await;
        let c = Client::new(&server.uri(), "SESSION=x".into());
        let l = License {
            kind: LicenseKind::Certification,
            title: "Protecting Human Research Participants (NIH)".into(),
            organization: "National Institutes of Health".into(),
            description: Some("Credential ID: 2504076".into()),
            valid_until: None,
            ids: Default::default(),
        };
        licenses::save(&c, &lookups(), Some(9), &l).await.expect("save");
        let reqs = server.received_requests().await.unwrap_or_default();
        let ct = reqs[0].headers.get("content-type").and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
        assert!(ct.starts_with("multipart/form-data"), "{ct}");
        let body = String::from_utf8_lossy(&reqs[0].body).to_string();
        // The part header exactly as the browser sent it (POST_supportingInfo_licenses capture).
        assert!(
            body.contains("name=\"model\"; filename=\"blob\"\r\nContent-Type: application/json\r\n\r\n{"),
            "{body}"
        );
        let start = body.find('{').unwrap_or(0);
        let end = body.rfind('}').map_or(body.len(), |i| i + 1);
        let model: Value = serde_json::from_str(&body[start..end]).unwrap_or_default();
        assert_eq!(model["id"], 9);
        assert_eq!(model["typeId"], "18897");
        assert_eq!(model["nursingValidUntilDate"], Value::Null);
    })
    .await
}

#[tokio::test]
async fn an_html_page_means_the_session_expired() {
    tokio::task::unconstrained(async move {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_string("<!doctype html><title>Sign in</title>"))
            .mount(&server)
            .await;
        let c = Client::new(&server.uri(), "SESSION=stale".into());
        assert!(matches!(c.get("/navigation").await, Err(ApiError::Expired { .. })));
    })
    .await
}

#[tokio::test]
async fn a_file_is_read_as_served_and_an_html_answer_is_the_sign_in_page() {
    tokio::task::unconstrained(async move {
        let server = MockServer::start().await;
        // A held text file may itself start with '<': only the type says HTML.
        Mock::given(method("GET"))
            .and(path("/applicant/api/programMaterials/documents/view/1"))
            .and(wiremock::matchers::header("Accept", "*/*"))
            .respond_with(ResponseTemplate::new(200).set_body_raw("<a>", "text/plain"))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/applicant/api/programMaterials/documents/view/2"))
            .respond_with(
                // `set_body_string` would answer `text/plain` whatever header is set.
                ResponseTemplate::new(200)
                    .set_body_raw("<!doctype html><title>Sign in</title>", "text/html;charset=UTF-8"),
            )
            .mount(&server)
            .await;
        let c = Client::new(&server.uri(), "SESSION=x".into());
        assert_eq!(c.get_bytes("/programMaterials/documents/view/1").await.expect("file"), b"<a>");
        assert!(matches!(c.get_bytes("/programMaterials/documents/view/2").await, Err(ApiError::Expired { .. })));
        let e = c.get_bytes("/programMaterials/documents/view/3").await.expect_err("404").to_string();
        assert!(e.contains("view/3: HTTP 404"), "{e}");
    })
    .await
}

#[tokio::test]
async fn adding_a_college_sends_what_the_portal_form_sent_for_pierce() {
    tokio::task::unconstrained(async move {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/applicant/api/lookup/colleges"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {"id": 1, "code": "999999", "text": "LOS ANGELES PIERCE COLLEGE - ANNEX"},
            {"id": 2, "code": "001226", "text": "LOS ANGELES PIERCE COLLEGE"}])))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/applicant/api/academicHistory/colleges"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;
        let c = Client::new(&server.uri(), "SESSION=x".into());
        let cc = CollegeCreate {
            name: "LOS ANGELES PIERCE COLLEGE".into(),
            first: ("Fall".into(), "2026".into()),
            last: ("Fall".into(), "2026".into()),
            months: (10, 12),
            still_attending: false,
        };
        create_college(&c, &lookups(), &cc).await.expect("create");
        let body: Value = serde_json::from_slice(&last_body(&server).await).unwrap_or_default();
        // The body the web form sent when Pierce was added by hand (HAR3).
        let captured = json!({"countryId":"","college":"LOS ANGELES PIERCE COLLEGE","isPrimary":false,"termTypeId":466,
        "stillAttending":false,"startTermId":473,"startMonthId":13,"startYearId":12216,"endTermId":473,"endMonthId":15,
        "endYearId":12216,"isDegree":"no","collegeCode":"001226","collegeEndMonthId":15,"collegeEndTermId":473,
        "collegeEndYearId":12216,"collegeStartMonthId":13,"collegeStartTermId":473,"collegeStartYearId":12216,
        "degrees":[],"degreeObtained":"no","auEnabled":false,"optOut":false});
        assert_eq!(body, captured);
    })
    .await
}

#[tokio::test]
async fn a_401_means_the_session_expired() {
    tokio::task::unconstrained(async move {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(401).set_body_json(json!({"status": "401",
            "messages": [{"message": "Full authentication is required to access this resource"}]})))
            .mount(&server)
            .await;
        let c = Client::new(&server.uri(), "SESSION=stale".into());
        assert!(matches!(c.get("/academicHistory/highSchools").await, Err(ApiError::Expired { .. })));
    })
    .await
}

async fn login_portal() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/applicant/initialize"))
        .respond_with(
            ResponseTemplate::new(200)
                .append_header("Set-Cookie", "cas=809; path=/applicant")
                .append_header("Set-Cookie", "casName=NursingCAS; path=/applicant")
                .append_header("Set-Cookie", "formId=12607; path=/applicant"),
        )
        .mount(&server)
        .await;
    // Basic base64("me@x.example:pw:with:colons")
    Mock::given(method("POST"))
        .and(path("/applicant/authenticate"))
        .and(wiremock::matchers::header("authorization", "Basic bWVAeC5leGFtcGxlOnB3OndpdGg6Y29sb25z"))
        .and(wiremock::matchers::header_regex("cookie", "cas=809.*formId=12607"))
        .and(wiremock::matchers::body_json(json!({})))
        .respond_with(
            ResponseTemplate::new(200)
                .append_header("Set-Cookie", "SESSION=abc; Path=/applicant; Secure; HttpOnly")
                .set_body_json(json!({"messages": [{"message": "Successful Authentication"}]})),
        )
        .with_priority(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/applicant/authenticate"))
        .respond_with(ResponseTemplate::new(401).set_body_string("Bad credentials"))
        .mount(&server)
        .await;
    server
}

#[tokio::test]
async fn login_sends_basic_auth_after_initialize_and_returns_the_cookie_header() {
    tokio::task::unconstrained(async move {
        let server = login_portal().await;
        let cookie = Client::login(&server.uri(), "me@x.example", "pw:with:colons").await.expect("login");
        assert_eq!(cookie, "cas=809; casName=NursingCAS; formId=12607; SESSION=abc");
    })
    .await
}

#[tokio::test]
async fn login_transport_failures_are_reported_per_request() {
    tokio::task::unconstrained(async move {
        let dropped = broken::responses(vec![broken::DROPPED]);
        let err = Client::login(&dropped, "me@x.example", "pw").await.expect_err("initialize dropped");
        assert!(err.to_string().contains("/initialize"), "{err}");
        let authenticate = broken::responses(vec![broken::EMPTY, broken::DROPPED]);
        let err = Client::login(&authenticate, "me@x.example", "pw").await.expect_err("authenticate dropped");
        assert!(err.to_string().contains("/authenticate"), "{err}");
        let body = broken::responses(vec![broken::EMPTY, broken::TRUNCATED]);
        let err = Client::login(&body, "me@x.example", "pw").await.expect_err("authenticate body truncated");
        assert!(matches!(err, ApiError::Transport { .. }), "{err}");
    })
    .await
}

#[tokio::test]
async fn login_with_a_wrong_password_is_bad_login() {
    tokio::task::unconstrained(async move {
        let server = login_portal().await;
        match Client::login(&server.uri(), "me@x.example", "nope").await {
            Err(ApiError::BadLogin { status: 401, .. }) => {}
            other => panic!("expected BadLogin 401, got {other:?}"),
        }
    })
    .await
}

#[tokio::test]
async fn course_edit_puts_to_the_course_url_from_the_bundle() {
    tokio::task::unconstrained(async move {
        let server = MockServer::start().await;
        Mock::given(method("PUT"))
            .and(path("/applicant/api/academicHistory/courses/collegeId/2/termId/1/courseId/9"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
            .expect(1)
            .mount(&server)
            .await;
        let c = Client::new(&server.uri(), "SESSION=x".into());
        let e = admit_liaison::coursework::CourseEdit {
            college_id: 2,
            college: "CONNECTICUT COLLEGE".into(),
            term_id: 1,
            term: "Spring 2021".into(),
            course_id: 9,
            from: ("PSY 323".into(), "SPORT & EXERCISE".into()),
            to: ("PSY 321".into(), "SPORT & EXERCISE PSYCHOLOGY".into()),
            subject: None,
            body: json!({"code": "PSY 321", "title": "SPORT & EXERCISE PSYCHOLOGY"}),
        };
        admit_liaison::coursework::apply_edits(&c, &[e]).await.expect("put");
        let sent: Value = serde_json::from_slice(&last_body(&server).await).unwrap_or_default();
        assert_eq!(sent["code"], "PSY 321");
    })
    .await
}

#[tokio::test]
async fn attendance_resaves_the_college_keeping_its_degrees() {
    tokio::task::unconstrained(async move {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/applicant/api/academicHistory/colleges"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"collegesList": [{
            "id": 26125387, "college": "LOS ANGELES PIERCE COLLEGE", "collegeCode": "001226", "isPrimary": false,
            "termTypeId": 466, "stillAttending": false, "collegeStartTermId": 471, "collegeStartMonthId": 8,
            "collegeStartYearId": 12216, "collegeEndTermId": 471, "collegeEndMonthId": 9, "collegeEndYearId": 12216,
            "degrees": [{"id": 77, "degreeStatusId": 431, "typeId": 489, "monthId": 13, "yearId": 12216,
                         "majorId": 1, "year": "2026", "month": "October"}]}]})))
            .mount(&server)
            .await;
        Mock::given(method("PUT"))
            .and(path("/applicant/api/academicHistory/colleges/26125387"))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&server)
            .await;
        let c = Client::new(&server.uri(), "SESSION=x".into());
        let fix = admit_liaison::coursework::AttendanceFix {
            college_id: 26125387,
            college: "LOS ANGELES PIERCE COLLEGE".into(),
            still_attending: true,
            last: ("Fall".into(), "2026".into()),
            end_month: 12,
            was: "ended Spring 2026".into(),
        };
        admit_liaison::coursework::apply_attendance(&c, &lookups(), &[fix]).await.expect("put");
        let sent: Value = serde_json::from_slice(&last_body(&server).await).unwrap_or_default();
        assert_eq!(sent["stillAttending"], true);
        assert!(sent.get("endTermId").is_none(), "a still-attending college sends no end term: {sent}");
        assert_eq!(sent["degrees"].as_array().map(Vec::len), Some(1), "degrees are re-sent, not dropped");
        assert_eq!(sent["degrees"][0]["id"], 77);
    })
    .await
}

#[test]
fn high_school_edit_sends_term_type_and_graduation_like_the_form() {
    // The stored row as captured 2026-10-06 (no term type, no graduation ids).
    let row = json!({"id": 12752875, "highSchoolName": "BRIDGES ACADEMY", "stateId": 339, "countryId": 222,
        "code": "050859", "termTypeId": null, "monthGraduatedId": null, "yearGraduatedId": null,
        "fromYearId": 19, "fromMonthId": 12, "toYearId": 12217, "toMonthId": 9,
        "fromYear": "2013", "fromMonth": "September", "toYear": "2027", "toMonth": "June", "graduated": true});
    let mut lk = lookups();
    lk.years = table("year", &[(19, "2013"), (5745, "2017")]);
    lk.months = table("month", &[(8, "May"), (12, "September")]);
    lk.term_types = table("term type", &[(465, "Quarter"), (466, "Semester")]);
    let want = admit_domain::HighSchool {
        name: "BRIDGES ACADEMY".into(),
        from: "2013-09".into(),
        to: "2017-05".into(),
        diploma: "High School Diploma".into(),
        diploma_date: "2017-05".into(),
        term_type: Some("Semester".into()),
    };
    let b = admit_liaison::high_school::attendance_body(&row, &want, &lk).expect("body");
    assert_eq!(
        (b["toYearId"].clone(), b["toMonthId"].clone(), b["toYear"].clone()),
        (json!(5745), json!(8), json!("2017"))
    );
    assert_eq!(b["termTypeId"], 466);
    assert_eq!(
        (b["graduated"].clone(), b["monthGraduatedId"].clone(), b["yearGraduatedId"].clone()),
        (json!(true), json!(8), json!(5745))
    );
    assert_eq!(b["code"], "050859", "the rest of the row is re-sent");
    let no_type = admit_domain::HighSchool { term_type: None, ..want };
    assert!(admit_liaison::high_school::attendance_body(&row, &no_type, &lk).is_err(), "never guesses a term type");
}

#[tokio::test]
async fn attendance_for_a_college_without_degrees_says_no_degree() {
    tokio::task::unconstrained(async move {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/applicant/api/academicHistory/colleges"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"collegesList": [{
            "id": 5, "college": "LOS ANGELES PIERCE COLLEGE", "collegeCode": "001226", "isPrimary": false,
            "termTypeId": 466, "stillAttending": true, "collegeStartTermId": 471, "collegeStartMonthId": 8,
            "collegeStartYearId": 12216, "degrees": []}]})))
            .mount(&server)
            .await;
        Mock::given(method("PUT"))
            .and(path("/applicant/api/academicHistory/colleges/5"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;
        let c = Client::new(&server.uri(), "SESSION=x".into());
        let fix = admit_liaison::coursework::AttendanceFix {
            college_id: 5,
            college: "LOS ANGELES PIERCE COLLEGE".into(),
            still_attending: false,
            last: ("Fall".into(), "2026".into()),
            end_month: 12,
            was: "still attending".into(),
        };
        admit_liaison::coursework::apply_attendance(&c, &lookups(), &[fix]).await.expect("put");
        let sent: Value = serde_json::from_slice(&last_body(&server).await).unwrap_or_default();
        assert_eq!((sent["isDegree"].clone(), sent["degreeObtained"].clone()), (json!("no"), json!("no")));
        assert_eq!(sent["stillAttending"], false);
        assert_eq!(
            (sent["endTermId"].clone(), sent["endYearId"].clone(), sent["endMonthId"].clone()),
            (json!(473), json!(12216), json!(15))
        );
    })
    .await
}
