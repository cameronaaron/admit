//! Branches the happy-path portal never takes: each failed fetch, a parsed
//! transcript, and plan lines for an in-progress course and a subject-only edit.

mod support;

#[path = "support/broken.rs"]
mod broken;

use std::collections::BTreeSet;
use std::path::Path;

use admit_domain::*;
use admit_liaison::coursework::{CollegeCreate, CourseAdd, CourseEdit, TermTarget};
use admit_liaison::lookup::{Lookups, Table};
use admit_liaison::reconcile::{Diff, Step};
use admit_liaison::{ApiError, Client, Plan, Portal, Section};
use admit_sources::TranscriptCourse;
use serde_json::{Value, json};
use time::macros::date;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn lookups() -> Lookups {
    Lookups {
        countries: Table::default(),
        us_states: Table::default(),
        achievement_types: Table::default(),
        experience_types: Table::default(),
        employment_status: Table::default(),
        license_types: Table::default(),
        degree_types: Table::default(),
        majors: Table::default(),
        months: Table::default(),
        years: Table::default(),
        terms: Table::default(),
        term_types: Table::default(),
        academic_status: Table::default(),
        subjects: Table::from_rows("subject", &json!([{"id": 1, "text": "Other Science"}])),
    }
}

fn source(colleges: &[(&str, &str)], subject: Option<(&str, &str)>) -> TranscriptSource {
    TranscriptSource {
        path: "unused.pdf".into(),
        parser: "laccd".into(),
        colleges: colleges.iter().map(|(k, v)| ((*k).to_string(), (*v).to_string())).collect(),
        summer_term: "Summer".into(),
        academic_status: "Freshman".into(),
        skip: vec![],
        subjects: subject.into_iter().map(|(c, s)| (c.to_string(), s.to_string())).collect(),
        fix_titles: vec![],
        create_missing_colleges: false,
    }
}

fn school() -> HighSchool {
    HighSchool {
        name: "HS".into(),
        from: "2013-09".into(),
        to: "2017-05".into(),
        diploma: "Diploma".into(),
        diploma_date: String::new(),
        term_type: None,
    }
}

fn course(grade: &str) -> TranscriptCourse {
    TranscriptCourse {
        year: "2026".into(),
        season: "Fall".into(),
        code: "BIO 1".into(),
        college: "LAVC".into(),
        title: "Biology".into(),
        credits: "3.00".into(),
        grade: grade.into(),
        noncredit: false,
        term_months: None,
    }
}

async fn reply(server: &MockServer, verb: &str, endpoint: &str, status: u16, body: Value) {
    Mock::given(method(verb))
        .and(path(format!("/applicant/api{endpoint}")))
        .respond_with(ResponseTemplate::new(status).set_body_json(body))
        .mount(server)
        .await;
}

async fn fail_at(server: &MockServer, calls: &[(&str, &str, Value)], index: usize) {
    server.reset().await;
    for (i, (verb, endpoint, body)) in calls.iter().enumerate() {
        reply(server, verb, endpoint, if i == index { 500 } else { 200 }, body.clone()).await;
    }
}

#[test]
fn render_covers_in_progress_courses_and_subject_only_edits() {
    let add = CourseAdd {
        college_id: 8,
        college: "Valley".into(),
        college_code: "LAVC".into(),
        target: TermTarget::New {
            term: "Fall".into(),
            year: "2026".into(),
            in_progress: true,
            academic_status: "Freshman".into(),
        },
        course: course(""),
        subject: "Other Science".into(),
    };
    let edit = CourseEdit {
        college_id: 8,
        college: "Valley".into(),
        term_id: 1,
        term: "Fall 2026".into(),
        course_id: 1,
        from: ("BIO 1".into(), "Biology".into()),
        to: ("BIO 1".into(), "Biology".into()),
        subject: None,
        body: json!({}),
    };
    let plan = Plan { courses: vec![add], course_edits: vec![edit], ..Plan::default() };
    let text = plan.render();
    assert!(text.contains("(in progress)"), "{text}");
    assert!(text.contains("grade -"), "{text}");
    assert!(text.contains("course(s) to correct"), "{text}");
}

