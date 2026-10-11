//! An LACCD transcript layout with synthetic grades, as a fixture.

use admit_sources::laccd::{COLLEGES, parse};

const RAW: &str = include_str!("fixtures/laccd_raw.txt");

/// Independent count: lines whose 2nd..nth token is a college abbreviation
/// followed later by two unit columns.
fn course_like_lines() -> usize {
    RAW.replace("U N O F F I C I A L", "\n")
        .lines()
        .filter(|l| {
            let t: Vec<&str> = l.split_whitespace().collect();
            t.iter().skip(1).any(|w| COLLEGES.contains(w))
                && t.windows(2).any(|w| {
                    w.iter().all(|x| x.len() >= 4 && x.contains('.') && x.replace('.', "").parse::<u32>().is_ok())
                })
        })
        .count()
}

#[test]
fn every_course_line_is_parsed_and_reconciles() {
    let courses = parse(RAW, &COLLEGES).expect("parses and reconciles with term totals");
    assert_eq!(courses.len(), course_like_lines());
    assert_eq!(courses.len(), 33);
}

#[test]
fn multi_word_departments_keep_their_college() {
    let courses = parse(RAW, &COLLEGES).unwrap_or_default();
    let get = |code: &str| courses.iter().find(|c| c.code == code).cloned();
    assert_eq!(get("CH DEV 001").map(|c| c.college), Some("LAMC".into()));
    assert_eq!(get("POL SCI 019").map(|c| c.college), Some("ELAC".into()));
    assert_eq!(get("SOC 004").map(|c| (c.college, c.grade)), Some(("LAPC".into(), String::new())));
    assert!(get("VOC ED 393CE").is_some_and(|c| c.noncredit));
    assert!(get("STAT 101").is_some_and(|c| !c.noncredit && c.season == "Summer"));
}
