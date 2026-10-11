//! Section completion, as the portal's own navigation reports it.

use admit_domain::Finding;
use serde_json::Value;

use crate::client::{arr, s};

fn sections(v: &Value, out: &mut Vec<(String, bool, bool)>) {
    match v {
        Value::Object(m) => {
            if m.contains_key("name") && m.contains_key("completed") && m.contains_key("sectionRequired") {
                out.push((
                    s(&v["title"]),
                    v["completed"].as_i64() == Some(1),
                    v["sectionRequired"].as_bool() == Some(true),
                ));
            }
            m.values().for_each(|x| sections(x, out));
        }
        Value::Array(a) => a.iter().for_each(|x| sections(x, out)),
        _ => {}
    }
}

pub fn from_navigation(nav: &Value) -> Vec<Finding> {
    let mut all = Vec::new();
    sections(nav, &mut all);
    all.into_iter()
        .filter(|(_, done, _)| !done)
        .map(|(title, _, required)| {
            if required {
                Finding::Violation(format!("required section {title:?} is not complete"))
            } else {
                Finding::Pending(format!("section {title:?} is not marked complete"))
            }
        })
        .collect()
}

pub fn from_programs(programs: &Value) -> Vec<Finding> {
    let mut out = Vec::new();
    for p in arr(programs) {
        let todo: Vec<String> = arr(&p["subSections"])
            .iter()
            .filter(|x| x["completed"].as_bool() != Some(true))
            .map(|x| {
                let t = s(&x["title"]);
                if t.is_empty() { s(&x["name"]) } else { t }
            })
            .collect();
        if !todo.is_empty() {
            out.push(Finding::Pending(format!(
                "{} - {} [{}]: {}",
                s(&p["orgName"]),
                s(&p["programName"]),
                s(&p["status"]),
                todo.join(", ")
            )));
        }
    }
    out
}

#[cfg(test)]
#[path = "tests/navigation_tests.rs"]
mod tests;
