use super::*;
use crate::lookup::{Lookups, Table};
use serde_json::Value;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn table(name: &str, rows: &[(i64, &str)]) -> Table {
    Table::from_rows(name, &Value::Array(rows.iter().map(|(id, t)| json!({"id": id, "text": t})).collect()))
}

fn lookups() -> Lookups {
    Lookups {
        countries: table("country", &[]),
        us_states: table("US state", &[]),
        achievement_types: table("achievement type", &[]),
        experience_types: table("experience type", &[]),
        employment_status: table("employment status", &[]),
        license_types: table("license type", &[]),
        degree_types: table("degree type", &[]),
        majors: table("major", &[]),
        months: table("month", &[]),
        years: table("year", &[(12216, "2026"), (19, "2020")]),
        terms: table("term", &[(473, "Fall"), (471, "Spring"), (470, "Winter")]),
        term_types: table("term type", &[(466, "Semester")]),
        academic_status: table("academic status", &[(1, "Post-Baccalaureate")]),
        subjects: table("subject", &[(9, "Biology")]),
    }
}

fn src() -> TranscriptSource {
    TranscriptSource {
        path: "t.pdf".into(),
        parser: "laccd".into(),
        colleges: [("LAVC", "LOS ANGELES VALLEY COLLEGE"), ("LAPC", "LOS ANGELES PIERCE COLLEGE")]
            .into_iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect(),
        summer_term: "Summer 2".into(),
        academic_status: "Post-Baccalaureate".into(),
        skip: vec![],
        subjects: [("BIO 9".into(), "Biology".into())].into(),
        create_missing_colleges: true,
        fix_titles: vec![],
    }
}

fn tc(code: &str, title: &str, college: &str, season: &str, year: &str, grade: &str) -> TranscriptCourse {
    TranscriptCourse {
        year: year.into(),
        season: season.into(),
        code: code.into(),
        college: college.into(),
        title: title.into(),
        credits: "3.00".into(),
        grade: grade.into(),
        noncredit: false,
        term_months: None,
    }
}

fn valley() -> LiveCollege {
    LiveCollege {
        id: 1,
        name: "LOS ANGELES VALLEY COLLEGE".into(),
        code: "001228".into(),
        terms: vec![json!({"id": 10, "term": "Fall", "year": "2026", "termInProgress": false, "courses": [
            {"id": 50, "code": "PSY 323", "title": "SPORT & EXERCISE", "subject": "Psychology",
             "grade": "A", "convertedGrade": "B",
             "courseClassification": {"labSelected": true}},
            {"id": 51, "code": "MAT 210", "title": "DISCRETE MATHEMATICS", "subject": "Mathematics", "grade": "None"}
        ]})],
        info: json!({"stillAttending": true, "collegeEndTerm": "", "collegeEndYear": ""}),
    }
}

fn college(name: &str, first: (&str, &str), last: (&str, &str), still: bool) -> CollegeCreate {
    CollegeCreate {
        name: name.into(),
        first: (first.0.into(), first.1.into()),
        last: (last.0.into(), last.1.into()),
        months: (8, 6),
        still_attending: still,
    }
}

fn add(target: TermTarget) -> CourseAdd {
    CourseAdd {
        college_id: 4,
        college: "Valley".into(),
        college_code: "001228".into(),
        target,
        course: tc("BIO 1", "Intro", "LAVC", "Fall", "2026", "A"),
        subject: "Biology".into(),
    }
}

fn new_term() -> TermTarget {
    TermTarget::New {
        term: "Fall".into(),
        year: "2026".into(),
        in_progress: false,
        academic_status: "Post-Baccalaureate".into(),
    }
}

fn fix(still: bool, last: (&str, &str)) -> AttendanceFix {
    AttendanceFix {
        college_id: 7,
        college: "Valley".into(),
        still_attending: still,
        last: (last.0.into(), last.1.into()),
        end_month: 13,
        was: String::new(),
    }
}

async fn client() -> (MockServer, Client) {
    let server = MockServer::start().await;
    let c = Client::new(&server.uri(), "SESSION=x".into());
    (server, c)
}

async fn mount(server: &MockServer, verb: &str, p: &str, status: u16, body: Value) {
    Mock::given(method(verb))
        .and(path(p))
        .respond_with(ResponseTemplate::new(status).set_body_json(body))
        .mount(server)
        .await;
}

