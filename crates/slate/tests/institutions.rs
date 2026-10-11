//! Academic History against pages captured 2026-10-06: the (empty)
//! institution list, the Add Institution form's degrees, and the post.

use admit_domain::Finding;
use admit_slate::institutions::{self, Institution};
use admit_slate::{Client, page};
use wiremock::matchers::{header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const ACA: &str = include_str!("fixtures/aca.html");
const ADD: &str = include_str!("fixtures/aca_add.html");

fn conn() -> Institution {
    Institution {
        name: "Connecticut College".into(),
        profile: None,
        key: Some("003284".into()),
        city: "New London".into(),
        region: "CT".into(),
        country: "US".into(),
        from: "2017-08".into(),
        to: "2021-05".into(),
        level: "U".into(),
        degree: Some("Bachelor of Arts".into()),
        conferred: Some("2021-05".into()),
        transcript: "transcripts/conncoll.pdf".into(),
        major: Some("Computer Science and Psychology".into()),
        gpa: Some("3.28".into()),
        gpa_scale: Some("4.0".into()),
    }
}

fn field<'a>(f: &'a [(String, String)], k: &str) -> Option<&'a str> {
    f.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str())
}

#[test]
fn the_list_and_the_degree_choices_read() {
    assert_eq!(page::institutions_listed(ACA), Some(vec![]));
    assert_eq!(page::institutions_listed("<h1>Review</h1>"), None);
    let d = page::degree_choices(ADD);
    assert_eq!(d.keys().map(String::as_str).collect::<Vec<_>>(), ["G", "U"]);
    let ba = d["U"].iter().find(|c| c.text == "Bachelor of Arts").expect("BA");
    assert_eq!(ba.value, "6ef2a40b-e2c1-4a61-8bec-aa9681dac333");
    assert!(d["G"].iter().any(|c| c.text == "Master of Education"));
}

#[test]
fn a_listed_school_posts_its_key_and_a_degree_its_value() {
    let d = page::degree_choices(ADD);
    let (adds, f) = institutions::plan(&[], &d, &[conn()]);
    assert!(f.is_empty(), "{f:?}");
    let x = &adds[0].fields;
    assert_eq!(field(x, "key"), Some("003284"));
    assert_eq!(field(x, "city_keyed"), Some("New London"));
    assert_eq!(field(x, "country_keyed"), Some("US"));
    assert_eq!((field(x, "from_m"), field(x, "from_y"), field(x, "to_m")), (Some("08"), Some("2017"), Some("05")));
    assert_eq!(field(x, "type"), Some("U"));
    assert_eq!(field(x, "degree"), Some("6ef2a40b-e2c1-4a61-8bec-aa9681dac333"));
    assert_eq!((field(x, "conferred_m"), field(x, "conferred_y")), (Some("05"), Some("2021")));
    assert_eq!(field(x, "city"), None, "the Other address block is disabled for a listed school");
    // Already listed: nothing to add.
    let (adds, f) = institutions::plan(&["Connecticut College".into()], &d, &[conn()]);
    assert!(adds.is_empty() && f.is_empty(), "{f:?}");
}

#[test]
fn a_listed_school_with_a_bad_date_is_a_violation() {
    // Its dates feed the Additional Academic Info form: a blank one used to be
    // skipped there and a malformed one written as it stood.
    let d = page::degree_choices(ADD);
    for (from, to) in [("", "2021-05"), ("2017-08", "2021/05"), ("2021-05", "2017-08")] {
        let i = Institution { from: from.into(), to: to.into(), ..conn() };
        let (adds, f) = institutions::plan(&["Connecticut College".into()], &d, &[i]);
        assert!(adds.is_empty());
        assert!(
            matches!(f.as_slice(), [Finding::Violation(m)] if m.contains("Connecticut College")),
            "{from} {to}: {f:?}"
        );
    }
}

#[test]
fn an_unlisted_school_goes_in_as_other_without_a_degree() {
    let d = page::degree_choices(ADD);
    let other = Institution { key: None, degree: None, conferred: None, name: "Elmbridge University".into(), ..conn() };
    let (adds, f) = institutions::plan(&[], &d, &[other]);
    assert!(f.is_empty(), "{f:?}");
    let x = &adds[0].fields;
    assert_eq!(field(x, "key"), Some(""));
    assert_eq!(field(x, "city_keyed"), Some(""));
    assert_eq!(
        (field(x, "country"), field(x, "city"), field(x, "region")),
        (Some("US"), Some("New London"), Some("CT"))
    );
    assert_eq!(field(x, "degree"), Some(""));
    assert_eq!(field(x, "conferred_m"), None);
}

#[test]
fn bad_institutions_are_violations() {
    let d = page::degree_choices(ADD);
    let cases = [
        Institution { degree: Some("Doctor of Wizardry".into()), ..conn() },
        Institution { conferred: None, ..conn() },
        Institution { from: "2022-01".into(), ..conn() },
        Institution { level: "X".into(), ..conn() },
        Institution { to: "May 2021".into(), ..conn() },
    ];
    let (adds, f) = institutions::plan(&[], &d, &cases);
    assert!(adds.is_empty());
    assert_eq!(f.len(), 5, "{f:#?}");
    assert!(f.iter().all(Finding::is_violation));
}

#[tokio::test]
async fn the_transcript_is_posted_as_file1() {
    tokio::task::unconstrained(async move {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/apply/aca"))
            .and(query_param("id", ""))
            .and(header("origin", server.uri().as_str()))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&server)
            .await;
        let c = Client::new(&server.uri(), "_uid=x".into());
        let fields = vec![("key".to_string(), "003284".to_string()), ("cmd".into(), "save".into())];
        c.save_file("aca?id=", &fields, "file1", "conncoll.pdf", b"%PDF-1.4 x".to_vec()).await.expect("posted");
        let body = String::from_utf8_lossy(&server.received_requests().await.expect("recorded")[0].body).to_string();
        assert!(
            body.contains(
                "name=\"file1\"; filename=\"conncoll.pdf\"\r\nContent-Type: application/pdf\r\n\r\n%PDF-1.4 x"
            ),
            "{body}"
        );
        assert!(body.contains("name=\"key\"\r\n\r\n003284"), "{body}");
    })
    .await
}

const ADDL: &str = include_str!("fixtures/addl_info.html");

#[test]
fn blank_dates_are_not_school_details() {
    let mut school = conn();
    school.from.clear();
    school.to.clear();
    school.major = None;
    school.gpa = None;
    school.gpa_scale = None;
    assert!(school.details().is_empty());
    school.from = "2017-08".into();
    assert!(school.details().contains_key("sys:school:from"));
    assert!(!school.details().contains_key("sys:school:to"));
}

#[test]
fn each_school_has_its_own_detail_form() {
    // Additional Academic Info, live 2026-10-06 after the institutions were added.
    let rows = page::widget_rows(ADDL);
    assert_eq!(rows.len(), 12, "{rows:?}");
    assert_eq!(rows[0].0, "Connecticut College");
    assert!(
        rows[0].1.starts_with("frm?cmd=widget&form=9001caf3-06cc-408b-a5e7-63c0ec7e87bb&id=777f4674"),
        "{}",
        rows[0].1
    );
    assert!(rows.iter().all(|(_, h)| h.contains("&id=")), "the Add New link is not a row");
    let d = conn().details();
    assert_eq!(d.len(), 5, "major, GPA, scale, and the dates kept in step");
    assert!(d.contains_key("sys:school:field:gd_school_gpa"));
    assert_eq!(d.get("sys:school:from"), Some(&admit_slate::Answer::One("2017-08".into())));
}
