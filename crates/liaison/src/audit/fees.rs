//! Application fees (`GET /programsSelected`). A program is not submitted
//! until it is paid or its fee is waived.

use admit_domain::Finding;
use serde_json::Value;

use crate::client::{arr, s};

pub fn check(selected: &Value) -> Vec<Finding> {
    let unpaid: Vec<String> = arr(selected)
        .iter()
        .filter(|p| p["paymentStatus"].as_bool() != Some(true))
        .filter(|p| !s(&p["submissionStatus"]).to_ascii_lowercase().contains("submitted"))
        .map(|p| s(&p["programName"]))
        .collect();
    if unpaid.is_empty() {
        return vec![];
    }
    let waiver = arr(selected).iter().map(|p| s(&p["feeWaiverStatus"])).find(|w| !w.is_empty());
    vec![Finding::Pending(format!(
        "{} program(s) not paid{}: {}",
        unpaid.len(),
        waiver.map(|w| format!(" (fee waiver {w})")).unwrap_or_default(),
        unpaid.join("; ")
    ))]
}

#[cfg(test)]
#[path = "tests/fees_tests.rs"]
mod tests;
