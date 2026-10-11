//! Recommendation letters per program
//! (`GET /programMaterials/evaluations/programSelected/{programSelectedId}`).
//! `evalReqLastDate` is not reported: observed 2026-10 it is the same
//! cycle-end date (2028-01-28) for every program, later than any deadline.

use admit_domain::Finding;
use serde_json::Value;
use time::Date;

use crate::client::{arr, int};

/// One program's recommendation letters against what it requires.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recs {
    /// "Organization - Program".
    pub label: String,
    pub need: i64,
    pub requested: i64,
    pub received: i64,
    /// The program's application deadline, unless it is submitted.
    pub deadline: Option<Date>,
}

/// `None` when the program asks for none (no minimum, or opted out).
pub fn read(label: &str, ev: &Value, deadline: Option<Date>) -> Option<Recs> {
    if ev["optOut"].as_bool() == Some(true) {
        return None;
    }
    let need = arr(&ev["evaluationTypes"]).iter().map(|t| int(&t["minRequired"])).max().unwrap_or(0);
    (need > 0).then(|| Recs {
        label: label.to_string(),
        need,
        requested: int(&ev["evaluationsRequested"]),
        received: int(&ev["evaluationsCompleted"]),
        deadline,
    })
}

pub fn check(label: &str, ev: &Value) -> Vec<Finding> {
    let Some(r) = read(label, ev, None) else { return vec![] };
    if r.requested < r.need {
        vec![Finding::Pending(format!("{label}: {} of {} recommendation(s) requested", r.requested, r.need))]
    } else if r.received < r.need {
        vec![Finding::Pending(format!("{label}: {} of {} recommendation(s) received", r.received, r.need))]
    } else {
        vec![]
    }
}

#[cfg(test)]
#[path = "tests/evaluations_tests.rs"]
mod tests;
