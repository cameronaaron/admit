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
        countries: table("country", &[(222, "United States")]),
        us_states: table("US state", &[(339, "California")]),
        achievement_types: table("achievement type", &[]),
        experience_types: table(
            "experience type",
            &[(1, "Employment"), (2, "Patient/Healthcare Experience"), (3, "Community Enrichment")],
        ),
        employment_status: table("employment status", &[(1, "Part time")]),
        license_types: table("license type", &[]),
        degree_types: table("degree type", &[]),
        majors: table("major", &[]),
        months: table("month", &[]),
        years: table("year", &[]),
        terms: table("term", &[]),
        term_types: table("term type", &[]),
        academic_status: table("academic status", &[]),
        subjects: table("subject", &[]),
    }
}

fn job(kind: ExperienceKind, country: &str, status: &str) -> Experience {
    Experience {
        kind,
        organization: "Clinic".into(),
        title: "Aide".into(),
        state: "California".into(),
        country: country.into(),
        start: "2020-01-01".into(),
        end: None,
        current: true,
        status: status.into(),
        hours_per_week: 10,
        weeks: 4,
        total_hours: 40,
        compensated: true,
        volunteer: false,
        description: "Care".into(),
        supervisor: Supervisor::default(),
        ids: Default::default(),
    }
}

#[test]
fn block_writes_skip_a_non_object_and_reject_each_lookup() {
    let lk = lookups();
    let mut b = Blocks::new();
    for key in ["experienceType", "organization", "supervisor", "employmentDates", "employmentDetails"] {
        b.insert(key.into(), json!("leave me"));
    }
    apply_to_blocks(&job(ExperienceKind::Employment, "United States", "Part time"), &mut b, &lk).expect("write");
    assert_eq!(b["experienceType"], json!("leave me"));

    let err = apply_to_blocks(&job(ExperienceKind::Employment, "Canada", "Part time"), &mut Blocks::new(), &lk)
        .expect_err("country");
    assert!(err.to_string().contains("Canada"), "{err}");

    let mut lk = lookups();
    lk.experience_types = table("experience type", &[(1, "Employment")]);
    let err = apply_to_blocks(&job(ExperienceKind::PatientCare, "United States", "Part time"), &mut Blocks::new(), &lk)
        .expect_err("kind");
    assert!(err.to_string().contains("Patient/Healthcare"), "{err}");

    let mut lk = lookups();
    lk.employment_status = table("employment status", &[]);
    let err = apply_to_blocks(&job(ExperienceKind::Employment, "United States", "Temporary"), &mut Blocks::new(), &lk)
        .expect_err("status");
    assert!(err.to_string().contains("Temporary"), "{err}");
}

#[tokio::test]
async fn live_create_and_update_surface_transport_and_lookup_errors() {
    tokio::task::unconstrained(async move {
        let server = MockServer::start().await;
        let c = Client::new(&server.uri(), "SESSION=x".into());
        let lk = lookups();
        let err = live(&c, &lk).await.expect_err("list");
        assert!(err.to_string().contains("experiences"), "{err}");

        Mock::given(method("GET"))
            .and(path("/applicant/api/supportingInfo/experiences"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"experiences": [{"id": 5}]})))
            .mount(&server)
            .await;
        let err = live(&c, &lk).await.expect_err("block");
        assert!(err.to_string().contains("experiences/5"), "{err}");

        Mock::given(method("GET"))
            .and(path("/applicant/api/questionBlock/supportingInfo/experiences/5"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"questionBlocks": []})))
            .mount(&server)
            .await;
        let rows = live(&c, &lk).await.expect("live");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].0, 5);

        let bad = job(ExperienceKind::Employment, "Canada", "Part time");
        let err = create(&c, &lk, &bad).await.expect_err("create");
        assert!(err.to_string().contains("Canada"), "{err}");
        let err = update(&c, &lk, 5, &bad, &Blocks::new()).await.expect_err("update");
        assert!(err.to_string().contains("Canada"), "{err}");

        let good = job(ExperienceKind::Employment, "United States", "Part time");
        Mock::given(method("POST"))
            .and(path("/applicant/api/questionBlock/supportingInfo/experiences"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
            .mount(&server)
            .await;
        create(&c, &lk, &good).await.expect("create");
        Mock::given(method("PUT"))
            .and(path("/applicant/api/questionBlock/supportingInfo/experiences/5"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
            .mount(&server)
            .await;
        update(&c, &lk, 5, &good, &Blocks::new()).await.expect("update");
    })
    .await
}
