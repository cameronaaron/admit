//! `drift::fetch` and `audit::recommendations` over the fake portal: what
//! they read, and that each call's failure is its own.
mod support;
use admit_liaison::{Client, audit, drift};
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

async fn fail(s: &MockServer, verb: &str, endpoint: &str) {
    Mock::given(method(verb))
        .and(path(format!("/applicant/api{endpoint}")))
        .respond_with(ResponseTemplate::new(500).set_body_string("down"))
        .with_priority(1)
        .mount(s)
        .await;
}

fn client(s: &MockServer) -> Client {
    Client::new(&s.uri(), "SESSION=test".into())
}

#[tokio::test]
async fn drift_records_every_watched_endpoint_and_a_rerun_matches() {
    tokio::task::unconstrained(async move {
        let s = support::portal().await;
        // One college, so its term summary is read too.
        for (p, body) in [
            ("/applicant/api/academicHistory/courseWork", json!([{"id": 9, "college": "School"}])),
            ("/applicant/api/academicHistory/courseWork/summary/9", json!({"terms": [{"id": 1, "courses": []}]})),
        ] {
            Mock::given(method("GET")).and(path(p)).respond_with(ResponseTemplate::new(200).set_body_json(body)).with_priority(1).mount(&s).await;
        }
        let live = drift::fetch(&client(&s)).await.expect("fetch");
        let mut want: Vec<&str> = drift::GETS.to_vec();
        want.extend([drift::SEARCH, drift::EVALUATIONS, drift::PREREQUISITES, drift::QUESTIONS, drift::DOCUMENTS]);
        want.extend([drift::COURSE_SUMMARY, drift::EXPERIENCE_BLOCKS, drift::HIGH_SCHOOL_INFO, drift::PERSONAL_BLOCKS]);
        want.sort_unstable();
        assert_eq!(live.keys().map(String::as_str).collect::<Vec<_>>(), want);
        // The templated reads the writes are built from are watched too.
        for t in [drift::COURSE_SUMMARY, drift::EXPERIENCE_BLOCKS, drift::HIGH_SCHOOL_INFO, drift::PERSONAL_BLOCKS] {
            assert!(!live[t].is_empty(), "{t} read nothing on the fake portal");
        }

        // A section's reads alone: just those, the id lists fetched but not returned.
        let reads = drift::reads_for(admit_liaison::Section::Experiences);
        let some = drift::fetch_only(&client(&s), Some(reads)).await.expect("some");
        assert_eq!(some.keys().map(String::as_str).collect::<std::collections::BTreeSet<_>>(), reads.iter().copied().collect());
        assert_eq!(some[drift::EXPERIENCE_BLOCKS], live[drift::EXPERIENCE_BLOCKS]);
        assert!(drift::check_only(&live, &some).is_empty(), "a subset says nothing of what it didn't read");
        let mut unrecorded = live.clone();
        unrecorded.remove(drift::EXPERIENCE_BLOCKS);
        assert!(drift::check_only(&unrecorded, &some).iter().any(|f| f.is_violation() && f.to_string().contains("never recorded")));
        // Every section's reads are watched endpoints.
        for sec in admit_liaison::Section::ALL {
            for r in drift::reads_for(sec) {
                assert!(live.contains_key(*r), "{sec:?} reads {r}, which drift doesn't watch");
            }
        }
        assert_eq!(live[drift::EVALUATIONS]["$.evaluationTypes[].minRequired"], "number");
        assert!(drift::check(&live, &drift::fetch(&client(&s)).await.expect("again")).is_empty());
        assert!(live[drift::QUESTIONS].is_empty(), "no program here has questions");
        assert!(live[drift::DOCUMENTS].is_empty(), "nor documents");

        // A program with prerequisites and questions: both are read and merged.
        // (Failures below go last-read first, each left mounted.)
        Mock::given(method("GET"))
            .and(path("/applicant/api/getProgramNavigation"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([{"programSelectedId": 1,
                "selectedProgramHasPreReqSection": true, "subSections": [{"name": "Questions"}, {"name": "Documents"}]}])))
            .with_priority(1)
            .mount(&s)
            .await;
        for (p, body) in [
            ("/applicant/api/programMaterials/1/prerequisites", json!({"prerequisites": [{"id": 1, "minCredits": 4}]})),
            ("/applicant/api/programMaterials/1/questions", json!({"questionGroups": [{"questions": [{"id": 2}]}]})),
            ("/applicant/api/programMaterials/1/documents", json!({"sectionId": 3, "documents": [{"id": 4, "attachment": null}]})),
        ] {
            Mock::given(method("GET"))
                .and(path(p))
                .respond_with(ResponseTemplate::new(200).set_body_json(body))
                .mount(&s)
                .await;
        }
        let live = drift::fetch(&client(&s)).await.expect("programs");
        // Documents' reads alone skip the program's other reads.
        let docs = drift::fetch_only(&client(&s), Some(drift::reads_for(admit_liaison::Section::Documents))).await.expect("docs");
        assert_eq!(docs[drift::DOCUMENTS], live[drift::DOCUMENTS]);
        assert!(!docs.contains_key(drift::EVALUATIONS));
        assert_eq!(live[drift::PREREQUISITES]["$.prerequisites[].minCredits"], "number");
        assert_eq!(live[drift::QUESTIONS]["$.questionGroups[].questions[].id"], "number");
        assert_eq!(live[drift::DOCUMENTS]["$.documents[].attachment"], "null");
        for (p, what) in [
            ("/applicant/api/programMaterials/1/documents", "documents"),
            ("/applicant/api/programMaterials/1/questions", "questions"),
            ("/applicant/api/programMaterials/1/prerequisites", "prerequisites"),
        ] {
            Mock::given(method("GET"))
                .and(path(p))
                .respond_with(ResponseTemplate::new(500))
                .with_priority(1)
                .mount(&s)
                .await;
            let e = drift::fetch(&client(&s)).await.expect_err(what).to_string();
            assert!(e.contains(p.trim_start_matches("/applicant/api")), "{e}");
        }
        // A navigation listing no personal section reads none.
        Mock::given(method("GET"))
            .and(path("/applicant/api/navigation"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
            .with_priority(1)
            .mount(&s)
            .await;
        let none = drift::fetch_only(&client(&s), Some(&[drift::PERSONAL_BLOCKS])).await.expect("personal");
        assert!(none[drift::PERSONAL_BLOCKS].is_empty());
    })
    .await;
}

#[tokio::test]
async fn each_drift_and_recommendation_read_fails_on_its_own_call() {
    tokio::task::unconstrained(async move {
        for (verb, endpoint) in [
            ("GET", "/lookup/year"),
            ("POST", "/search/filteredQueries"),
            ("GET", "/programMaterials/evaluations/programSelected/1"),
            ("GET", "/questionBlock/supportingInfo/experiences/5"),
            ("GET", "/academicHistory/highSchoolInfo/42"),
            ("GET", "/applicationFormSubSectionBlocks/subSection/24164"),
            ("GET", "/academicHistory/courseWork/summary/9"),
        ] {
            let s = support::portal().await;
            Mock::given(method("GET"))
                .and(path("/applicant/api/academicHistory/courseWork"))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!([{"id": 9}])))
                .with_priority(1)
                .mount(&s)
                .await;
            Mock::given(method("GET"))
                .and(path("/applicant/api/academicHistory/courseWork/summary/9"))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({"terms": []})))
                .with_priority(2)
                .mount(&s)
                .await;
            fail(&s, verb, endpoint).await;
            let e = drift::fetch(&client(&s)).await.expect_err(endpoint).to_string();
            assert!(e.contains(endpoint) && e.contains("HTTP 500"), "{e}");
        }
        for (verb, endpoint) in [
            ("GET", "/programsSelected"),
            ("POST", "/search/filteredQueries"),
            ("GET", "/getProgramNavigation"),
            ("GET", "/programMaterials/evaluations/programSelected/1"),
        ] {
            let s = support::portal().await;
            fail(&s, verb, endpoint).await;
            let e = audit::recommendations(&client(&s)).await.expect_err(endpoint).to_string();
            assert!(e.contains(endpoint) && e.contains("HTTP 500"), "{e}");
        }
    })
    .await;
}

#[tokio::test]
async fn recommendations_join_each_program_to_its_deadline_by_selected_id() {
    tokio::task::unconstrained(async move {
        let s = support::portal().await;
        Mock::given(method("POST"))
            .and(path("/applicant/api/search/filteredQueries"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"programs": [
                {"programSelectedId": 1, "deadline": "2026-11-02", "deadlineDisplay": "Nov 1, 2026, 11:59 PM EST",
                 "submissionStatus": "In Progress"},
                {"programSelectedId": 7, "deadline": "2026-10-20", "submissionStatus": "In Progress"}]})))
            .with_priority(1)
            .mount(&s)
            .await;
        let rows = audit::recommendations(&client(&s)).await.expect("recs");
        assert_eq!(
            rows,
            vec![audit::Recs {
                label: "University - Nursing".into(),
                need: 1,
                requested: 0,
                received: 0,
                deadline: Some(time::macros::date!(2026 - 11 - 01)),
            }]
        );
    })
    .await;
}
