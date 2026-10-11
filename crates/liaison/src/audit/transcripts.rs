//! Official transcripts per institution (`GET /transcripts`, `college`).
//!
//! Institutions that share one transcript (every LACCD college is on the one
//! district transcript; the groups come from `coursework.toml`) are covered
//! once any of them has an official transcript received: the others become a
//! Note, since NursingCAS still tracks each institution separately and may
//! need to be asked to apply the district transcript to them.

use admit_domain::Finding;
use serde_json::Value;

use crate::client::s;

pub fn check(college: &Value, shared: &[Vec<String>]) -> Vec<Finding> {
    let mut out = Vec::new();
    let Some(map) = college["collegeMap"].as_object() else { return out };
    let received: Vec<String> = map
        .values()
        .filter(|c| c["officialTranscript"]["transcriptReceived"].as_bool() == Some(true))
        .map(|c| s(&c["collegeName"]))
        .collect();
    let covered_by = |name: &str| {
        shared
            .iter()
            .filter(|g| g.iter().any(|n| n.eq_ignore_ascii_case(name)))
            .flat_map(|g| g.iter())
            .find(|n| !n.eq_ignore_ascii_case(name) && received.iter().any(|r| r.eq_ignore_ascii_case(n)))
            .cloned()
    };
    for c in map.values() {
        if c["domesticTranscriptsNotRequired"].as_bool() == Some(true)
            || c["officialTranscript"]["transcriptReceived"].as_bool() == Some(true)
        {
            continue;
        }
        let name = s(&c["collegeName"]);
        if let Some(by) = covered_by(&name) {
            out.push(Finding::Note(format!(
                "official transcript from {name}: on the shared transcript received for {by} \
                 (the portal still lists {name} separately - ask the CAS to apply it if it stays missing)"
            )));
            continue;
        }
        let ordered = s(&c["orderedTRF"]);
        out.push(Finding::Pending(if ordered.is_empty() {
            format!("official transcript from {name} not ordered")
        } else {
            format!("official transcript from {name} ordered {ordered}, not yet received")
        }));
    }
    out.sort_by_key(ToString::to_string);
    out
}

#[cfg(test)]
#[path = "tests/transcripts_tests.rs"]
mod tests;