#[tokio::test]
async fn authenticate_that_never_answers_is_a_transport_error() {
    tokio::task::unconstrained(async move {
        let url = broken::responses(vec![broken::EMPTY, broken::DROPPED]);
        let err = Client::login(&url, "user", "password").await.expect_err("dropped authenticate");
        assert!(matches!(err, ApiError::Transport { .. }), "{err}");
        assert!(err.to_string().contains("/authenticate"), "{err}");
    })
    .await
}

#[tokio::test]
async fn post_model_sends_the_json_part() {
    tokio::task::unconstrained(async move {
        let server = MockServer::start().await;
        reply(&server, "POST", "/supportingInfo/licenses", 200, json!({"id": 1})).await;
        let client = Client::new(&server.uri(), "SESSION=x".into());
        let saved = client.post_model("/supportingInfo/licenses", &json!({"id": 0})).await.expect("saved");
        assert_eq!(saved["id"], 1);
    })
    .await
}

#[tokio::test]
async fn each_lookup_failure_is_reported() {
    tokio::task::unconstrained(async move {
        let paths = [
            "/lookup/countries",
            "/lookup/states",
            "/lookup/achievementTypes",
            "/lookup/experienceTypes",
            "/lookup/employmentStatusTypes",
            "/lookup/license",
            "/lookup/degreeType",
            "/lookup/collegeMinor",
            "/lookup/month",
            "/lookup/year",
            "/lookup/terms",
            "/lookup/termType",
            "/lookup/academicStatus",
            "/lookup/subjects",
        ];
        let calls: Vec<_> = paths.iter().map(|p| ("GET", *p, json!([{"id": 1, "text": "X"}]))).collect();
        let server = MockServer::start().await;
        let client = Client::new(&server.uri(), "SESSION=x".into());
        for i in 0..calls.len() {
            fail_at(&server, &calls, i).await;
            assert!(Lookups::fetch(&client).await.is_err(), "{}", calls[i].1);
        }
    })
    .await
}

#[tokio::test]
async fn audit_stops_when_any_portal_read_fails() {
    tokio::task::unconstrained(async move {
        let calls = [
            ("GET", "/navigation", json!([])),
            ("GET", "/getProgramNavigation", json!([{"orgName": "U", "programName": "N", "programSelectedId": 7}])),
            ("GET", "/programsSelected", json!({"selectedPrograms": []})),
            ("POST", "/search/filteredQueries", json!({"programs": []})),
            ("GET", "/programMaterials/evaluations/programSelected/7", json!({})),
            ("GET", "/transcripts", json!({"college": {}})),
            ("GET", "/academicHistory/courseWork/prerequisites", json!([])),
            ("GET", "/academicHistory/courseWork", json!([])),
        ];
        let server = MockServer::start().await;
        let client = Client::new(&server.uri(), "SESSION=x".into());
        for i in 0..calls.len() {
            fail_at(&server, &calls, i).await;
            assert!(admit_liaison::audit::run(&client, date!(2026 - 10 - 06), &[]).await.is_err(), "{}", calls[i].1);
        }
        for i in 0..2 {
            fail_at(&server, &calls[2..4], i).await;
            assert!(admit_liaison::audit::deadlines(&client).await.is_err(), "{}", calls[i + 2].1);
        }
    })
    .await
}

#[tokio::test]
async fn colleges_sharing_a_transcript_are_grouped() {
    tokio::task::unconstrained(async move {
        let server = support::portal().await;
        let portal = Portal { name: "test".into(), client: Client::new(&server.uri(), "SESSION=x".into()) };
        let mut profile = Profile::default();
        profile.coursework.transcripts =
            vec![source(&[("LAVC", "Valley")], None), source(&[("LAMC", "Mission"), ("ELAC", "East")], None)];
        let findings = portal.audit(&profile, date!(2026 - 10 - 06)).await.expect("audit");
        assert!(findings.iter().any(|f| f.to_string().contains("recommendation")), "{findings:?}");
    })
    .await
}

