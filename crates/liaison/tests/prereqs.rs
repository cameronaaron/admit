//! Prerequisite assignment over the fake portal: the four POSTs must match
//! the bodies the browser sent (Yale, 2026-10-07) exactly.
mod support;
use admit_domain::{CourseworkFile, PrerequisitesFile, Profile};
use admit_liaison::{Client, Portal, Section, prereqs};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::path::Path;
use wiremock::matchers::{body_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const PROGRAM: &str = "/applicant/api/programMaterials/79439764/prerequisites";

fn fixture() -> Value {
    serde_json::from_str(include_str!("../src/sections/tests/fixtures/yale_prereqs.json")).expect("fixture")
}

fn profile() -> Profile {
    let rulings = r#"
[[program]]
name = "Yale University - Graduate Entry Prespecialty in Nursing (GEPN) / Master of Science in Nursing (MSN) Program"
portal = "test"
[[program.requirement]]
name = "Human Anatomy & Physiology I or Anatomy w/Lab"
courses = [{ college = "LACC", code = "ANATOMY 001" }]
[[program.requirement]]
name = "Human Anatomy & Physiology II or Physiology w/Lab"
courses = [{ college = "LAHC", code = "PHYSIOL 001" }]
[[program.requirement]]
name = "General Chemistry w/Lab"
courses = [{ college = "LAVC", code = "CHEM 051" }]
[[program.requirement]]
name = "Microbiology w/Lab"
courses = [{ college = "LACC", code = "MICRO 020" }]
"#;
    let coursework = r#"
[[transcript]]
path = "none.pdf"
parser = "laccd"
summer_term = "Summer 2"
academic_status = "Post-Baccalaureate"
skip = []
[transcript.colleges]
LACC = "LOS ANGELES CITY COLLEGE"
LAHC = "LOS ANGELES HARBOR COLLEGE"
LAVC = "LOS ANGELES VALLEY COLLEGE"
"#;
    Profile {
        prerequisites: toml::from_str::<PrerequisitesFile>(rulings).expect("rulings"),
        coursework: toml::from_str::<CourseworkFile>(coursework).expect("coursework"),
        ..Default::default()
    }
}

async fn mount(s: &MockServer, verb: &str, p: &str, status: u16, body: Value) {
    Mock::given(method(verb))
        .and(path(p))
        .respond_with(ResponseTemplate::new(status).set_body_json(body))
        .with_priority(2)
        .mount(s)
        .await;
}

/// The fake portal plus Yale (with a prerequisites section) and a program without one.
async fn server(fx: &Value) -> MockServer {
    let s = support::portal().await;
    let nav = json!([
        {"orgName": "Yale University", "programName": "Graduate Entry Prespecialty in Nursing (GEPN) / Master of Science in Nursing (MSN) Program",
         "programSelectedId": 79439764, "selectedProgramHasPreReqSection": true},
        {"orgName": "University", "programName": "Nursing", "programSelectedId": 1, "selectedProgramHasPreReqSection": false}]);
    mount(&s, "GET", "/applicant/api/getProgramNavigation", 200, nav).await;
    mount(&s, "GET", PROGRAM, 200, fx["get"].clone()).await;
    mount(&s, "GET", "/applicant/api/programMaterials/prerequisites/transcripts", 200, fx["transcripts"].clone()).await;
    s
}

/// The client and the four planned assignments.
async fn assign(s: &MockServer) -> (Client, Vec<prereqs::Assign>) {
    let c = portal(s).client;
    let (live, pool) = prereqs::live(&c).await.expect("live");
    let (a, _) =
        prereqs::plan(&profile().prerequisites.programs, "test", &profile().coursework.colleges(), &live, &pool);
    assert_eq!(a.len(), 4);
    (c, a)
}

fn portal(s: &MockServer) -> Portal {
    Portal { name: "test".into(), client: Client::new(&s.uri(), "SESSION=test".into()) }
}

#[tokio::test]
async fn apply_posts_each_assignment_as_the_form_did() {
    tokio::task::unconstrained(async move {
        let fx = fixture();
        let s = server(&fx).await;
        for sent in fx["posts"].as_array().expect("posts") {
            Mock::given(method("POST"))
                .and(path(PROGRAM))
                .and(body_json(&sent["request"]))
                .respond_with(ResponseTemplate::new(200).set_body_json(&sent["response"]))
                .with_priority(1)
                .expect(1)
                .mount(&s)
                .await;
        }
        let p = portal(&s);
        let lk = p.lookups().await.expect("lookups");
        let only: BTreeSet<Section> = [Section::Prerequisites].into();
        let plan = p.plan(&profile(), Path::new("."), &only, &lk).await.expect("plan");
        assert_eq!(plan.change_count(), 4, "{}", plan.render());
        assert!(plan.render().contains("prerequisites: 4 course(s) to assign"), "{}", plan.render());
        p.apply(&plan, &lk).await.expect("apply");
        // wiremock verifies on drop that each exact body arrived once.
    })
    .await;
}

#[tokio::test]
async fn each_prerequisite_call_fails_on_its_own() {
    tokio::task::unconstrained(async move {
        let fx = fixture();
        for (verb, p) in [("GET", PROGRAM), ("GET", "/applicant/api/programMaterials/prerequisites/transcripts")] {
            let s = server(&fx).await;
            Mock::given(method(verb))
                .and(path(p))
                .respond_with(ResponseTemplate::new(500))
                .with_priority(1)
                .mount(&s)
                .await;
            let e = prereqs::live(&portal(&s).client).await.expect_err(p).to_string();
            assert!(e.contains(p.trim_start_matches("/applicant/api")) && e.contains("HTTP 500"), "{e}");
        }
        let s = server(&fx).await;
        let (c, a) = assign(&s).await;
        Mock::given(method("GET"))
            .and(path(PROGRAM))
            .respond_with(ResponseTemplate::new(500))
            .with_priority(1)
            .mount(&s)
            .await;
        assert!(prereqs::apply(&c, &a).await.expect_err("get").to_string().contains("GET"));
        // Through Portal::apply, as `admit apply` runs it.
        let s = server(&fx).await;
        mount(&s, "POST", PROGRAM, 500, json!({})).await;
        let p = portal(&s);
        let lk = p.lookups().await.expect("lookups");
        let only: BTreeSet<Section> = [Section::Prerequisites].into();
        let plan = p.plan(&profile(), Path::new("."), &only, &lk).await.expect("plan");
        assert!(p.apply(&plan, &lk).await.expect_err("post").to_string().contains("POST"));
    })
    .await;
}
