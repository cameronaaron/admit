//! Program prerequisites matched to entered courses
//! (`GET /academicHistory/courseWork/prerequisites`). Matching is optional on
//! NursingCAS (the section completes without it), so this is a Note: programs
//! that read the matches see none.

use admit_domain::Finding;
use serde_json::Value;

use crate::client::{arr, s};

pub fn check(prereqs: &Value) -> Vec<Finding> {
    arr(prereqs)
        .iter()
        .filter(|p| p["optOut"].as_bool() != Some(true))
        .filter_map(|p| {
            let all = arr(&p["prerequisites"]);
            let matched = all.iter().filter(|q| q["courseMatched"].as_bool() == Some(true)).count();
            (matched < all.len()).then(|| {
                Finding::Note(format!(
                    "{} - {}: {matched} of {} prerequisites matched to courses",
                    s(&p["organizationName"]),
                    s(&p["programName"]),
                    all.len()
                ))
            })
        })
        .collect()
}

#[cfg(test)]
#[path = "tests/prerequisites_tests.rs"]
mod tests;
