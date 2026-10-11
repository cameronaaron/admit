//! The completeness audit. Regression for 2026-10-06: the application was
//! called complete from the Review page and blank questions alone while
//! Professional Experience was empty and the profile held 18 paid roles.

use std::collections::BTreeMap;

use admit_domain::{Experience, ExperienceKind, Finding, Profile};
use admit_slate::answer::AnswersFile;
use admit_slate::coverage::{self, Entry, Section};
use admit_slate::page::{self, Issue};

const JOB_EMPTY: &str = include_str!("fixtures/job.html");

fn role(org: &str, title: &str, paid: bool) -> Experience {
    Experience {
        kind: ExperienceKind::Employment,
        organization: org.into(),
        title: title.into(),
        state: "California".into(),
        country: "United States".into(),
        start: "2022-11-01".into(),
        end: Some("2024-07-01".into()),
        current: false,
        status: "Full time".into(),
        hours_per_week: 40,
        weeks: 1,
        total_hours: 40,
        compensated: paid,
        volunteer: false,
        description: String::new(),
        supervisor: Default::default(),
        ids: Default::default(),
    }
}

fn entries() -> Vec<Entry> {
    vec![
        Entry {
            title: "Professional Experience".into(),
            href: "job".into(),
            section: Section::List { rows: 0, declared: Some(0) },
        },
        Entry { title: "Test Scores".into(), href: "tes".into(), section: Section::List { rows: 0, declared: None } },
        Entry { title: "Mystery".into(), href: "xyz".into(), section: Section::Unknown },
        Entry { title: "Applicant Agreements".into(), href: "certify".into(), section: Section::Signature },
    ]
}

#[test]
fn every_section_gets_one_line_and_empty_or_unknown_ones_are_not_ok() {
    let issues = vec![Issue {
        section: "Applicant Agreements".into(),
        href: "certify".into(),
        message: "Please complete prior to submission.".into(),
        error: true,
    }];
    let f = coverage::report(&entries(), &issues, &BTreeMap::new());
    assert_eq!(f.len(), 4, "one line per section: {f:#?}");
    assert!(matches!(&f[0], Finding::Pending(m) if m.contains("Professional Experience: TODO (empty")), "{f:#?}");
    assert!(matches!(&f[1], Finding::Pending(m) if m.contains("Test Scores: TODO")));
    assert!(matches!(&f[2], Finding::Violation(m) if m.contains("UNCHECKED")));
    assert!(matches!(&f[3], Finding::Pending(m) if m.contains("Please complete")));
    // An empty section excluded with a reason is fine.
    let excluded = BTreeMap::from([("Test Scores".to_string(), "MECN requires no tests".to_string())]);
    let f = coverage::report(&entries(), &issues, &excluded);
    assert!(matches!(&f[1], Finding::Note(m) if m.contains("empty: MECN requires no tests")), "{f:#?}");
}

#[test]
fn review_counts_warnings_that_are_not_its_own_errors() {
    let e = [Entry { title: "Review".into(), href: "review".into(), section: Section::Review }];
    let issues = vec![
        Issue { section: "Other".into(), href: "frm?x".into(), message: "blocks".into(), error: true },
        Issue { section: "Review".into(), href: "review".into(), message: "check this".into(), error: false },
    ];
    let f = coverage::report(&e, &issues, &BTreeMap::new());
    assert!(matches!(&f[0], Finding::Note(m) if m.contains("1 warning")), "{f:#?}");
}

#[test]
fn fewer_rows_than_declared_is_a_gap() {
    let e = [Entry {
        title: "Professional Experience".into(),
        href: "job".into(),
        section: Section::List { rows: 1, declared: Some(18) },
    }];
    let f = coverage::report(&e, &[], &BTreeMap::new());
    assert!(matches!(&f[0], Finding::Pending(m) if m.contains("1 of 18")), "{f:#?}");
}

#[test]
fn list_rows_counts_the_captured_tables() {
    assert_eq!(page::list_rows(JOB_EMPTY), Some(1), "the job list as captured has one row");
    assert_eq!(page::list_rows("<h1>Welcome</h1>"), None);
}

#[test]
fn profile_roles_missing_from_the_answers_file_are_reported() {
    let mut p = Profile::default();
    p.experiences.experiences =
        vec![role("Bridges Academy", "Engineering Instructor", true), role("Upkey", "Intern", false)];
    let f = coverage::profile_gaps(&p, &AnswersFile::default());
    assert_eq!(f.len(), 1, "unpaid roles aren't employment: {f:#?}");
    assert!(matches!(&f[0], Finding::Pending(m) if m.contains("Bridges Academy - Engineering Instructor")));
    let file = AnswersFile {
        excluded: BTreeMap::from([("Bridges Academy - Engineering Instructor".to_string(), "why".to_string())]),
        ..Default::default()
    };
    assert!(coverage::profile_gaps(&p, &file).is_empty(), "an excluded role is accounted for");
}