#[test]
fn blank_grades_default_months_and_college_span() {
    assert_eq!(blank_grade(&json!("None")), "");
    assert_eq!(blank_grade(&json!("A")), "A");
    assert_eq!(default_months("Winter"), (1, 2));
    assert_eq!(default_months("Spring"), (2, 6));
    assert!(college_create("Valley", &[]).is_none());

    let winter = tc("BIO 1", "Bio", "LAPC", "Winter", "2020", "A");
    let spring = tc("BIO 2", "Bio 2", "LAPC", "Spring", "2026", "");
    // Later row is the earlier term, so both comparisons move the running bound.
    let created = college_create("Pierce", &[&spring, &winter]).expect("span");
    assert_eq!(created.months, (1, 6));
    assert!(created.still_attending);
    assert_eq!(created.first.0, "Winter");
    assert_eq!(created.last.0, "Spring");
}

#[test]
fn plan_hits_unmatched_college_summer_term_and_grade_text() {
    let summer = tc("BIO 9", "Intro Biology", "LAVC", "Summer", "2025", "A");
    let abbrev = tc("PSY 321", "SPORT & EXERCISE PSYCHOLOGY", "LAVC", "Fall", "2026", "A");
    let graded = tc("MAT 210", "DISCRETE MATHEMATICS", "LAVC", "Fall", "2026", "A");
    let missing = tc("SOC 1", "Sociology", "LAPC", "Winter", "2020", "A");
    let spring = tc("SOC 2", "Sociology 2", "LAPC", "Spring", "2026", "");
    let unknown = tc("ZZZ 1", "Nowhere", "NOWHERE", "Fall", "2026", "A");
    let p = plan(&src(), &[summer, abbrev, graded, missing, spring, unknown], &[valley()], |_| Some(1));
    let notes: Vec<String> = p.findings.iter().map(ToString::to_string).collect();
    assert!(notes.iter().any(|m| m.contains("portal grade")), "{notes:?}");
    assert!(
        p.adds
            .iter()
            .any(|a| matches!(&a.target, TermTarget::New { term, year, .. } if term == "Summer 2" && year == "2025")),
        "{:?}",
        p.adds
    );
    assert!(p.edits.iter().any(|e| e.body["convertedGrade"] == "B"), "{:?}", p.edits);
    assert_eq!(p.colleges.len(), 1, "{:?}", p.colleges);
    assert_eq!(p.colleges[0].months, (1, 6));
    assert!(!notes.iter().any(|m| m.contains("NOWHERE")), "{notes:?}");
}

#[test]
fn course_edit_keeps_a_converted_grade_and_falls_back_on_subject() {
    let col = valley();
    let t = json!({"id": 10, "term": "Fall", "year": "2026"});
    let x = json!({
        "id": 3, "code": "BIO 1", "title": "Intro", "grade": "None", "convertedGrade": "B",
        "subjectId": 4, "subject": "Biology", "credits": "3.00",
        "courseClassification": {"labSelected": true, "honorsSelected": "no"}, "source": null
    });
    let edited = course_edit(&col, &t, &x, ("BIO 1".into(), "Intro".into()), Some(("Art".into(), 8)));
    assert_eq!(edited.body["convertedGrade"], "B");
    assert_eq!(edited.body["grade"], "");
    assert_eq!(edited.body["subjectId"], "8");
    assert_eq!(edited.body["courseClassification"]["labSelected"], true);
    assert_eq!(edited.body["courseClassification"]["honorsSelected"], false);

    let y = json!({
        "id": 3, "code": "BIO 1", "title": "Intro", "grade": "A", "convertedGrade": "",
        "subjectId": 4, "subject": "Biology", "credits": "3", "courseClassification": {}, "source": null
    });
    let edited = course_edit(&col, &t, &y, ("BIO 1".into(), "Intro".into()), None);
    assert_eq!(edited.body["convertedGrade"], "None");
    assert_eq!(edited.body["subjectId"], "4");
}

