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
        months: table("month", &[(8, "May"), (9, "June")]),
        years: table("year", &[(19, "2017")]),
        terms: table("term", &[]),
        term_types: table("term type", &[(466, "Semester")]),
        academic_status: table("academic status", &[]),
        subjects: table("subject", &[]),
    }
}

fn school(from: &str, to: &str, diploma_date: &str) -> HighSchool {
    HighSchool {
        name: "Bridges Academy".into(),
        from: from.into(),
        to: to.into(),
        diploma: "High School Diploma".into(),
        diploma_date: diploma_date.into(),
        term_type: Some("Semester".into()),
    }
}

#[test]
fn month_ids_follow_the_liaison_table() {
    assert_eq!(month_id("2017-05"), Some(8));
    assert_eq!(month_id("2013-09"), Some(12));
    assert_eq!(month_id("2017-13"), None);
    assert_eq!(month_id("x"), None);
    assert_eq!(month_id("2017-00"), None);
}

#[test]
fn scalar_navigation_is_not_a_subsection() {
    assert_eq!(find_section(&json!([null, 1, "highSchoolAttended"]), SECTION), None);
    assert_eq!(find_section(&json!({"name":SECTION,"id":"42"}), SECTION), Some(42));
    assert_eq!(month_id("invalid"), None);
    let nested = json!({"name": "root", "kids": [null, {"name": SECTION, "id": 9}]});
    assert_eq!(find_section(&nested, SECTION), Some(9));
    assert_eq!(find_section(&json!(1), SECTION), None);
}

#[test]
fn attendance_body_covers_missing_dates_and_lookup_failures() {
    let lk = lookups();
    let row = json!({"id": 3, "termTypeId": 466});
    let mut untyped = school("2017-05", "2017-06", "2017-06");
    untyped.term_type = None;
    let err = attendance_body(&json!({"id": 3}), &untyped, &lk).expect_err("no term type");
    assert!(err.to_string().contains("term_type"), "{err}");

    let typed = attendance_body(&json!({"id": 3}), &school("2017-05", "2017-06", "2017-06"), &lk).expect("term type");
    assert_eq!(typed["termTypeId"], 466);

    let body = attendance_body(&row, &school("2017-05", "2017-06", ""), &lk).expect("no diploma");
    assert_eq!(body["graduated"], false);
    assert!(body.get("yearGraduatedId").is_none(), "{body}");

    let body = attendance_body(&row, &school("2017-00", "2017-00", "2017-06"), &lk).expect("month 0");
    assert_eq!(body["fromMonthId"], 0);

    let err = attendance_body(&row, &school("1999-05", "2017-06", "2017-06"), &lk).expect_err("from year");
    assert!(err.to_string().contains("1999"), "{err}");

    let err = attendance_body(
        &json!({"id": 3}),
        &{
            let mut want = school("2017-05", "2017-06", "2017-06");
            want.term_type = Some("Trimester".into());
            want
        },
        &lk,
    )
    .expect_err("term type");
    assert!(err.to_string().contains("Trimester"), "{err}");

    let err = attendance_body(&row, &school("2017-05", "2017-06", "1999-06"), &lk).expect_err("diploma year");
    assert!(err.to_string().contains("1999"), "{err}");

    let err = attendance_body(&row, &school("x", "2017-06", "2017-06"), &lk).expect_err("short date");
    assert!(err.to_string().contains("x") || err.to_string().contains("year"), "{err}");
}

#[tokio::test]
async fn live_and_apply_cover_missing_sections_and_body_errors() {
    tokio::task::unconstrained(async move {
        let server = MockServer::start().await;
        let c = Client::new(&server.uri(), "SESSION=x".into());
        assert!(live(&c).await.is_err(), "rows");

        Mock::given(method("GET"))
            .and(path("/applicant/api/academicHistory/highSchools"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": []})))
            .mount(&server)
            .await;
        assert!(live(&c).await.is_err(), "navigation");

        Mock::given(method("GET"))
            .and(path("/applicant/api/navigation"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"name": "other"})))
            .with_priority(10)
            .mount(&server)
            .await;
        let l = live(&c).await.expect("no section");
        assert_eq!(l.info_section, 0);
        assert!(l.info.is_null());

        Mock::given(method("GET"))
            .and(path("/applicant/api/navigation"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "kids": [null, {"name": SECTION, "id": 9}]
            })))
            .with_priority(1)
            .mount(&server)
            .await;
        assert!(live(&c).await.is_err(), "info");

        let ch = Change::Attendance {
            row: json!({"id": 3, "termTypeId": 466}),
            want: school("1999-05", "2017-06", "2017-06"),
            was: String::new(),
        };
        let err = apply(&c, &lookups(), &ch).await.expect_err("body");
        assert!(err.to_string().contains("1999"), "{err}");

        Mock::given(method("POST"))
            .and(path("/applicant/api/academicHistory/highSchoolInfo"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
            .mount(&server)
            .await;
        let ch = Change::Diploma {
            info_section: 9,
            info: json!({"questionBlocks": "not-an-array"}),
            want: school("2017-05", "2017-06", "2017-06"),
            was: String::new(),
        };
        apply(&c, &lookups(), &ch).await.expect("diploma post");
        // A document that is not an object is posted as it came, not a panic.
        let ch = Change::Diploma {
            info_section: 9,
            info: json!(["not", "an", "object"]),
            want: school("2017-05", "2017-06", "2017-06"),
            was: String::new(),
        };
        apply(&c, &lookups(), &ch).await.expect("non-object diploma document");

        Mock::given(method("PUT"))
            .and(path("/applicant/api/academicHistory/highSchools/3"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
            .mount(&server)
            .await;
        let ch = Change::Attendance {
            row: json!({"id": 3, "termTypeId": 466}),
            want: school("2017-05", "2017-06", "2017-06"),
            was: String::new(),
        };
        apply(&c, &lookups(), &ch).await.expect("attendance");

        let ch = Change::Diploma {
            info_section: 9,
            info: json!({"questionBlocks": [{"id": "graduationStatus", "data": {}}]}),
            want: school("2017-05", "2017-06", "2017-06"),
            was: String::new(),
        };
        apply(&c, &lookups(), &ch).await.expect("diploma block");
    })
    .await
}
