//! Program questions over the fake portal: the impact check and the save
//! must carry the bodies the browser sent (Yale, 2026-10-07) exactly, and a
//! reported impact must stop the save.
mod support;
use admit_domain::{Profile, QuestionsFile};
use admit_liaison::{Client, Portal, Section, questions};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::path::Path;
use wiremock::matchers::{body_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const LABEL: &str = "Yale University - GEPN";
const SAVE: &str = "/applicant/api/programMaterials/79439764/questionsPartialSave";
const IMPACT: &str = "/applicant/api/extendedProfile/applicationProgramSelectionImpact";

fn fixture() -> Value {
    serde_json::from_str(include_str!("../src/sections/tests/fixtures/yale_questions.json")).expect("fixture")
}

async fn mount(s: &MockServer, verb: &str, p: &str, status: u16, body: Value) {
    Mock::given(method(verb))
        .and(path(p))
        .respond_with(ResponseTemplate::new(status).set_body_json(body))
        .with_priority(2)
        .mount(s)
        .await;
}

async fn server(fx: &Value) -> MockServer {
    let s = support::portal().await;
    let nav = json!([{"orgName": "Yale University", "programName": "GEPN", "programSelectedId": 79439764,
        "subSections": [{"name": "Questions"}, {"name": "Documents"}]},
        {"orgName": "University", "programName": "Nursing", "programSelectedId": 1, "subSections": [{"name": "Documents"}]}]);
    mount(&s, "GET", "/applicant/api/getProgramNavigation", 200, nav).await;
    mount(&s, "GET", "/applicant/api/programMaterials/79439764/questions", 200, fx["before"].clone()).await;
    s
}

fn profile(fx: &Value) -> Profile {
    let after = questions::LiveProgram { id: 79439764, label: LABEL.into(), doc: fx["after"].clone() };
    Profile { questions: QuestionsFile { programs: questions::pull(&[after], "test") }, ..Default::default() }
}

async fn plan_and_apply(s: &MockServer, fx: &Value) -> admit_liaison::Result<()> {
    let p = Portal { name: "test".into(), client: Client::new(&s.uri(), "SESSION=test".into()) };
    let lk = p.lookups().await.expect("lookups");
    let only: BTreeSet<Section> = [Section::Questions].into();
    let plan = p.plan(&profile(fx), Path::new("."), &only, &lk).await.expect("plan");
    assert_eq!(plan.change_count(), 7, "{}", plan.render());
    assert_eq!(plan.sections(), [Section::Questions].into(), "what the pre-write shape check covers");
    assert!(plan.render().contains(&format!("questions: 7 answer(s) for {LABEL}")), "{}", plan.render());
    p.apply(&plan, &lk).await
}

#[tokio::test]
async fn the_save_follows_a_clean_impact_check_with_the_browsers_bodies() {
    tokio::task::unconstrained(async move {
        let fx = fixture();
        let s = server(&fx).await;
        Mock::given(method("POST"))
            .and(path(IMPACT))
            .and(body_json(&fx["impact"]["request"]))
            .respond_with(ResponseTemplate::new(200).set_body_json(&fx["impact"]["response"]))
            .with_priority(1)
            .expect(1)
            .mount(&s)
            .await;
        Mock::given(method("POST"))
            .and(path(SAVE))
            .and(body_json(&fx["save"]["request"]))
            .respond_with(ResponseTemplate::new(200).set_body_json(&fx["save"]["response"]))
            .with_priority(1)
            .expect(1)
            .mount(&s)
            .await;
        plan_and_apply(&s, &fx).await.expect("apply");
    })
    .await;
}

#[tokio::test]
async fn a_reported_impact_saves_nothing_and_each_call_fails_on_its_own() {
    tokio::task::unconstrained(async move {
        let fx = fixture();
        let s = server(&fx).await;
        let mut r = fx["impact"]["response"].clone();
        r["hasImpacts"] = json!(true);
        mount(&s, "POST", IMPACT, 200, r).await;
        Mock::given(method("POST")).and(path(SAVE)).respond_with(ResponseTemplate::new(200)).expect(0).mount(&s).await;
        let e = plan_and_apply(&s, &fx).await.expect_err("impact").to_string();
        assert!(e.contains("would affect other program selections") && e.contains("nothing saved"), "{e}");

        for (verb, p) in [("POST", IMPACT), ("POST", SAVE)] {
            let s = server(&fx).await;
            mount(&s, "POST", IMPACT, 200, fx["impact"]["response"].clone()).await;
            Mock::given(method(verb))
                .and(path(p))
                .respond_with(ResponseTemplate::new(500))
                .with_priority(1)
                .mount(&s)
                .await;
            let e = plan_and_apply(&s, &fx).await.expect_err(p).to_string();
            assert!(e.contains(p.trim_start_matches("/applicant/api")) && e.contains("HTTP 500"), "{e}");
        }
        let s = server(&fx).await;
        Mock::given(method("GET"))
            .and(path("/applicant/api/programMaterials/79439764/questions"))
            .respond_with(ResponseTemplate::new(500))
            .with_priority(1)
            .mount(&s)
            .await;
        let c = Client::new(&s.uri(), "SESSION=test".into());
        assert!(questions::live(&c).await.expect_err("get").to_string().contains("HTTP 500"));
        let s = server(&fx).await;
        Mock::given(method("GET"))
            .and(path("/applicant/api/getProgramNavigation"))
            .respond_with(ResponseTemplate::new(500))
            .with_priority(1)
            .mount(&s)
            .await;
        let c = Client::new(&s.uri(), "SESSION=test".into());
        assert!(questions::live(&c).await.expect_err("nav").to_string().contains("getProgramNavigation"));
    })
    .await;
}

#[tokio::test]
async fn a_finished_form_is_saved_then_marked_complete_with_an_empty_put() {
    tokio::task::unconstrained(async move {
        let fx: Value =
            serde_json::from_str(include_str!("../src/sections/tests/fixtures/davis_questions.json")).expect("fixture");
        let complete = format!("/applicant/api{}", fx["complete"]["path"].as_str().expect("path"));
        let run = |fail: Option<&'static str>| {
            let fx = fx.clone();
            let complete = complete.clone();
            async move {
                let s = support::portal().await;
                let nav = json!([{"orgName": "University of California - Davis", "programName": "MEPN",
                    "programSelectedId": 79437728, "subSections": [{"name": "Questions"}]}]);
                mount(&s, "GET", "/applicant/api/getProgramNavigation", 200, nav).await;
                mount(&s, "GET", "/applicant/api/programMaterials/79437728/questions", 200, fx["before"].clone()).await;
                mount(&s, "POST", IMPACT, 200, fx["impact"]["response"].clone()).await;
                if fail != Some("save") {
                    Mock::given(method("POST"))
                        .and(path("/applicant/api/programMaterials/79437728/questions"))
                        .and(body_json(&fx["save"]["request"]))
                        .respond_with(ResponseTemplate::new(200).set_body_json(&fx["save"]["response"]))
                        .with_priority(1)
                        .expect(1)
                        .mount(&s)
                        .await;
                }
                if fail.is_none() {
                    Mock::given(method("PUT"))
                        .and(path(complete.as_str()))
                        .and(wiremock::matchers::body_string(""))
                        .respond_with(ResponseTemplate::new(200).set_body_json(&fx["complete"]["response"]))
                        .with_priority(1)
                        .expect(1)
                        .mount(&s)
                        .await;
                }
                Mock::given(method("POST"))
                    .and(path("/applicant/api/programMaterials/79437728/questionsPartialSave"))
                    .respond_with(ResponseTemplate::new(200))
                    .expect(0)
                    .mount(&s)
                    .await;
                for (what, verb, p) in [
                    ("save", "POST", "/applicant/api/programMaterials/79437728/questions"),
                    ("put", "PUT", complete.as_str()),
                ] {
                    if fail == Some(what) {
                        Mock::given(method(verb))
                            .and(path(p))
                            .respond_with(ResponseTemplate::new(500))
                            .with_priority(1)
                            .mount(&s)
                            .await;
                    }
                }
                let after = questions::LiveProgram {
                    id: 79437728,
                    label: "University of California - Davis - MEPN".into(),
                    doc: fx["after"].clone(),
                };
                let profile = Profile {
                    questions: QuestionsFile { programs: questions::pull(&[after], "test") },
                    ..Default::default()
                };
                let p = Portal { name: "test".into(), client: Client::new(&s.uri(), "SESSION=test".into()) };
                let lk = p.lookups().await.expect("lookups");
                let only: BTreeSet<Section> = [Section::Questions].into();
                let plan = p.plan(&profile, Path::new("."), &only, &lk).await.expect("plan");
                p.apply(&plan, &lk).await.map_err(|e| e.to_string())
            }
        };
        run(None).await.expect("saved and completed");
        assert!(run(Some("save")).await.expect_err("save").contains("POST /programMaterials/79437728/questions"));
        assert!(run(Some("put")).await.expect_err("put").contains("PUT /updateProgramMaterialsStatus"));
    })
    .await;
}
