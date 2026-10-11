//! Recommendation requests over the fake portal: the email check comes
//! first, a refused check posts nothing, and the request carries the
//! form's body.
mod support;
use admit_domain::{Profile, Recommender, ReferencesFile};
use admit_liaison::{Client, Portal, Section};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::path::Path;
use wiremock::matchers::{body_json, method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

const EVALS: &str = "/applicant/api/programMaterials/evaluations/programSelected/79439764";
const VERIFY: &str = "/applicant/api/emailVerification/pooledEvaluations/email/jane@school.example/evaluationType/Letter%20Upload%20Only";

fn section() -> Value {
    json!({"sectionId": 2013254, "evaluationTypes": [{"minRequired": 3, "maxAllowed": 4,
        "type": "Letter Upload Only", "evaluationTypeConstant": "Letter Upload Only",
        "evaluationTypeDisplayName": null}], "data": {"evaluations": []}, "evalSubmitDeadline": "2028-02-10"})
}

fn profile() -> Profile {
    Profile {
        references: ReferencesFile {
            consent: true,
            recommenders: vec![Recommender {
                first_name: "Jane".into(),
                last_name: "Doe".into(),
                email: "jane@school.example".into(),
                due: "2026-10-25".into(),
                waive: false,
                message: "Thank you.".into(),
                programs: vec!["Yale University - GEPN".into()],
                kind: None,
            }],
        },
        ..Default::default()
    }
}

/// The path exactly as sent (wiremock's `path()` misses one with `%20`).
fn raw_path(p: &'static str) -> impl Fn(&Request) -> bool + Send + Sync + 'static {
    move |r: &Request| r.url.path() == p
}

async fn server() -> MockServer {
    let s = support::portal().await;
    let nav = json!([{"orgName": "Yale University", "programName": "GEPN", "programSelectedId": 79439764,
        "status": "INCOMPLETE", "subSections": [{"name": "Documents"}, {"name": "Evaluation Types"}]},
        {"orgName": "Other", "programName": "None", "programSelectedId": 5, "subSections": [{"name": "Documents"}]}]);
    for (p, body) in [("/applicant/api/getProgramNavigation", nav), (EVALS, section())] {
        Mock::given(method("GET"))
            .and(path(p))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .with_priority(2)
            .mount(&s)
            .await;
    }
    s
}

async fn run(s: &MockServer) -> admit_liaison::Result<String> {
    let p = Portal { name: "test".into(), client: Client::new(&s.uri(), "SESSION=test".into()) };
    let lk = p.lookups().await.expect("lookups");
    let only: BTreeSet<Section> = [Section::References].into();
    let plan = p.plan(&profile(), Path::new("."), &only, &lk).await?;
    p.apply(&plan, &lk).await?;
    Ok(plan.render())
}

#[tokio::test]
async fn the_email_check_then_the_request_with_the_forms_body() {
    tokio::task::unconstrained(async move {
        let s = server().await;
        Mock::given(method("GET"))
            .and(raw_path(VERIFY))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
            .expect(1)
            .mount(&s)
            .await;
        Mock::given(method("POST"))
            .and(path(EVALS))
            .and(body_json(json!({"committeeEvaluation": false, "evaluationType": "Letter Upload Only",
                "evaluationTypeDisplayName": "Letter Upload Only",
                "permissionForSchoolsToContactReference": true, "permissionToContactReference": true,
                "waiverOfEvaluation": false,
                "evaluator": {"dueDate": "2026-10-25", "emailAddress": "jane@school.example", "evalFirstName": "Jane",
                    "evalLastName": "Doe", "message": "Thank you.", "strDueDate": "2026-10-25"},
                "reSendEvaluatorEmail": false})))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
            .with_priority(1)
            .expect(1)
            .mount(&s)
            .await;
        let shown = run(&s).await.expect("apply");
        assert_eq!(
            shown,
            "references: 1 request(s), each emails the recommender\n  + Jane Doe <jane@school.example> for Yale University - GEPN (due 2026-10-25, waived: false)\n"
        );
    })
    .await;
}

#[tokio::test]
async fn a_refused_email_check_requests_nothing_and_each_call_fails_on_its_own() {
    tokio::task::unconstrained(async move {
        let s = server().await;
        Mock::given(method("GET")).and(raw_path(VERIFY)).respond_with(ResponseTemplate::new(400)).mount(&s).await;
        Mock::given(method("POST"))
            .and(path(EVALS))
            .respond_with(ResponseTemplate::new(200))
            .with_priority(1)
            .expect(0)
            .mount(&s)
            .await;
        let e = run(&s).await.expect_err("refused").to_string();
        assert!(e.contains("the portal refused jane@school.example") && e.contains("nothing requested"), "{e}");

        let s = server().await;
        Mock::given(method("GET")).and(raw_path(VERIFY)).respond_with(ResponseTemplate::new(200)).mount(&s).await;
        Mock::given(method("POST"))
            .and(path(EVALS))
            .respond_with(ResponseTemplate::new(500))
            .with_priority(1)
            .mount(&s)
            .await;
        let e = run(&s).await.expect_err("post").to_string();
        assert!(e.contains("POST /programMaterials/evaluations/programSelected/79439764: HTTP 500"), "{e}");

        for p in [EVALS, "/applicant/api/getProgramNavigation"] {
            let s = server().await;
            Mock::given(method("GET"))
                .and(path(p))
                .respond_with(ResponseTemplate::new(500))
                .with_priority(1)
                .mount(&s)
                .await;
            let e = run(&s).await.expect_err(p).to_string();
            assert!(e.contains(p.trim_start_matches("/applicant/api")), "{e}");
        }
    })
    .await;
}
