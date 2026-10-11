//! Checks that span sections: dates in one file that another file bounds.

use time::Date;

use super::high_school::year_month;
use super::month_number;
use crate::{DegreeStatus, Finding, Profile};

pub(super) fn check(p: &Profile, _today: Date, out: &mut Vec<Finding>) {
    // A college degree awarded before the high-school diploma is a typo in one
    // of the two dates.
    let Some((hs, diploma)) =
        p.high_school.high_schools.iter().filter_map(|h| Some((h, year_month(&h.diploma_date)?))).min_by_key(|x| x.1)
    else {
        return;
    };
    for c in &p.education.colleges {
        for d in c.degrees.iter().filter(|d| d.status == DegreeStatus::Awarded) {
            let Some(m) = month_number(&d.month) else { continue };
            if d.year * 12 + i32::from(m) - 1 < diploma {
                out.push(Finding::Violation(format!(
                    "{} {}: awarded {} {} is before the {:?} diploma ({})",
                    c.name,
                    d.key(),
                    d.month,
                    d.year,
                    hs.name,
                    hs.diploma_date
                )));
            }
        }
    }
}
