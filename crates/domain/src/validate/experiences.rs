//! Experiences: dates, hours, and overlapping weekly load.

use std::collections::BTreeMap;

use time::Date;

use super::{MAX_PLAUSIBLE_WEEKLY_HOURS, dupes, month_index, month_label, parse_date};
use crate::{ExperienceKind, Finding, Profile};

pub(super) fn check(p: &Profile, today: Date, out: &mut Vec<Finding>) {
    let xs = &p.experiences.experiences;
    dupes(xs.iter().map(|x| x.key()), "experience", out);
    if !xs.is_empty() && !xs.iter().any(|x| x.kind == ExperienceKind::PatientCare) {
        out.push(Finding::Pending(
            "no patient-care experience: nursing programs weigh direct patient contact heavily".into(),
        ));
    }

    // Month index → (total weekly hours, contributing experiences).
    let mut load: BTreeMap<i32, (u32, Vec<String>)> = BTreeMap::new();
    for x in xs {
        let what = format!("experience {:?}", x.key());
        let Some(start) = parse_date(&x.start) else {
            out.push(Finding::Violation(format!("{what}: start {:?} is not YYYY-MM-DD", x.start)));
            continue;
        };
        let end = match (&x.end, x.current) {
            (Some(e), _) => match parse_date(e) {
                Some(e) => e,
                None => {
                    out.push(Finding::Violation(format!("{what}: end {e:?} is not YYYY-MM-DD")));
                    continue;
                }
            },
            (None, true) => today,
            (None, false) => {
                out.push(Finding::Violation(format!("{what}: has no end date and is not current")));
                continue;
            }
        };
        if !x.current && end > today {
            out.push(Finding::Violation(format!("{what}: ends {end} (in the future) but is not marked current")));
        }
        if end < start {
            out.push(Finding::Violation(format!("{what}: ends before it starts")));
        }
        if x.hours_per_week > 168 {
            out.push(Finding::Violation(format!("{what}: {} hours per week", x.hours_per_week)));
        }
        if x.hours_per_week * x.weeks != x.total_hours {
            out.push(Finding::Note(format!(
                "{what}: {} h/week x {} weeks = {}, but total_hours is {}",
                x.hours_per_week,
                x.weeks,
                x.hours_per_week * x.weeks,
                x.total_hours
            )));
        }
        // Months are inclusive of the start month and exclusive of an end on
        // the 1st, matching how month-precision dates are entered.
        let last = if end.day() == 1 && end > start { month_index(end) - 1 } else { month_index(end) };
        for m in month_index(start)..=last {
            let e = load.entry(m).or_default();
            e.0 += x.hours_per_week;
            e.1.push(format!("{} ({}h)", x.title, x.hours_per_week));
        }
    }

    // Report each run of consecutive over-limit months with the same roles once.
    let mut run: Option<(i32, i32, u32, Vec<String>)> = None;
    let flush = |run: &mut Option<(i32, i32, u32, Vec<String>)>, out: &mut Vec<Finding>| {
        if let Some((a, b, h, who)) = run.take() {
            let span = if a == b { month_label(a) } else { format!("{} – {}", month_label(a), month_label(b)) };
            out.push(Finding::Violation(format!(
                "{span}: overlapping experiences claim {h} h/week (limit {MAX_PLAUSIBLE_WEEKLY_HOURS}): {}",
                who.join(", ")
            )));
        }
    };
    for (m, (h, who)) in &load {
        let over = *h > MAX_PLAUSIBLE_WEEKLY_HOURS;
        match &mut run {
            Some((_, b, rh, rw)) if over && *b + 1 == *m && rh == h && rw == who => *b = *m,
            _ => {
                flush(&mut run, out);
                if over {
                    run = Some((*m, *m, *h, who.clone()));
                }
            }
        }
    }
    flush(&mut run, out);
}
