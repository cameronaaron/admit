//! Each program's recommendation requests (`references.toml`). Requesting
//! one emails the recommender, so a request is only ever planned for an
//! address the program has not been sent before.
//!
//! Read from the applicant bundle (2026-10-07); no request was captured:
//! - `GET /programMaterials/evaluations/programSelected/{programSelectedId}`:
//!   `evaluationTypes[]` (`type`, `evaluationTypeConstant`,
//!   `evaluationTypeDisplayName`, `minRequired`, `maxAllowed`), the requests
//!   made (`data.evaluations[]` / `allEvaluationRequestList.evaluations[]`,
//!   each with its `evaluator.emailAddress`) and `evalSubmitDeadline`.
//! - On leaving the email field the form calls `GET
//!   /emailVerification/pooledEvaluations/email/{email}/evaluationType/{type}`
//!   and shows its failure; [`apply`] makes the same call first.
//! - Request = `POST /programMaterials/evaluations/programSelected/{programSelectedId}`
//!   with [`body`] (the bundle's `vh`).
//! - The form requires a first and last name, an email, a due date between
//!   today and `evalSubmitDeadline` (checked with `parseISO`, so
//!   `YYYY-MM-DD`), a message, a waiver answer and both permissions.

use admit_domain::{Finding, Recommender, ReferencesFile};
use serde_json::{Value, json};

use crate::client::{ApiError, Client, Result, arr, int, s};

/// A program's evaluation section as the portal holds it.
#[derive(Debug, Clone, PartialEq)]
pub struct LiveProgram {
    pub id: i64,
    /// "Organization - Program".
    pub label: String,
    pub doc: Value,
}

/// Every selected program that asks for evaluations.
pub async fn live(c: &Client) -> Result<Vec<LiveProgram>> {
    let mut out = Vec::new();
    for p in arr(&c.get("/getProgramNavigation").await?) {
        if !arr(&p["subSections"]).iter().any(|x| s(&x["name"]) == "Evaluation Types") {
            continue;
        }
        let id = int(&p["programSelectedId"]);
        out.push(LiveProgram {
            id,
            label: format!("{} - {}", s(&p["orgName"]), s(&p["programName"])),
            doc: c.get(&format!("/programMaterials/evaluations/programSelected/{id}")).await?,
        });
    }
    Ok(out)
}

fn norm(t: &str) -> String {
    t.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

/// Addresses the program has already sent a request to.
fn requested(doc: &Value) -> Vec<String> {
    let lists = [&doc["data"]["evaluations"], &doc["allEvaluationRequestList"]["evaluations"]];
    let mut out: Vec<String> = lists
        .iter()
        .flat_map(|l| arr(l))
        .map(|e| s(&e["evaluator"]["emailAddress"]).trim().to_lowercase())
        .filter(|e| !e.is_empty())
        .collect();
    out.sort();
    out.dedup();
    out
}

/// One letter to request.
#[derive(Debug, Clone, PartialEq)]
pub struct Request {
    pub program_id: i64,
    pub program: String,
    pub recommender: Recommender,
    /// The program's evaluation type this letter is for.
    pub kind: Value,
}

pub fn plan(file: &ReferencesFile, live: &[LiveProgram]) -> (Vec<Request>, Vec<Finding>) {
    let (mut out, mut f) = (Vec::new(), Vec::new());
    if !file.consent {
        // validate reports it; nothing is planned without it.
        return (out, f);
    }
    for r in &file.recommenders {
        let email = r.email.trim();
        for name in &r.programs {
            let Some(l) = live.iter().find(|l| norm(&l.label) == norm(name)) else {
                f.push(Finding::Violation(format!(
                    "references.toml: {name:?} ({email}) is not a program asking for evaluations"
                )));
                continue;
            };
            if requested(&l.doc).contains(&email.to_lowercase()) {
                continue;
            }
            let types = arr(&l.doc["evaluationTypes"]);
            let kind = match &r.kind {
                Some(k) => types.iter().find(|t| norm(&s(&t["type"])) == norm(k)),
                None if types.len() == 1 => types.first(),
                None => None,
            };
            let Some(kind) = kind else {
                let names: Vec<String> = types.iter().map(|t| format!("{:?}", s(&t["type"]))).collect();
                f.push(Finding::Violation(format!(
                    "{}: {email}: type = one of {}",
                    l.label,
                    if names.is_empty() { "(none offered)".into() } else { names.join(", ") }
                )));
                continue;
            };
            let last = s(&l.doc["evalSubmitDeadline"]);
            if !last.is_empty() && r.due.as_str() > last.as_str() {
                f.push(Finding::Violation(format!(
                    "{}: {email}: due {} is after the evaluation deadline {last}",
                    l.label, r.due
                )));
                continue;
            }
            let max = int(&kind["maxAllowed"]);
            let planned = out.iter().filter(|q: &&Request| q.program_id == l.id).count();
            if max > 0 && (requested(&l.doc).len() + planned) as i64 >= max {
                f.push(Finding::Violation(format!(
                    "{}: {email}: the program takes at most {max} evaluation(s)",
                    l.label
                )));
                continue;
            }
            out.push(Request {
                program_id: l.id,
                program: l.label.clone(),
                recommender: r.clone(),
                kind: kind.clone(),
            });
        }
    }
    (out, f)
}

/// The request body, as the form's `vh` builds it from its fields.
pub fn body(q: &Request) -> Value {
    let r = &q.recommender;
    let t = &q.kind;
    let display = if t["evaluationTypeDisplayName"].is_null() { &t["type"] } else { &t["evaluationTypeDisplayName"] };
    json!({
        "committeeEvaluation": s(&t["type"]) == "Committee Letters",
        "evaluationType": t["evaluationTypeConstant"],
        "evaluationTypeDisplayName": display,
        "permissionForSchoolsToContactReference": true,
        "permissionToContactReference": true,
        "waiverOfEvaluation": r.waive,
        "evaluator": {
            "dueDate": r.due,
            "emailAddress": r.email.trim(),
            "evalFirstName": r.first_name.trim(),
            "evalLastName": r.last_name.trim(),
            "message": r.message.trim(),
            "strDueDate": r.due,
        },
        "reSendEvaluatorEmail": false,
    })
}

/// The email check's path (the browser's `encodeURI` of the concatenation).
pub fn verify_path(q: &Request) -> String {
    let enc = |x: &str| x.replace('%', "%25").replace(' ', "%20");
    format!(
        "/emailVerification/pooledEvaluations/email/{}/evaluationType/{}",
        enc(q.recommender.email.trim()),
        enc(&s(&q.kind["evaluationTypeConstant"]))
    )
}

pub async fn apply(c: &Client, requests: &[Request]) -> Result<()> {
    for q in requests {
        c.get(&verify_path(q)).await.map_err(|e| {
            ApiError::Shape(format!(
                "{}: the portal refused {} ({e}); nothing requested",
                q.program, q.recommender.email
            ))
        })?;
        c.post(&format!("/programMaterials/evaluations/programSelected/{}", q.program_id), &body(q)).await?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "tests/references_tests.rs"]
mod tests;
