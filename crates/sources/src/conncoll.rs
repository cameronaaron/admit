//! Connecticut College transcript (Banner), read through `pdftotext -raw`.
//! Rows lead with credits; the semester appears on a term's first row only:
//!
//! ```text
//! 4.00 COM 110 INTRO COMPUTER SCI/PROB SOLV Fall 2017 12.00 B
//! 0.00 COM 110L INTRO COMP SCI/PROB SOLV LAB .00 P
//! 3.00 BUS 201 BUSINESS LEADERSHIP & ENTREPRE Summer 2020 A      (transfer: no points)
//! ```
//!
//! Transfer coursework is listed inline under the sending institution's name
//! for one term ("Trinity Christian College", "UMASS Amherst", …); the course's
//! `college` is that name, and it reverts to the home institution after the
//! term's "Term Summary". Reconciled against "OVERALL" credits attempted, which
//! excludes the Spring 2020 emergency grade `S*` (that term's own summary
//! shows 6.00 attempted of 18.00 on the rows).

use std::path::Path;

use crate::{ParseError, TranscriptCourse, hundredths, is_units, run_pdftotext};

pub const INSTITUTION: &str = "Connecticut College";
const SEASONS: [&str; 4] = ["Fall", "Winter", "Spring", "Summer"];

pub fn read_pdf(path: &Path) -> Result<String, ParseError> {
    run_pdftotext(path, "-raw")
}

fn is_credits(t: &str) -> bool {
    is_units(t) || (t.starts_with('.') && t.len() == 3 && t[1..].bytes().all(|b| b.is_ascii_digit()))
}

fn is_grade(t: &str) -> bool {
    let b = t.as_bytes();
    match b {
        [g] => b"ABCDFPWS".contains(g),
        [g, m] => b"ABCDF".contains(g) && (*m == b'+' || *m == b'-') || t == "NP" || t == "S*" || t == "IP",
        _ => false,
    }
}

/// `transfer_labels`: institution names that head transfer blocks.
pub fn parse(text: &str, transfer_labels: &[&str]) -> Result<Vec<TranscriptCourse>, ParseError> {
    let mut courses = Vec::new();
    let (mut season, mut year) = (String::new(), String::new());
    let mut college = INSTITUTION.to_string();
    let mut stated: Option<String> = None;
    for line in text.lines().map(str::trim) {
        if transfer_labels.contains(&line) {
            college = line.to_string();
            continue;
        }
        if line == "Term Summary:" {
            college = INSTITUTION.to_string();
            continue;
        }
        let t: Vec<&str> = line.split_whitespace().collect();
        if t.first() == Some(&"OVERALL") && t.len() >= 2 {
            stated = Some(t[1].to_string());
            continue;
        }
        let n = t.len();
        if n < 5 || !is_credits(t[0]) || !t[1].bytes().all(|b| b.is_ascii_uppercase()) || !is_grade(t[n - 1]) {
            continue;
        }
        let mut end = n - 1;
        if is_credits(t[end - 1]) {
            end -= 1; // quality points
        }
        if end >= 2 && SEASONS.contains(&t[end - 2]) && t[end - 1].len() == 4 {
            season = t[end - 2].into();
            year = t[end - 1].into();
            end -= 2;
        }
        if season.is_empty() || end <= 3 {
            continue;
        }
        courses.push(TranscriptCourse {
            year: year.clone(),
            season: season.clone(),
            code: format!("{} {}", t[1], t[2]),
            college: college.clone(),
            title: t[3..end].join(" "),
            credits: if t[0].starts_with('.') { format!("0{}", t[0]) } else { t[0].into() },
            grade: t[n - 1].into(),
            noncredit: false,
            term_months: None,
        });
    }
    if courses.is_empty() {
        return Err(ParseError::Empty);
    }
    let attempted: u64 = courses.iter().filter(|c| c.grade != "S*").map(|c| hundredths(&c.credits)).sum();
    match stated {
        Some(s) if hundredths(&s) == attempted => Ok(courses),
        Some(s) => Err(ParseError::Unreconciled(vec![format!(
            "transcript OVERALL says {s} credits attempted, parsed {:.2} (excluding S*)",
            attempted as f64 / 100.0
        )])),
        None => Err(ParseError::Unreconciled(vec!["no OVERALL totals line found".into()])),
    }
}
