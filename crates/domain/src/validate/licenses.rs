//! Licenses and certifications: field limits and expiry.

use time::Date;

use super::{LICENSE_TITLE_MAX, dupes, parse_date, too_long};
use crate::{Finding, Profile};

/// A credential expiring within this many days is flagged for renewal: an
/// application read in the next admissions cycle should not show it lapsed.
pub const RENEWAL_WINDOW_DAYS: i64 = 90;

pub(super) fn check(p: &Profile, today: Date, out: &mut Vec<Finding>) {
    for l in &p.licenses.licenses {
        let what = format!("license {:?}", l.title);
        too_long(&what, "title", Some(&l.title), LICENSE_TITLE_MAX, out);
        let Some(d) = &l.valid_until else { continue };
        match parse_date(d) {
            None => out.push(Finding::Violation(format!("{what}: valid_until {d:?} is not YYYY-MM-DD"))),
            Some(until) if until < today => out.push(Finding::Violation(format!("{what}: expired {until}"))),
            Some(until) if (until - today).whole_days() <= RENEWAL_WINDOW_DAYS => {
                out.push(Finding::Pending(format!("{what}: expires {until} - renew it")))
            }
            Some(_) => {}
        }
    }
    dupes(p.licenses.licenses.iter().map(|l| l.title.clone()), "license", out);
}
