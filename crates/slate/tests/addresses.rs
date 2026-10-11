//! The address widget against markup captured 2026-10-06 (address
//! replaced): reading the table, planning inserts, and the insert request.

use admit_domain::Finding;
use admit_slate::answer::{self, Address, AnswersFile};
use admit_slate::{Client, Portal, page};
use wiremock::matchers::{header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const PAGE: &str = include_str!("fixtures/address_page.html");
const FORM: &str = include_str!("fixtures/address_form.html");
const HREF: &str = "frm?7946b795-1470-4fc2-bc93-35f853dc8846";
const TYPE: &str = "form_bdf3bde8-fcee-45e6-b215-c9b971d5d506";
const BLOCK: &str = "form_ce335d0f-e189-4977-b906-9b4d4543c276";

fn pages() -> Vec<page::Page> {
    let mut p = page::page(HREF, PAGE);
    let w = p.addresses.as_mut().expect("widget");
    w.form = page::page(&w.add, FORM).questions;
    vec![p]
}

fn addr(kind: &str, street: &str) -> Address {
    Address {
        kind: kind.into(),
        street: street.into(),
        city: "Springfield".into(),
        region: "CA".into(),
        country: "US".into(),
        postal: "00000-0000".into(),
    }
}

fn file(addresses: Vec<Address>) -> AnswersFile {
    AnswersFile { addresses, ..Default::default() }
}

#[test]
fn the_widget_lists_saved_addresses_and_its_insert_form() {
    let w = page::address_widget(PAGE).expect("widget");
    assert_eq!(
        w.add,
        "frm?cmd=widget&form=aa55f0de-ee58-4792-9ede-7a05b4000682&field=ad250bbe-ebc3-4075-8a2f-18d8bfbd0de7&scope=app_hosted_page"
    );
    assert_eq!(w.rows.len(), 1);
    let r = &w.rows[0];
    assert_eq!(
        (r.kind.as_str(), r.street.as_str(), r.region.as_str(), r.country.as_str()),
        ("Permanent Address", "1 Example St Apt 2", "CA", "United States") // oss-publish:allow: fictional
    );
    assert_eq!(page::address_widget("<h1>Citizenship</h1>"), None);
}

#[test]
fn a_missing_address_is_inserted_with_the_captured_fields() {
    let plan =
        answer::plan(&pages(), &file(vec![addr("permanent", "1 Example St Apt 2"), addr("mailing", "PO Box 9")])); // oss-publish:allow: fictional
    assert!(plan.findings.is_empty(), "{:?}", plan.findings);
    assert_eq!(plan.addresses.len(), 1, "the permanent one is already there");
    let a = &plan.addresses[0];
    assert_eq!(a.address.kind, "mailing");
    assert_eq!(a.post_to, pages()[0].addresses.as_ref().expect("widget").add);
    // Field names as the browser posted them when the permanent address was saved.
    let names: Vec<String> = a.fields.iter().map(|(n, _)| n.clone()).collect();
    let want: Vec<String> = [
        TYPE.to_string(),
        format!("{BLOCK}_country"),
        format!("{BLOCK}_street"),
        format!("{BLOCK}_city"),
        format!("{BLOCK}_region"),
        format!("{BLOCK}_postal"),
        format!("{BLOCK}_quality"),
        format!("{BLOCK}_dirty"),
        "cmd".into(),
    ]
    .into();
    assert_eq!(names, want);
    assert_eq!(a.fields[0].1, "mailing");
    assert_eq!(a.fields.last().map(|(_, v)| v.as_str()), Some("save"));
}

#[test]
fn a_differing_or_unknown_address_is_reported_not_written() {
    let plan = answer::plan(&pages(), &file(vec![addr("permanent", "2 Other Rd"), addr("vacation", "x")])); // oss-publish:allow: fictional
    assert!(plan.addresses.is_empty());
    assert!(matches!(&plan.findings[0], Finding::Pending(m) if m.contains("not automated")), "{:?}", plan.findings);
    assert!(
        matches!(&plan.findings[1], Finding::Violation(m) if m.contains("\"vacation\" is not offered")),
        "{:?}",
        plan.findings
    );
}

#[test]
fn pulled_addresses_keep_an_unmatched_kind_and_a_non_us_country() {
    let mut pages = pages();
    let w = pages[0].addresses.as_mut().expect("widget");
    w.rows[0].kind = "Holiday".into();
    w.rows[0].country = "Canada".into();
    let text = answer::to_toml(&pages, "# pulled");
    assert!(text.contains("kind = \"Holiday\""), "{text}");
    assert!(text.contains("country = \"Canada\""), "{text}");
}

#[test]
fn pulled_addresses_plan_to_nothing() {
    let pages = pages();
    let text = answer::to_toml(&pages, "# pulled");
    let f: AnswersFile = toml::from_str(&text).expect("parses");
    assert_eq!(f.addresses, vec![addr("permanent", "1 Example St Apt 2")], "{text}"); // oss-publish:allow: fictional
    assert_eq!(answer::plan(&pages, &f).count(), 0);
}

#[tokio::test]
async fn apply_inserts_then_checks_the_table() {
    tokio::task::unconstrained(async move {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/apply/frm"))
        .and(query_param("cmd", "widget"))
        .and(header("origin", server.uri().as_str()))
        .respond_with(ResponseTemplate::new(200).set_body_string("<script>parent.FW.Dialog.Unload();</script>"))
        .expect(1)
        .mount(&server)
        .await;
    let after = PAGE.replace(
        "</tbody>",
        "<tr class=\"widget_table_row\"><td><a>Mailing Address</a></td><td>PO Box 9</td><td>Springfield</td><td>CA</td><td>United States</td><td>00000-0000</td></tr></tbody>",
    );
    Mock::given(method("GET"))
        .and(path("/apply/frm"))
        .respond_with(ResponseTemplate::new(200).set_body_string(after))
        .mount(&server)
        .await;
    let before = pages();
    let plan = answer::plan(&before, &file(vec![addr("mailing", "PO Box 9")]));
    let portal = Portal { name: "ucla".into(), client: Client::new(&server.uri(), "_uid=x".into()) };
    portal.apply(&plan, &before).await.expect("inserted and listed");

    // The same insert against a page that never lists it fails loudly.
    let server2 = MockServer::start().await;
    Mock::given(method("POST")).respond_with(ResponseTemplate::new(200)).mount(&server2).await;
    Mock::given(method("GET")).respond_with(ResponseTemplate::new(200).set_body_string(PAGE)).mount(&server2).await;
    let portal = Portal { name: "ucla".into(), client: Client::new(&server2.uri(), "_uid=x".into()) };
    let err = portal.apply(&plan, &before).await.expect_err("not listed");
    assert!(err.to_string().contains("doesn't list it"), "{err}");
})
    .await
}

#[test]
fn missing_address_widgets_and_incomplete_forms_are_violations() {
    let f = file(vec![addr("mailing", "PO Box 9")]);
    let plan = answer::plan(&[], &f);
    assert!(plan.findings[0].is_violation());
    let mut bare = pages();
    bare[0].addresses = None;
    let plan = answer::plan(&bare, &f);
    assert!(plan.findings[0].to_string().contains("no page has an address widget"));
    assert!(plan.findings[0].to_string().contains("no page has an address widget"));
    let mut before = pages();
    before[0].addresses.as_mut().expect("widget").form.clear();
    let plan = answer::plan(&before, &f);
    assert!(plan.findings[0].to_string().contains("no type/address questions"));
    let plan = answer::plan(&pages(), &f);
    assert!(plan.render().contains("mailing address: PO Box 9"));
}

#[tokio::test]
async fn inserting_an_address_rejects_collateral_answer_changes() {
    tokio::task::unconstrained(async move {
    let server = MockServer::start().await;
    Mock::given(method("POST")).respond_with(ResponseTemplate::new(200)).mount(&server).await;
    let question = "<div class=\"form_question\" data-id='name' data-type='text' data-export='name'><input name='form_name' value='Original'></div>";
    let mut before = pages();
    before[0].questions.extend(page::page("frm?x", question).questions);
    let plan = answer::plan(&before, &file(vec![addr("mailing", "PO Box 9")]));
    let after=PAGE.replace("</tbody>","<tr class=\"widget_table_row\"><td>Mailing Address</td><td>PO Box 9</td><td>Springfield</td><td>CA</td><td>United States</td><td>00000-0000</td></tr></tbody>");
    Mock::given(method("GET")).respond_with(ResponseTemplate::new(200).set_body_string(after)).mount(&server).await;
    let portal = Portal { name: "test".into(), client: Client::new(&server.uri(), "_uid=x".into()) };
    let err = portal.apply(&plan, &before).await.expect_err("collateral loss");
    assert!(err.to_string().contains("inserting an address changed"));
})
    .await
}
