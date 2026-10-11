//! "Still attending" against the terms entered for each institution
//! (`GET /academicHistory/courseWork` + each college's term summary).
//! Programs read the flag to decide whether to expect more grades.

use admit_domain::Finding;

use crate::client::s;
use crate::coursework::{LiveCollege, season_rank};

pub fn check(live: &[LiveCollege]) -> Vec<Finding> {
    let mut out = Vec::new();
    for l in live {
        let (name, c) = (&l.name, &l.info);
        let in_progress: Vec<String> = l
            .terms
            .iter()
            .filter(|t| t["termInProgress"].as_bool() == Some(true))
            .map(|t| format!("{} {}", s(&t["term"]), s(&t["year"])))
            .collect();
        match (c["stillAttending"].as_bool() == Some(true), in_progress.is_empty()) {
            (false, false) => out.push(Finding::Violation(format!(
                "{name}: not marked still attending, but {} is in progress",
                in_progress.join(", ")
            ))),
            (true, true) if !l.terms.is_empty() => {
                // Summary order is not documented: take the latest by date.
                let last = l
                    .terms
                    .iter()
                    .max_by_key(|t| (s(&t["year"]), season_rank(&s(&t["term"]))))
                    .map(|t| format!("{} {}", s(&t["term"]), s(&t["year"])));
                out.push(Finding::Pending(format!(
                    "{name}: marked still attending, but no term is in progress (last entered: {})",
                    last.unwrap_or_default()
                )))
            }
            _ => {}
        }
    }
    out
}

#[cfg(test)]
#[path = "tests/attendance_tests.rs"]
mod tests;
