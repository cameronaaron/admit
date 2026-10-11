//! Application deadlines of the selected programs.
//!
//! Each program carries `deadline` (a date) and `deadlineDisplay` ("Jan 15,
//! 2027, 11:59 PM EST"), and they disagree: observed 2026-10, one program
//! showed "Jan 15, 2027" with `deadline` 2027-03-16. The earlier of the two
//! is used, so a warning is never late.

use std::collections::BTreeMap;

use admit_domain::Finding;
use serde_json::Value;
use time::{Date, Month};

use crate::client::{arr, int, s};

/// Days before a deadline at which an unsubmitted program becomes Pending.
pub const SOON_DAYS: i64 = 30;

/// "Jan 15, 2027, 11:59 PM EST" → 2027-01-15.
pub fn display_date(d: &str) -> Option<Date> {
    let mut w = d.split([' ', ',']).filter(|w| !w.is_empty());
    let month = w.next()?.get(..3)?.to_ascii_lowercase();
    // The index is 0..12 whenever the name matches, so `Month::try_from` cannot
    // fail there. Match the abbreviation; an unknown name is `None`.
    let m = match month.as_str() {
        "jan" => Month::January,
        "feb" => Month::February,
        "mar" => Month::March,
        "apr" => Month::April,
        "may" => Month::May,
        "jun" => Month::June,
        "jul" => Month::July,
        "aug" => Month::August,
        "sep" => Month::September,
        "oct" => Month::October,
        "nov" => Month::November,
        "dec" => Month::December,
        _ => return None,
    };
    let day: u8 = w.next()?.parse().ok()?;
    Date::from_calendar_date(w.next()?.parse().ok()?, m, day).ok()
}

fn submitted(p: &Value) -> bool {
    s(&p["submissionStatus"]).to_ascii_lowercase().contains("submitted")
}

/// An unsubmitted program's deadline: the earlier of its two dates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Deadline {
    /// "Organization - Program".
    pub label: String,
    pub date: Date,
    /// As the portal shows it, else `date`.
    pub shown: String,
}

/// An unsubmitted program's deadline: the earlier of its two dates, and
/// how the portal shows it.
fn date_of(p: &Value) -> Option<(Date, String)> {
    if submitted(p) {
        return None;
    }
    let shown = s(&p["deadlineDisplay"]);
    let date =
        [display_date(&shown), admit_domain::validate::parse_date(&s(&p["deadline"]))].into_iter().flatten().min()?;
    Some((date, if shown.is_empty() { date.to_string() } else { shown }))
}

pub fn read(programs: &Value) -> Vec<Deadline> {
    arr(programs)
        .iter()
        .filter_map(|p| {
            let (date, shown) = date_of(p)?;
            Some(Deadline { label: format!("{} - {}", s(&p["organizationName"]), s(&p["programName"])), date, shown })
        })
        .collect()
}

/// Unsubmitted programs' deadlines by `programSelectedId`, the id
/// `/getProgramNavigation` and the evaluations endpoint use.
pub fn by_program(programs: &Value) -> BTreeMap<i64, Date> {
    arr(programs).iter().filter_map(|p| Some((int(&p["programSelectedId"]), date_of(p)?.0))).collect()
}

/// Past: Violation; within [`SOON_DAYS`]: Pending; later: Note.
pub fn grade(d: &Deadline, today: Date) -> Finding {
    let Deadline { label, shown, .. } = d;
    let days = (d.date - today).whole_days();
    if days < 0 {
        Finding::Violation(format!("{label}: deadline {shown} has passed and it is not submitted"))
    } else if days <= SOON_DAYS {
        Finding::Pending(format!("{label}: deadline {shown} is in {days} day(s)"))
    } else {
        Finding::Note(format!("{label}: deadline {shown} ({days} days)"))
    }
}

pub fn check(programs: &Value, today: Date) -> Vec<Finding> {
    read(programs).iter().map(|d| grade(d, today)).collect()
}

#[cfg(test)]
#[path = "tests/deadlines_tests.rs"]
mod tests;