#[tokio::test]
async fn a_parsed_transcript_plans_known_and_unknown_subjects() {
    tokio::task::unconstrained(async move {
        let server = MockServer::start().await;
        reply(
            &server,
            "GET",
            "/academicHistory/courseWork",
            200,
            json!([{"id": 8, "college": "Valley", "collegeCode": "LAVC"}]),
        )
        .await;
        reply(&server, "GET", "/academicHistory/courseWork/summary/8", 200, json!({"terms": []})).await;
        let pdf = Path::new(env!("CARGO_MANIFEST_DIR")).join("../laccd/tests/fixtures/transcript.pdf");
        let mut known = source(&[("LAVC", "Valley")], Some(("BIO 1", "Other Science")));
        known.path = pdf.display().to_string();
        let mut unknown = source(&[("LAVC", "Valley")], Some(("BIO 1", "Not A Subject")));
        unknown.path = known.path.clone();
        let profile =
            Profile { coursework: CourseworkFile { transcripts: vec![known, unknown] }, ..Profile::default() };
        let portal = Portal { name: "test".into(), client: Client::new(&server.uri(), "SESSION=x".into()) };
        let plan = portal
            .plan(&profile, Path::new("."), &BTreeSet::from([Section::Coursework]), &lookups())
            .await
            .expect("plan");
        assert!(
            plan.courses.iter().any(|a| a.course.code == "BIO 1" && a.subject == "Other Science"),
            "{}",
            plan.render()
        );
        assert!(plan.findings.iter().any(|f| f.to_string().contains("BIO 1")), "{}", plan.render());
    })
    .await
}

#[tokio::test]
async fn plan_apply_and_pull_surface_each_failed_request() {
    tokio::task::unconstrained(async move {
        let server = support::portal().await;
        let live = Portal { name: "test".into(), client: Client::new(&server.uri(), "SESSION=x".into()) };
        let lk = live.lookups().await.expect("lookups");
        let pulled = live.pull(&lk).await.expect("pull");
        let dead = Portal { name: "test".into(), client: Client::new("not a URL", String::new()) };
        let sections = [Section::Achievements, Section::Licenses, Section::Experiences, Section::Education];
        for section in sections {
            assert!(
                dead.plan(&Profile::default(), Path::new("."), &BTreeSet::from([section]), &lk).await.is_err(),
                "{section:?}"
            );
        }
        let mut profile = Profile::default();
        profile.coursework.transcripts.push(source(&[("LAVC", "Valley")], None));
        assert!(dead.plan(&profile, Path::new("."), &BTreeSet::from([Section::Coursework]), &lk).await.is_err());
        profile.high_school.high_schools.push(school());
        assert!(dead.plan(&profile, Path::new("."), &BTreeSet::from([Section::HighSchool]), &lk).await.is_err());

        let (a, l, x) = (
            pulled.achievements.achievements[0].clone(),
            pulled.licenses.licenses[0].clone(),
            pulled.experiences.experiences[0].clone(),
        );
        let add = CourseAdd {
            college_id: 8,
            college: "Valley".into(),
            college_code: "LAVC".into(),
            target: TermTarget::Existing { id: 10, label: "Fall 2026".into() },
            course: course("A"),
            subject: "Other Science".into(),
        };
        let plans = [
            Plan {
                achievements: Some(Diff { steps: vec![Step::Create(a.clone())], extras: vec![] }),
                ..Plan::default()
            },
            Plan {
                achievements: Some(Diff {
                    steps: vec![Step::Update { id: 1, fields: vec!["name".into()], desired: a }],
                    extras: vec![],
                }),
                ..Plan::default()
            },
            Plan { licenses: Some(Diff { steps: vec![Step::Create(l.clone())], extras: vec![] }), ..Plan::default() },
            Plan {
                licenses: Some(Diff {
                    steps: vec![Step::Update { id: 3, fields: vec!["title".into()], desired: l }],
                    extras: vec![],
                }),
                ..Plan::default()
            },
            Plan {
                experiences: Some(Diff { steps: vec![Step::Create(x.clone())], extras: vec![] }),
                ..Plan::default()
            },
            Plan {
                experiences: Some(Diff {
                    steps: vec![Step::Update { id: 5, fields: vec!["description".into()], desired: x }],
                    extras: vec![],
                }),
                ..Plan::default()
            },
            Plan {
                colleges: vec![admit_liaison::education::CollegeUpdate {
                    college: json!({"id": 8, "stillAttending": true}),
                    changes: vec!["year".into()],
                    degrees: vec![],
                    kept: vec![],
                }],
                ..Plan::default()
            },
            Plan {
                high_school: vec![admit_liaison::high_school::Change::Attendance {
                    row: json!({"id": 9, "termTypeId": 466}),
                    want: school(),
                    was: "portal".into(),
                }],
                ..Plan::default()
            },
            Plan {
                new_colleges: vec![CollegeCreate {
                    name: "Valley".into(),
                    first: ("Fall".into(), "2026".into()),
                    last: ("Fall".into(), "2026".into()),
                    months: (9, 12),
                    still_attending: true,
                }],
                ..Plan::default()
            },
            Plan { courses: vec![add], ..Plan::default() },
            Plan {
                course_edits: vec![CourseEdit {
                    college_id: 8,
                    college: "Valley".into(),
                    term_id: 1,
                    term: "Fall 2026".into(),
                    course_id: 1,
                    from: ("BIO".into(), "Old".into()),
                    to: ("BIO 1".into(), "Biology".into()),
                    subject: Some(("Math".into(), "Other Science".into())),
                    body: json!({"code": "BIO 1"}),
                }],
                ..Plan::default()
            },
        ];
        for plan in &plans {
            assert!(dead.apply(plan, &lk).await.is_err(), "{}", plan.render());
        }
        live.apply(&Plan::default(), &lk).await.expect("empty apply still saves attendance");

        let pulls = [
            "/supportingInfo/achievements",
            "/supportingInfo/licenses/blockName/license2",
            "/supportingInfo/experiences",
            "/academicHistory/colleges",
            "/academicHistory/highSchools",
        ];
        let calls: Vec<_> = pulls.iter().map(|p| ("GET", *p, json!({}))).collect();
        let fresh = MockServer::start().await;
        let portal = Portal { name: "test".into(), client: Client::new(&fresh.uri(), "SESSION=x".into()) };
        for i in 0..calls.len() {
            fail_at(&fresh, &calls, i).await;
            assert!(portal.pull(&lk).await.is_err(), "{}", calls[i].1);
        }
    })
    .await
}

