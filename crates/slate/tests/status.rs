//! The status portal after submission (Azusa Pacific, captured 2026-10-06;
//! names replaced).

use admit_domain::Finding;
use admit_slate::page;
use admit_slate::{Status, Update};

const STATUS: &str = include_str!("fixtures/apu_status.html");
const UPDATE: &str = include_str!("fixtures/apu_update.html");

#[test]
fn checklists_read_every_row_with_its_list() {
    let items = page::checklists(STATUS);
    assert_eq!(items.len(), 11);
    assert_eq!(items[0].list, "Financial Aid Checklist");
    assert_eq!(items[0].detail, "Free Application for Federal Student Aid (FAFSA)");
    assert_eq!(items[0].date, "12/07/2025");
    let awaiting: Vec<&str> = items.iter().filter(|i| i.status == "Awaiting").map(|i| i.detail.as_str()).collect();
    assert_eq!(awaiting, ["Essay", "Prerequisites"]);
    assert!(items.iter().any(|i| i.detail == "Résumé"), "entities decoded");
    assert!(items.iter().all(|i| i.list != "Financial Aid Checklist" || i.status == "Received"));
}

#[test]
fn the_latest_update_and_its_letter() {
    assert_eq!(page::update_posted(STATUS).as_deref(), Some("June 2, 2026"));
    assert_eq!(page::update_posted("<h3>Application Checklist</h3>"), None);
    let (href, released) = page::update_pdf(UPDATE).expect("pdf link");
    assert_eq!(href, "update?cmd=stream&id=cb6a5baa-88db-4fb2-9e5b-483ac66d565f");
    assert_eq!(released, "June 2, 2026");
}

#[test]
fn awaiting_items_are_pending_and_the_update_a_note() {
    let s =
        Status { items: page::checklists(STATUS), update: Some(Update { posted: "June 2, 2026".into(), pdf: None }) };
    let f = s.findings("apu");
    assert_eq!(f.iter().filter(|x| matches!(x, Finding::Pending(_))).count(), 2);
    assert!(!f.iter().any(Finding::is_violation));
    assert!(matches!(f.last(), Some(Finding::Note(m)) if m.contains("June 2, 2026")));
}
