//! Elmbridge University (formerly Bridges Graduate School) official transcript,
//! read through `pdftotext -layout`. Rows look like
//!
//! ```text
//! 8   RES 621A   Methods and Techniques of Educational Research   2025 SPRING   1.00   A-   3.67
//!                Part 1                                            (title continued on the next line)
//! ```
//!
//! Reconciled against the transcript's "Credits Earned".

use std::path::Path;

use crate::{ParseError, TranscriptCourse, is_units, run_pdftotext};

pub const INSTITUTION: &str = "Elmbridge University";
const SEASONS: [&str; 4] = ["FALL", "WINTER", "SPRING", "SUMMER"];
/// Grades that earn no credit (from the transcript key).
const NO_CREDIT: [&str; 8] = ["W", "NP", "F", "I", "IP", "NR", "AU", "N"];

pub fn read_pdf(path: &Path) -> Result<String, ParseError> {
    run_pdftotext(path, "-layout")
}

fn title_case(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_ascii_uppercase().to_string() + &c.as_str().to_ascii_lowercase()).unwrap_or_default()
}

pub fn parse(text: &str) -> Result<Vec<TranscriptCourse>, ParseError> {
    let mut courses: Vec<TranscriptCourse> = Vec::new();
    let mut stated_earned: Option<String> = None;
    let mut last_was_course = false;
    for line in text.lines() {
        let t: Vec<&str> = line.split_whitespace().collect();
        if let Some(i) = t.iter().position(|w| *w == "Earned:") {
            stated_earned = t.get(i + 1).map(|s| s.to_string());
        }
        let n = t.len();
        let is_course = n >= 8
            && t[0].bytes().all(|b| b.is_ascii_digit())
            && t[1].bytes().all(|b| b.is_ascii_uppercase())
            && is_units(t[n - 3])
            && SEASONS.contains(&t[n - 4])
            && t[n - 5].len() == 4
            && t[n - 5].bytes().all(|b| b.is_ascii_digit());
        if is_course {
            courses.push(TranscriptCourse {
                year: t[n - 5].into(),
                season: title_case(t[n - 4]),
                code: format!("{} {}", t[1], t[2]),
                college: INSTITUTION.into(),
                title: t[3..n - 5].join(" "),
                credits: t[n - 3].into(),
                grade: t[n - 2].into(),
                noncredit: false,
                term_months: None,
            });
            last_was_course = true;
            continue;
        }
        let trimmed = line.trim();
        if last_was_course
            && !trimmed.is_empty()
            && !trimmed.starts_with("Term")
            && trimmed.len() < 40
            && let Some(c) = courses.last_mut()
        {
            c.title = format!("{} {trimmed}", c.title);
        }
        last_was_course = false;
    }
    if courses.is_empty() {
        return Err(ParseError::Empty);
    }
    let earned: u64 =
        courses.iter().filter(|c| !NO_CREDIT.contains(&c.grade.as_str())).map(|c| crate::hundredths(&c.credits)).sum();
    match stated_earned {
        Some(s) if crate::hundredths(&s) == earned => Ok(courses),
        Some(s) => Err(ParseError::Unreconciled(vec![format!(
            "transcript says {s} credits earned, parsed {:.2}",
            earned as f64 / 100.0
        )])),
        None => Err(ParseError::Unreconciled(vec!["no \"Credits Earned\" total found".into()])),
    }
}
