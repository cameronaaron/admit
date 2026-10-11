//! Portal-independent checks on a profile. Limits that come from a specific
//! portal's form schema are noted where they were observed.

use std::collections::BTreeSet;

use time::macros::format_description;
use time::{Date, Month};

mod cross;
mod degrees;
mod experiences;
mod high_school;
mod licenses;
pub mod personal;
mod references;
mod text;

use crate::{Finding, Profile};

/// Field limits from the Liaison achievement form schema (yup `max(…)` in the
/// applicant bundle, 2026-10).
pub const ACHIEVEMENT_NAME_MAX: usize = 60;
pub const ACHIEVEMENT_ORG_MAX: usize = 60;
pub const ACHIEVEMENT_DESC_MAX: usize = 600;
/// Liaison truncates license titles at 50 characters (observed on a stored
/// record, not read from a schema).
pub const LICENSE_TITLE_MAX: usize = 50;
/// Combined weekly hours across overlapping experiences above which a
/// reviewer can't take the hours at face value. A judgment call: two
/// full-time jobs (80h) is already the edge of credible.
pub const MAX_PLAUSIBLE_WEEKLY_HOURS: u32 = 80;

pub fn parse_date(s: &str) -> Option<Date> {
    Date::parse(s, format_description!("[year]-[month]-[day]")).ok()
}

fn month_index(d: Date) -> i32 {
    d.year() * 12 + (d.month() as i32 - 1)
}

fn month_label(i: i32) -> String {
    let m = Month::try_from((i.rem_euclid(12) + 1) as u8).unwrap_or(Month::January);
    format!("{m} {}", i.div_euclid(12))
}

fn month_number(name: &str) -> Option<u8> {
    (1..=12u8).find(|&m| Month::try_from(m).map(|mm| mm.to_string().eq_ignore_ascii_case(name)).unwrap_or(false))
}

fn dupes(keys: impl Iterator<Item = String>, what: &str, out: &mut Vec<Finding>) {
    let mut seen = BTreeSet::new();
    for k in keys {
        if !seen.insert(k.clone()) {
            out.push(Finding::Violation(format!("{what} {k:?} appears twice")));
        }
    }
}

fn too_long(what: &str, field: &str, value: Option<&str>, max: usize, out: &mut Vec<Finding>) {
    if let Some(v) = value {
        let n = v.chars().count();
        if n > max {
            out.push(Finding::Violation(format!("{what}: {field} is {n} characters (max {max})")));
        }
    }
}

/// Every check, given today's date (passed in so tests are deterministic).
pub fn validate(p: &Profile, today: Date) -> Vec<Finding> {
    let mut out = Vec::new();

    for a in &p.achievements.achievements {
        let what = format!("achievement {:?}", a.name);
        if a.name.trim().is_empty() {
            out.push(Finding::Violation("an achievement has no name".into()));
        }
        too_long(&what, "name", Some(&a.name), ACHIEVEMENT_NAME_MAX, &mut out);
        too_long(&what, "organization", a.organization.as_deref(), ACHIEVEMENT_ORG_MAX, &mut out);
        too_long(&what, "description", a.description.as_deref(), ACHIEVEMENT_DESC_MAX, &mut out);
        if let Some(d) = &a.date {
            match parse_date(d) {
                None => out.push(Finding::Violation(format!("{what}: date {d:?} is not YYYY-MM-DD"))),
                Some(d) if d > today => out.push(Finding::Violation(format!("{what}: date {d} is in the future"))),
                _ => {}
            }
        }
    }
    dupes(p.achievements.achievements.iter().map(|a| a.name.clone()), "achievement", &mut out);

    licenses::check(p, today, &mut out);
    experiences::check(p, today, &mut out);
    degrees::check(p, today, &mut out);
    high_school::check(p, today, &mut out);
    cross::check(p, today, &mut out);
    references::check(p, today, &mut out);
    text::check(p, &mut out);
    out
}

#[cfg(test)]
#[path = "tests/validate_tests.rs"]
mod tests;