#[tokio::test]
async fn live_reports_coursework_and_summary_errors() {
    tokio::task::unconstrained(async move {
        let (server, c) = client().await;
        assert!(live(&c).await.is_err(), "coursework");
        mount(
            &server,
            "GET",
            "/applicant/api/academicHistory/courseWork",
            200,
            json!([{"id": 4, "college": "X", "collegeCode": "1"}]),
        )
        .await;
        assert!(live(&c).await.is_err(), "summary");
        mount(&server, "GET", "/applicant/api/academicHistory/courseWork/summary/4", 200, json!({"terms": []})).await;
        let rows = live(&c).await.expect("live");
        assert_eq!(rows.len(), 1);
        assert_eq!((rows[0].name.as_str(), rows[0].code.as_str()), ("X", "1"));
    })
    .await
}

#[tokio::test]
async fn create_college_reports_each_lookup_failure() {
    tokio::task::unconstrained(async move {
        let (server, c) = client().await;
        let lk = lookups();
        let cc = college("Los Angeles Valley College", ("Fall", "2026"), ("Spring", "2020"), false);
        assert!(create_college(&c, &lk, &cc).await.is_err(), "lookup get");

        mount(
            &server,
            "GET",
            "/applicant/api/lookup/colleges",
            200,
            json!([{"text": "Some Other College", "code": "9"}]),
        )
        .await;
        let err = create_college(&c, &lk, &cc).await.expect_err("name");
        assert!(err.to_string().contains("not in the portal"), "{err}");

        Mock::given(method("GET"))
            .and(path("/applicant/api/lookup/colleges"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!([{"text": "Los Angeles Valley College", "code": "001228"}])),
            )
            .with_priority(1)
            .mount(&server)
            .await;
        let mut lk = lookups();
        lk.terms = table("term", &[]);
        let err = create_college(&c, &lk, &cc).await.expect_err("start term");
        assert!(err.to_string().contains("Fall"), "{err}");

        let mut lk = lookups();
        lk.years = table("year", &[(19, "2020")]);
        let err = create_college(&c, &lk, &cc).await.expect_err("start year");
        assert!(err.to_string().contains("2026"), "{err}");

        let mut lk = lookups();
        lk.term_types = table("term type", &[]);
        let err = create_college(&c, &lk, &cc).await.expect_err("term type");
        assert!(err.to_string().contains("Semester"), "{err}");

        let mut lk = lookups();
        lk.terms = table("term", &[(473, "Fall")]);
        let err = create_college(&c, &lk, &cc).await.expect_err("end term");
        assert!(err.to_string().contains("Spring"), "{err}");

        let mut lk = lookups();
        lk.years = table("year", &[(12216, "2026")]);
        let err = create_college(&c, &lk, &cc).await.expect_err("end year");
        assert!(err.to_string().contains("2020"), "{err}");

        let mut cc = college("Los Angeles Valley College", ("Fall", "2026"), ("Fall", "2026"), false);
        cc.months = (13, 0);
        let err = create_college(&c, &lookups(), &cc).await.expect_err("post");
        assert!(err.to_string().contains("500") || err.to_string().contains("404"), "{err}");

        mount(&server, "POST", "/applicant/api/academicHistory/colleges", 200, json!({})).await;
        let attending = college("Los Angeles Valley College", ("Fall", "2026"), ("Fall", "2026"), true);
        create_college(&c, &lookups(), &attending).await.expect("still attending");
    })
    .await
}