#[tokio::test]
async fn personal_read_stops_at_each_missing_piece() {
    tokio::task::unconstrained(async move {
        let server = MockServer::start().await;
        let client = Client::new(&server.uri(), "SESSION=x".into());
        let lk = lookups();
        let nav = json!({"personalInfo": {"sections": [
            {"name": "biographicInfo", "id": 1},
            {"name": "contactInfo", "id": 2}
        ]}});
        let bio = "/applicationFormSubSectionBlocks/subSection/1";
        let contact = "/applicationFormSubSectionBlocks/subSection/2";
        let err = admit_liaison::personal::read(&client, &lk).await.expect_err("navigation");
        assert!(err.to_string().contains("/navigation"), "{err}");

        let bio_body =
            json!({"questionBlocks": [{"questions": [{"shortFieldName": "birth_state", "value": ["339"]}]}]});
        let cases: &[(&str, &[(&str, Value)])] = &[
            ("biographicInfo", &[("/navigation", json!({"personalInfo": {"sections": []}}))]),
            ("/subSection/1", &[("/navigation", nav.clone())]),
            (
                "contactInfo",
                &[
                    ("/navigation", json!({"personalInfo": {"sections": [{"name": "biographicInfo", "id": 1}]}})),
                    (bio, json!({})),
                ],
            ),
            ("/subSection/2", &[("/navigation", nav.clone()), (bio, json!({}))]),
            ("/authenticatedUser", &[("/navigation", nav.clone()), (bio, json!({})), (contact, json!({}))]),
            (
                "/lookup/counties",
                &[("/navigation", nav), (bio, bio_body), (contact, json!({})), ("/authenticatedUser", json!({}))],
            ),
        ];
        for (label, mounts) in cases {
            server.reset().await;
            for (endpoint, body) in *mounts {
                reply(&server, "GET", endpoint, 200, body.clone()).await;
            }
            let err = admit_liaison::personal::read(&client, &lk).await.expect_err(label);
            assert!(err.to_string().contains(label), "{label}: {err}");
        }
    })
    .await
}
