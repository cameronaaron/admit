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
        license_types: table("license type", &[(1, "Certification")]),
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

fn license(kind: LicenseKind) -> License {
    License {
        kind,
        title: "BLS".into(),
        organization: "AHA".into(),
        description: None,
        valid_until: None,
        ids: Default::default(),
    }
}

#[tokio::test]
async fn live_and_save_report_lookup_errors() {
    tokio::task::unconstrained(async move {
        let server = MockServer::start().await;
        let c = Client::new(&server.uri(), "SESSION=x".into());
        let err = live(&c).await.expect_err("missing list");
        assert!(err.to_string().contains("licenses"), "{err}");

        let err = save(&c, &lookups(), None, &license(LicenseKind::Licensure)).await.expect_err("type");
        assert!(err.to_string().contains("Licensure"), "{err}");

        let body = model(&license(LicenseKind::Certification), 0, &lookups()).expect("model");
        assert_eq!(body["id"], 0);
        let body = model(&license(LicenseKind::Certification), 9, &lookups()).expect("model");
        assert_eq!(body["id"], 9);

        Mock::given(method("POST"))
            .and(path("/applicant/api/supportingInfo/licenses"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
            .mount(&server)
            .await;
        save(&c, &lookups(), Some(9), &license(LicenseKind::Certification)).await.expect("save");
        save(&c, &lookups(), None, &license(LicenseKind::Certification)).await.expect("create");
    })
    .await
}
