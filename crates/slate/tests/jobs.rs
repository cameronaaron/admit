//! Professional Experience against the list as it read live on 2026-10-06
//! after the first role was added.

use admit_domain::Finding;
use admit_slate::jobs::{self, Job};
use admit_slate::page;

const JOB: &str = include_str!("fixtures/job.html");

fn role(employer: &str, title: &str, from: &str, to: Option<&str>) -> Job {
    Job {
        employer: employer.into(),
        title: title.into(),
        from: from.into(),
        to: to.map(Into::into),
        hours: 40,
        description: "•Did things".into(),
        profile_title: None,
    }
}

#[test]
fn the_list_reads_employer_and_dates() {
    assert_eq!(page::jobs_listed(JOB), Some(vec![("4me (Xurrent)".to_string(), "02/2023\u{2013}04/2023".to_string())]));
    assert_eq!(page::jobs_listed("<h1>Test Scores</h1>"), None);
    assert_eq!(
        jobs::dates_label(&role("x", "y", "2023-02", Some("2023-04"))).as_deref(),
        Some("02/2023\u{2013}04/2023")
    );
}

#[test]
fn listed_roles_are_skipped_and_same_dated_roles_counted() {
    let listed = page::jobs_listed(JOB).expect("job page");
    let want = [
        role("4me (Xurrent)", "DevOps Engineer", "2023-02", Some("2023-04")),
        role("Bridges Academy", "Engineering Instructor", "2022-11", Some("2024-07")),
        role("Bridges Academy", "Biopsychology Instructor", "2022-11", Some("2024-07")),
    ];
    let (adds, f) = jobs::plan(&listed, &want);
    assert!(f.is_empty(), "{f:?}");
    let titles: Vec<&str> = adds.iter().map(|a| a.job.title.as_str()).collect();
    assert_eq!(titles, ["Engineering Instructor", "Biopsychology Instructor"], "two roles, same employer and dates");
    // Once one Bridges row exists, only the other is still to add.
    let mut more = listed.clone();
    more.push(("Bridges Academy".into(), "11/2022\u{2013}07/2024".into()));
    let (adds, _) = jobs::plan(&more, &want);
    assert_eq!(adds.len(), 1);
    let f = &adds[0].fields;
    assert!(f.contains(&("from_m".into(), "11".into())) && f.contains(&("to_y".into(), "2024".into())), "{f:?}");
}

#[test]
fn a_current_role_posts_an_empty_end_and_limits_are_checked() {
    let (adds, _) = jobs::plan(&[], &[role("Clinic", "EMT", "2025-01", None)]);
    let f = &adds[0].fields;
    assert!(f.contains(&("to_m".into(), String::new())), "empty to_m is Present: {f:?}");
    let long = "x".repeat(65);
    let bad = [
        role("A", &long, "2020-01", None),
        role("A", "T", "2021-01", Some("2020-01")),
        role("A", "T", "January 2020", None),
        Job { hours: 100, ..role("A", "T", "2020-01", None) },
    ];
    let (adds, f) = jobs::plan(&[], &bad);
    assert!(adds.is_empty());
    assert_eq!(f.len(), 4, "{f:#?}");
    assert!(f.iter().all(Finding::is_violation));
    let month = [
        role("A", "T", "2020-13", None),
        role("A", "T", "2020-01", Some("nope")),
        role("A", "T", "2020-00", Some("2020-01")),
    ];
    let (adds, f) = jobs::plan(&[], &month);
    assert!(adds.is_empty());
    assert_eq!(f.len(), 3, "{f:#?}");
    assert!(jobs::dates_label(&month[1]).is_none());
}
