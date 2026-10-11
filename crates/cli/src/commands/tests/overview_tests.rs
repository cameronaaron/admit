use time::macros::date;

use super::*;

fn due(portal: &str, label: &str, date: Date) -> Due {
    Due { portal: portal.into(), deadline: Deadline { label: label.into(), date, shown: date.to_string() } }
}

#[test]
fn deadlines_from_every_portal_interleave_soonest_first() {
    let today = date!(2026 - 10 - 06);
    let out = render(
        today,
        vec![
            due("nursingcas", "Johns Hopkins University - MSN", date!(2027 - 01 - 15)),
            due("ucla", "ucla application", date!(2026 - 12 - 01)),
            due("nursingcas", "UCSB - ABSN", date!(2026 - 11 - 15)),
            due("nursingcas", "Old - Gone", date!(2026 - 09 - 01)),
        ],
        &[],
    );
    let order: Vec<usize> = ["Old - Gone", "UCSB - ABSN", "ucla application", "Johns Hopkins"]
        .iter()
        .map(|l| out.find(l).unwrap_or_else(|| panic!("{l} missing:\n{out}")))
        .collect();
    assert!(order.windows(2).all(|w| w[0] < w[1]), "not soonest first:\n{out}");
    assert!(out.contains("PASSED 35 ago"), "{out}");
    assert!(out.contains("in 40 day(s)"), "{out}");
}

#[test]
fn groups_list_violations_before_pending_and_only_count_notes() {
    let groups = [Group {
        name: "ucla".into(),
        findings: vec![
            Finding::Note("deep in the weeds".into()),
            Finding::Pending("2 answer(s) not yet applied".into()),
            Finding::Violation("UNCHECKED: session expired".into()),
        ],
    }];
    let out = render(date!(2026 - 10 - 06), vec![], &groups);
    assert!(out.contains("ucla: 1 violation(s), 1 pending, 1 note(s)"), "{out}");
    assert!(!out.contains("deep in the weeds"), "notes are counted, not listed:\n{out}");
    let (v, p) = (out.find("UNCHECKED"), out.find("not yet applied"));
    assert!(v.is_some() && v < p, "violations come first:\n{out}");
    assert!(out.contains("1 violation(s), 1 pending across 1 group(s)"), "{out}");
    let only = [Group { name: "profile".into(), findings: vec![Finding::Violation("bad".into())] }];
    let out = render(
        date!(2026 - 10 - 06),
        vec![due("a", "same", date!(2026 - 12 - 01)), due("b", "same", date!(2026 - 12 - 01))],
        &only,
    );
    assert!(out.contains("profile: 1 violation(s), 0 pending\n"), "{out}");
    assert!(out.find("  a  ").is_some_and(|i| out.find("  b  ").is_some_and(|j| i < j)), "{out}");
}

fn slate(deadline: Option<&str>, submitted: bool) -> PortalConfig {
    PortalConfig {
        name: "ucla".into(),
        kind: "slate".into(),
        base_url: String::new(),
        har_dir: None,
        cookie_env: None,
        application: None,
        submitted,
        deadline: deadline.map(str::to_string),
        same_as: None,
    }
}

#[test]
fn a_slate_deadline_is_declared_and_dropped_once_submitted() {
    let date = |pc| slate_deadline(&pc).ok().flatten().map(|d| d.date);
    assert_eq!(date(slate(Some("2026-12-01"), false)), Some(date!(2026 - 12 - 01)));
    assert!(matches!(slate_deadline(&slate(Some("2026-12-01"), true)), Ok(None)));
    assert!(matches!(slate_deadline(&slate(None, false)), Ok(None)));
    assert!(slate_deadline(&slate(Some("Dec 1"), false)).is_err(), "a malformed date is an error, not ignored");
}