#[tokio::test]
async fn apply_edits_and_attendance_report_each_failure() {
    tokio::task::unconstrained(async move {
        let (server, c) = client().await;
        let edit = CourseEdit {
            college_id: 1,
            college: "Valley".into(),
            term_id: 2,
            term: "Fall 2026".into(),
            course_id: 3,
            from: ("A".into(), "B".into()),
            to: ("A".into(), "B".into()),
            subject: None,
            body: json!({}),
        };
        assert!(apply_edits(&c, &[edit]).await.is_err(), "put");
        assert!(apply_edits(&c, &[]).await.is_ok());

        assert!(apply_attendance(&c, &lookups(), &[fix(false, ("Fall", "2026"))]).await.is_err(), "education");

        mount(
            &server,
            "GET",
            "/applicant/api/academicHistory/colleges",
            200,
            json!({"collegesList": [{"id": 1, "degrees": [], "stillAttending": true}]}),
        )
        .await;
        let err = apply_attendance(&c, &lookups(), &[fix(false, ("Fall", "2026"))]).await.expect_err("missing");
        assert!(err.to_string().contains("no longer"), "{err}");

        Mock::given(method("GET"))
            .and(path("/applicant/api/academicHistory/colleges"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "collegesList": [{"id": 7, "degrees": [], "college": "Valley", "stillAttending": true,
                    "termTypeId": 466, "collegeStartTermId": 473, "collegeStartMonthId": 8,
                    "collegeStartYearId": 12216}]
            })))
            .with_priority(1)
            .mount(&server)
            .await;
        let mut lk = lookups();
        lk.terms = table("term", &[]);
        let err = apply_attendance(&c, &lk, &[fix(false, ("Winter", "2026"))]).await.expect_err("end term");
        assert!(err.to_string().contains("Winter"), "{err}");

        let mut lk = lookups();
        lk.years = table("year", &[]);
        let err = apply_attendance(&c, &lk, &[fix(false, ("Fall", "2026"))]).await.expect_err("end year");
        assert!(err.to_string().contains("2026"), "{err}");

        let err = apply_attendance(&c, &lookups(), &[fix(false, ("Fall", "2026"))]).await.expect_err("put");
        assert!(err.to_string().contains("500") || err.to_string().contains("404"), "{err}");

        mount(&server, "PUT", "/applicant/api/academicHistory/colleges/7", 200, json!({})).await;
        apply_attendance(&c, &lookups(), &[fix(true, ("Fall", "2026"))]).await.expect("still attending");

        assert!(apply_attendance(&c, &lookups(), &[]).await.is_ok());
    })
    .await
}

#[tokio::test]
async fn apply_reports_each_term_and_course_failure() {
    tokio::task::unconstrained(async move {
        let (server, c) = client().await;
        let lk = lookups();

        let mut missing_term = lookups();
        missing_term.terms = table("term", &[]);
        let err = apply(&c, &missing_term, &[add(new_term())]).await.expect_err("term");
        assert!(err.to_string().contains("Fall"), "{err}");

        let mut missing_year = lookups();
        missing_year.years = table("year", &[]);
        let err = apply(&c, &missing_year, &[add(new_term())]).await.expect_err("year");
        assert!(err.to_string().contains("2026"), "{err}");

        let mut missing_status = lookups();
        missing_status.academic_status = table("academic status", &[]);
        let err = apply(&c, &missing_status, &[add(new_term())]).await.expect_err("status");
        assert!(err.to_string().contains("Post-Baccalaureate"), "{err}");

        let err = apply(&c, &lk, &[add(new_term())]).await.expect_err("term post");
        assert!(err.to_string().contains("saveOrUpdateTermAndCourses"), "{err}");

        let existing = TermTarget::Existing { id: 10, label: "Fall 2026".into() };
        let err = apply(&c, &lk, &[add(existing.clone())]).await.expect_err("grade");
        assert!(err.to_string().contains("gradeConversion"), "{err}");

        mount(&server, "POST", "/applicant/api/gradeConversion", 200, json!({"grade": "A"})).await;
        let mut missing_subject = lookups();
        missing_subject.subjects = table("subject", &[]);
        let err = apply(&c, &missing_subject, &[add(existing.clone())]).await.expect_err("subject");
        assert!(err.to_string().contains("Biology"), "{err}");

        let err = apply(&c, &lk, &[add(existing)]).await.expect_err("course post");
        assert!(err.to_string().contains("courses/collegeId/4"), "{err}");

        mount(&server, "POST", "/applicant/api/saveOrUpdateTermAndCourses/4", 200, json!({"id": 55})).await;
        mount(&server, "POST", "/applicant/api/academicHistory/courses/collegeId/4/termId/55", 200, json!({})).await;
        let first = add(new_term());
        let second = add(new_term());
        apply(&c, &lk, &[first, second]).await.expect("new term once");

        Mock::given(method("POST"))
            .and(path("/applicant/api/saveOrUpdateTermAndCourses/4"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": 0})))
            .with_priority(1)
            .mount(&server)
            .await;
        let err = apply(&c, &lk, &[add(new_term())]).await.expect_err("no id");
        assert!(err.to_string().contains("returned no id"), "{err}");

        assert!(apply(&c, &lk, &[]).await.is_ok());
    })
    .await
}
