//! Degrees: status must agree with the date (the Liaison form enforces it).

use time::Date;

use super::{dupes, month_number};
use crate::{DegreeStatus, Finding, Profile};

pub(super) fn check(p: &Profile, today: Date, out: &mut Vec<Finding>) {
    let now = today.year() * 12 + (today.month() as i32 - 1);
    for c in &p.education.colleges {
        dupes(c.degrees.iter().map(|d| d.key()), &format!("{} degree", c.name), out);
        for d in &c.degrees {
            let what = format!("{} {}", c.name, d.key());
            let Some(m) = month_number(&d.month) else {
                out.push(Finding::Violation(format!("{what}: month {:?} is not a month name", d.month)));
                continue;
            };
            let at = d.year * 12 + (m as i32 - 1);
            match d.status {
                // The Liaison form rejects these ("verify the expected degree date").
                DegreeStatus::Expected if at <= now => out
                    .push(Finding::Violation(format!("{what}: expected {} {} is not in the future", d.month, d.year))),
                DegreeStatus::Awarded if at > now => {
                    out.push(Finding::Violation(format!("{what}: awarded {} {} is in the future", d.month, d.year)))
                }
                _ => {}
            }
        }
    }
}
