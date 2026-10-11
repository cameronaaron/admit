//! High school attendance and diploma dates.

use time::Date;

use super::month_index;
use crate::{Finding, Profile};

/// `YYYY-MM` → month index.
pub(super) fn year_month(s: &str) -> Option<i32> {
    let (y, m) = s.split_once('-')?;
    let (y, m): (i32, i32) = (y.parse().ok()?, m.parse().ok()?);
    (y > 1900 && (1..=12).contains(&m) && s.len() == 7).then_some(y * 12 + m - 1)
}

pub(super) fn check(p: &Profile, today: Date, out: &mut Vec<Finding>) {
    let now = month_index(today);
    for h in &p.high_school.high_schools {
        let mut get = |field: &str, v: &str| {
            let r = year_month(v);
            if r.is_none() {
                out.push(Finding::Violation(format!("high school {:?}: {field} {v:?} is not YYYY-MM", h.name)));
            }
            r
        };
        let (from, to, diploma) = (get("from", &h.from), get("to", &h.to), get("diploma_date", &h.diploma_date));
        if let (Some(f), Some(t)) = (from, to)
            && f > t
        {
            out.push(Finding::Violation(format!(
                "high school {:?}: attended to {} is before from {}",
                h.name, h.to, h.from
            )));
        }
        for (field, v) in [("to", to), ("diploma_date", diploma)] {
            if v.is_some_and(|v| v > now) {
                out.push(Finding::Violation(format!("high school {:?}: {field} is in the future", h.name)));
            }
        }
    }
}
