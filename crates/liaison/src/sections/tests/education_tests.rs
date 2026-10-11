use super::*;
use crate::lookup::{Lookups, Table};
use serde_json::Value;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn table(name: &str, rows: &[(i64, &str)]) -> Table {
    Table::from_rows(name, &Value::Array(rows.iter().map(|(id, t)| json!({"id": id, "text": t})).collect()))
}

fn lookups(degrees: &[(i64, &str)], months: &[(i64, &str)], years: &[(i64, &str)], majors: &[(i64, &str)]) -> Lookups {
    Lookups {
        countries: table("country", &[]),
        us_states: table("US state", &[]),
        achievement_types: table("achievement type", &[]),
        experience_types: table("experience type", &[]),
        employment_status: table("employment status", &[]),
        license_types: table("license type", &[]),
        degree_types: table("degree type", degrees),
        majors: table("major", majors),
        months: table("month", months),
        years: table("year", years),
        terms: table("term", &[]),
        term_types: table("term type", &[]),
        academic_status: table("academic status", &[]),
        subjects: table("subject", &[]),
    }
}

fn degree(second: Option<&str>, minor: Option<&str>) -> Degree {
    Degree {
        kind: "Associate of Arts".into(),
        major: "Biology".into(),
        second_major: second.map(str::to_string),
        minor: minor.map(str::to_string),
        status: DegreeStatus::Awarded,
        month: "May".into(),
        year: 2026,
        note: None,
    }
}

fn full() -> Lookups {
    lookups(&[(1, "Associate of Arts")], &[(8, "May")], &[(12216, "2026")], &[(3, "Biology"), (4, "Chemistry")])
}

fn update(d: Degree) -> CollegeUpdate {
    CollegeUpdate {
        college: json!({"id": 4, "college": "Valley", "stillAttending": true}),
        changes: vec![],
        degrees: vec![(Some(9), d)],
        kept: vec![],
    }
}

#[test]
fn degree_body_fails_at_each_lookup() {
    let err = degree_body(None, &degree(None, None), &lookups(&[], &[(8, "May")], &[(1, "2026")], &[(3, "Biology")]))
        .expect_err("type");
    assert!(err.to_string().contains("Associate"), "{err}");

    let err = degree_body(
        Some(1),
        &degree(None, None),
        &lookups(&[(1, "Associate of Arts")], &[], &[(1, "2026")], &[(3, "Biology")]),
    )
    .expect_err("month");
    assert!(err.to_string().contains("May"), "{err}");

    let err = degree_body(
        None,
        &degree(None, None),
        &lookups(&[(1, "Associate of Arts")], &[(8, "May")], &[], &[(3, "Biology")]),
    )
    .expect_err("year");
    assert!(err.to_string().contains("2026"), "{err}");

    let err = degree_body(
        None,
        &degree(None, None),
        &lookups(&[(1, "Associate of Arts")], &[(8, "May")], &[(1, "2026")], &[]),
    )
    .expect_err("major");
    assert!(err.to_string().contains("Biology"), "{err}");

    let lk = lookups(&[(1, "Associate of Arts")], &[(8, "May")], &[(1, "2026")], &[(3, "Biology")]);
    let err = degree_body(None, &degree(Some("Chemistry"), None), &lk).expect_err("second");
    assert!(err.to_string().contains("Chemistry"), "{err}");

    let err = degree_body(None, &degree(None, Some("Art")), &full()).expect_err("minor");
    assert!(err.to_string().contains("Art"), "{err}");

    let mut done = degree(Some("Chemistry"), Some("Art"));
    done.status = DegreeStatus::Expected;
    let lk = lookups(
        &[(1, "Associate of Arts")],
        &[(8, "May")],
        &[(12216, "2026")],
        &[(3, "Biology"), (4, "Chemistry"), (5, "Art")],
    );
    let fresh = degree_body(None, &degree(None, None), &full()).expect("new degree");
    assert!(fresh.get("id").is_none(), "{fresh}");

    let body = degree_body(Some(3), &done, &lk).expect("degree");
    assert_eq!(body["degreeStatusId"], 430);
    assert_eq!(body["id"], 3);
    assert!(body.get("secondMajorId").is_some() && body.get("minorId").is_some(), "{body}");
    let saved = college_body(&update(done), &lk).expect("college");
    assert_eq!(saved["isDegree"], "yes");
}

#[tokio::test]
async fn live_and_apply_propagate_lookup_errors() {
    tokio::task::unconstrained(async move {
        let server = MockServer::start().await;
        let c = Client::new(&server.uri(), "SESSION=x".into());
        let err = live(&c).await.expect_err("colleges");
        assert!(err.to_string().contains("colleges"), "{err}");

        let err = apply(&c, &lookups(&[], &[], &[], &[]), &update(degree(None, None))).await.expect_err("apply");
        assert!(err.to_string().contains("Associate"), "{err}");

        let err = college_body(&update(degree(None, None)), &lookups(&[], &[], &[], &[])).expect_err("body");
        assert!(err.to_string().contains("Associate"), "{err}");

        Mock::given(method("PUT"))
            .and(path("/applicant/api/academicHistory/colleges/4"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
            .mount(&server)
            .await;
        apply(&c, &full(), &update(degree(None, None))).await.expect("apply");
    })
    .await
}
