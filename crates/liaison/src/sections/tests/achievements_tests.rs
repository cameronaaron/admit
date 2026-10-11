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
        achievement_types: table("achievement type", &[(1, "Awards")]),
        experience_types: table("experience type", &[]),
        employment_status: table("employment status", &[]),
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

fn honor() -> Achievement {
    Achievement {
        kind: AchievementKind::Honor,
        name: "Dean's List".into(),
        organization: None,
        date: None,
        state: "California".into(),
        country: "United States".into(),
        description: None,
        ids: Default::default(),
    }
}

#[tokio::test]
async fn live_and_update_report_lookup_and_transport_errors() {
    tokio::task::unconstrained(async move {
        let server = MockServer::start().await;
        let c = Client::new(&server.uri(), "SESSION=x".into());
        let err = live(&c, &lookups()).await.expect_err("missing list");
        assert!(err.to_string().contains("achievements"), "{err}");

        let mut abroad = honor();
        abroad.country = "Canada".into();
        let err = payload(&abroad, None, &lookups()).expect_err("country");
        assert!(err.to_string().contains("Canada"), "{err}");

        let err = update(&c, &lookups(), 4, &honor()).await.expect_err("honor type");
        assert!(err.to_string().contains("Honors"), "{err}");

        Mock::given(method("PUT"))
            .and(path("/applicant/api/supportingInfo/achievements/4"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;
        let mut lk = lookups();
        lk.achievement_types = table("achievement type", &[(2, "Honors")]);
        let err = update(&c, &lk, 4, &honor()).await.expect_err("put");
        assert!(err.to_string().contains("500"), "{err}");
    })
    .await
}
