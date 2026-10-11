//! Inputs that come from outside the profile: transcripts and browser captures.
//!
//! Every transcript parser reconciles its result against a total the
//! transcript itself prints, so a row the parser silently skips is an error,
//! not a missing course.

pub mod conncoll;
pub mod elmbridge;
pub mod har;
pub mod laccd;

use std::path::Path;
use std::process::Command;

/// One course as printed on a transcript.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptCourse {
    pub year: String,
    /// "Fall", "Winter", "Spring" or "Summer".
    pub season: String,
    pub code: String,
    /// Where the course was taken, as the transcript labels it (an LACCD
    /// abbreviation like "LAVC", or an institution name for transfer rows).
    pub college: String,
    pub title: String,
    /// Units attempted, as printed ("3.00").
    pub credits: String,
    /// Empty while the course is in progress.
    pub grade: String,
    pub noncredit: bool,
    /// Start and end month (1-12) of the session, when the transcript says.
    pub term_months: Option<(u8, u8)>,
}

#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error("running pdftotext (install poppler): {0}")]
    Pdftotext(std::io::Error),
    #[error("pdftotext failed: {0}")]
    PdftotextStatus(String),
    #[error("no courses found - wrong parser for this transcript?")]
    Empty,
    #[error("parse does not reconcile with the transcript's own totals:\n  {}", .0.join("\n  "))]
    Unreconciled(Vec<String>),
    #[error("unknown transcript parser {0:?} (known: laccd, elmbridge, conncoll)")]
    UnknownParser(String),
}

/// Parsers by name, as used in `coursework.toml`.
pub const PARSERS: [&str; 3] = ["laccd", "elmbridge", "conncoll"];

/// Parses a transcript PDF. `labels` are the transcript's college labels the
/// profile maps (LACCD abbreviations, or transfer institution names).
pub fn parse_file(parser: &str, path: &Path, labels: &[&str]) -> Result<Vec<TranscriptCourse>, ParseError> {
    match parser {
        "laccd" => laccd::parse(&laccd::read_pdf(path)?, labels),
        "elmbridge" => elmbridge::parse(&elmbridge::read_pdf(path)?),
        "conncoll" => conncoll::parse(&conncoll::read_pdf(path)?, labels),
        other => Err(ParseError::UnknownParser(other.into())),
    }
}

pub(crate) fn run_pdftotext(path: &Path, mode: &str) -> Result<String, ParseError> {
    let out = Command::new("pdftotext").arg(mode).arg(path).arg("-").output().map_err(ParseError::Pdftotext)?;
    if !out.status.success() {
        return Err(ParseError::PdftotextStatus(String::from_utf8_lossy(&out.stderr).into_owned()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// "3.00", "12.50": digits, a dot, two digits.
pub(crate) fn is_units(t: &str) -> bool {
    let b = t.as_bytes();
    b.len() >= 4
        && b[b.len() - 3] == b'.'
        && t.split('.').all(|p| !p.is_empty() && p.bytes().all(|c| c.is_ascii_digit()))
}

/// "3.00" → 300; ".00" → 0.
pub(crate) fn hundredths(u: &str) -> u64 {
    u.replace('.', "").parse::<u64>().unwrap_or(0)
}

pub(crate) fn month_abbrev(w: &str) -> Option<u8> {
    const M: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    M.iter().position(|m| *m == w).map(|i| i as u8 + 1)
}

/// Upper-cased alphanumerics only: how course codes and titles are compared
/// ("VOC ED 384 CE" == "VOC ED384CE", "FAM & CS 021" == "FAM &CS 021").
pub fn normalize(s: &str) -> String {
    s.chars().filter(char::is_ascii_alphanumeric).map(|c| c.to_ascii_uppercase()).collect()
}

#[cfg(test)]
#[path = "tests/parse_tests.rs"]
mod parse_tests;
