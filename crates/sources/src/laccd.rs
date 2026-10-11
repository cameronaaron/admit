//! Los Angeles Community College District unofficial transcript
//! (Oracle Analytics Publisher PDF), read through `pdftotext -raw`.
//!
//! One district transcript lists courses from every LACCD college; the college
//! column (LAVC, LACC, …) says where each was taken. Course lines look like
//!
//! ```text
//! CH DEV 001 LAMC Child Growth&Develop 3.00 3.00 A 12 CSU/UC
//! ARTH C1200 LAVC Surv of Art: Renais to Contemp 3.00 0.00 0 CSU/UC   (in progress: no grade)
//! ```
//!
//! The college column is found by matching a *known* abbreviation, never a
//! generic "capital letters" token: "CH DEV 001" and "POL SCI 019" contain
//! words that look like colleges, and a generic match once dropped both.
//! As a second line of defence every parse is reconciled against the
//! transcript's own per-term unit totals.

use crate::{ParseError, TranscriptCourse, hundredths, is_units, run_pdftotext};
use std::collections::BTreeMap;
use std::path::Path;

/// Every LACCD college abbreviation.
pub const COLLEGES: [&str; 9] = ["ELAC", "LACC", "LAHC", "LAMC", "LAPC", "LASC", "LATTC", "LAVC", "WLAC"];

const SEASONS: [&str; 4] = ["Fall", "Winter", "Spring", "Summer"];
/// pdftotext glues this onto the last line of each page.
const WATERMARK: &str = "U N O F F I C I A L";

pub fn read_pdf(path: &Path) -> Result<String, ParseError> {
    run_pdftotext(path, "-raw")
}

fn is_int(t: &str) -> bool {
    !t.is_empty() && t.bytes().all(|c| c.is_ascii_digit())
}

fn is_note(t: &str) -> bool {
    !t.is_empty() && t.bytes().all(|c| c.is_ascii_uppercase() || c == b'/')
}

/// Parses one course line; `None` if it isn't one.
fn course_line(line: &str, colleges: &[&str]) -> Option<(String, String, String, String, String)> {
    let t: Vec<&str> = line.split_whitespace().collect();
    let mut end = t.len();
    if end > 0 && is_note(t[end - 1]) && !colleges.contains(&t[end - 1]) {
        end -= 1; // CSU/UC, CSU, NDA
    }
    if end < 5 || !is_int(t[end - 1]) {
        return None;
    }
    end -= 1; // grade points
    let grade = if is_units(t[end - 1]) {
        ""
    } else {
        end -= 1;
        t[end]
    };
    if end < 3 || !is_units(t[end - 1]) || !is_units(t[end - 2]) {
        return None;
    }
    let attempted = t[end - 2];
    let title_end = end - 2;
    // The college column: the first known abbreviation after at least one code token.
    let ci = (1..title_end).find(|&i| colleges.contains(&t[i]))?;
    Some((
        t[..ci].join(" "),
        t[ci].to_string(),
        t[ci + 1..title_end].join(" "),
        attempted.to_string(),
        grade.to_string(),
    ))
}

/// Parses `pdftotext -raw` output. `colleges` are the abbreviations to accept.
pub fn parse(text: &str, colleges: &[&str]) -> Result<Vec<TranscriptCourse>, ParseError> {
    let text = text.replace(WATERMARK, "\n");
    let mut courses = Vec::new();
    let (mut year, mut season, mut noncredit) = (String::new(), String::new(), false);
    // (noncredit, year, season) → units: stated by the transcript / summed by us.
    let mut stated: BTreeMap<(bool, String, String), String> = BTreeMap::new();
    let mut summed: BTreeMap<(bool, String, String), u64> = BTreeMap::new();
    let mut months: Option<(u8, u8)> = None;

    for line in text.lines().map(str::trim) {
        if line == "Beginning of Non Credit Record" {
            noncredit = true;
            continue;
        }
        let t: Vec<&str> = line.split_whitespace().collect();
        if t.len() >= 3 && t[0].len() == 4 && is_int(t[0]) && SEASONS.contains(&t[1]) && t[2].starts_with('(') {
            (year, season) = (t[0].to_string(), t[1].to_string());
            // "(Sep 1, 2025 - Dec 21, 2025)": the session's real start and end months.
            let m: Vec<u8> = t[2..].iter().filter_map(|w| crate::month_abbrev(w.trim_start_matches('('))).collect();
            months = (m.len() == 2).then(|| (m[0], m[1]));
            continue;
        }
        if t.len() >= 6 && t[..2] == ["Term", "GPA"] && t[3..5] == ["Term", "Totals"] {
            stated.insert((noncredit, year.clone(), season.clone()), t[5].to_string());
            continue;
        }
        if season.is_empty() {
            continue;
        }
        if let Some((code, college, title, credits, grade)) = course_line(line, colleges) {
            *summed.entry((noncredit, year.clone(), season.clone())).or_default() += hundredths(&credits);
            courses.push(TranscriptCourse {
                year: year.clone(),
                season: season.clone(),
                code,
                college,
                title,
                credits,
                grade,
                noncredit,
                term_months: months,
            });
        }
    }
    if courses.is_empty() {
        return Err(ParseError::Empty);
    }
    // In-progress terms print no totals, so only stated terms are checked.
    let bad: Vec<String> = stated
        .iter()
        .filter(|(k, v)| summed.get(*k).copied().unwrap_or(0) != hundredths(v))
        .map(|((nc, y, s), v)| {
            format!(
                "{} {s} {y}: transcript says {v} units, parsed {:.2}",
                if *nc { "noncredit" } else { "credit" },
                summed.get(&(*nc, y.clone(), s.clone())).copied().unwrap_or(0) as f64 / 100.0
            )
        })
        .collect();
    if !bad.is_empty() {
        return Err(ParseError::Unreconciled(bad));
    }
    Ok(courses)
}

#[cfg(test)]
#[path = "tests/laccd_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "tests/laccd_line_boundaries.rs"]
mod line_boundaries;
