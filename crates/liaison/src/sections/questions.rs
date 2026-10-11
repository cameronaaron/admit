//! Each program's own questions (`questions.toml`): choices by option text,
//! keyed by question id.
//!
//! Captured 2026-10-07 (Yale, one save of seven answers):
//! - `GET /programMaterials/{programSelectedId}/questions`: `questionGroups[]
//!   .questions[]`, each with `input.type`, `value[]` (an option id for a
//!   choice) and `answerOptions[]`. Unlike Biographic Information, the GET
//!   carries the chosen option.
//! - Before saving, the browser POSTs every choice answer to
//!   `/extendedProfile/applicationProgramSelectionImpact?includeJumpstartValidation=false&programSelectedId={id}`,
//!   which says whether the answers would affect other program selections
//!   (the save body has `programDeleted`/`programIdsToDelete`). [`apply`]
//!   makes the same call and refuses to save if it reports any impact.
//! - Save = `POST /programMaterials/{programSelectedId}/questionsPartialSave`
//!   (Yale) with the GET document reshaped as [`body`] says. Once no required
//!   question is left blank, the browser's other save (UC Davis, captured
//!   2026-10-07) is used instead: the same body to `POST
//!   /programMaterials/{id}/questions`, then `PUT
//!   /updateProgramMaterialsStatus/programSelected/{id}/section/{Questions
//!   section}/optOut/false/completionStatus/true`, which marks it done.
//!
//! Written: `radio` and `dropdown` (by option text) and `textarea` (the text,
//! within `maxLength`), the kinds a captured save holds. Both captured saves
//! replay byte for byte.

use std::collections::BTreeMap;

use admit_domain::{Finding, ProgramAnswers};
use serde_json::{Map, Value, json};

use crate::client::{ApiError, Client, Result, arr, int, s};

const CHOICES: [&str; 2] = ["radio", "dropdown"];
/// A text box this large is an essay, and an answer this short in one is a
/// placeholder: UC Davis's statement of interest (5000) held "test" after
/// the save was captured, while short answers belong in its 300- and
/// 1000-character boxes ("LACCD", "N/A").
const ESSAY_MIN_LIMIT: i64 = 2000;
const ESSAY_MIN_ANSWER: usize = 100;
const TEXTS: [&str; 1] = ["textarea"];

/// A program's question document as the portal holds it.
#[derive(Debug, Clone, PartialEq)]
pub struct LiveProgram {
    pub id: i64,
    /// "Organization - Program".
    pub label: String,
    pub doc: Value,
}

/// Every selected program that has a Questions subsection.
pub async fn live(c: &Client) -> Result<Vec<LiveProgram>> {
    let mut out = Vec::new();
    for p in arr(&c.get("/getProgramNavigation").await?) {
        if !arr(&p["subSections"]).iter().any(|x| s(&x["name"]) == "Questions") {
            continue;
        }
        let id = int(&p["programSelectedId"]);
        out.push(LiveProgram {
            id,
            label: format!("{} - {}", s(&p["orgName"]), s(&p["programName"])),
            doc: c.get(&format!("/programMaterials/{id}/questions")).await?,
        });
    }
    Ok(out)
}

fn questions(doc: &Value) -> impl Iterator<Item = &Value> {
    arr(&doc["questionGroups"]).iter().flat_map(|g| arr(&g["questions"]))
}

fn kind(q: &Value) -> String {
    s(&q["input"]["type"])
}

/// The stored answer: its option id(s) or text, blanks dropped.
fn stored(q: &Value) -> Vec<String> {
    arr(&q["value"]).iter().map(s).filter(|v| !v.is_empty()).collect()
}

fn norm(t: &str) -> String {
    t.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

/// A question's answer as `questions.toml` writes it: the option's text for
/// a choice, the text itself otherwise.
fn readable(q: &Value) -> Option<String> {
    let v = stored(q);
    let first = v.first()?;
    if CHOICES.contains(&kind(q).as_str()) {
        return arr(&q["answerOptions"])
            .iter()
            .find(|o| s(&o["code"]) == *first || s(&o["id"]) == *first)
            .map(|o| s(&o["text"]));
    }
    Some(first.clone())
}

/// Plain text from the question's HTML, for comments and findings.
fn text(q: &Value) -> String {
    let mut out = String::new();
    let mut tag = false;
    for ch in s(&q["question"]).chars() {
        match ch {
            '<' => tag = true,
            '>' => {
                tag = false;
                out.push(' ');
            }
            _ if !tag => out.push(ch),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// One answer to write.
#[derive(Debug, Clone, PartialEq)]
pub struct Change {
    pub question_id: i64,
    pub question: String,
    pub from: Option<String>,
    pub to: String,
    /// What is sent: the option id `to` names, or the text itself.
    pub value: String,
}

/// One program's save: its changes and the document they apply to.
#[derive(Debug, Clone, PartialEq)]
pub struct Save {
    pub program_id: i64,
    pub program: String,
    pub doc: Value,
    pub changes: Vec<Change>,
    /// No required question is left blank: save and mark the section done.
    pub complete: bool,
}

/// Shown when a question is visible: every rule (`operator` "or": any of its
/// option ids; "and": all) is met by the answers. No rule: always.
fn visible(q: &Value, chosen: &[String]) -> bool {
    arr(&q["visibilityRules"]).iter().all(|r| {
        let ids: Vec<String> = arr(&r["rules"]).iter().map(s).collect();
        if s(&r["operator"]).eq_ignore_ascii_case("and") {
            ids.iter().all(|i| chosen.contains(i))
        } else {
            ids.iter().any(|i| chosen.contains(i))
        }
    })
}

pub fn plan(answers: &[ProgramAnswers], portal: &str, live: &[LiveProgram]) -> (Vec<Save>, Vec<Finding>) {
    let (mut out, mut f) = (Vec::new(), Vec::new());
    let mine: Vec<&ProgramAnswers> = answers.iter().filter(|a| a.portal == portal).collect();
    for a in &mine {
        if !live.iter().any(|l| norm(&l.label) == norm(&a.name)) {
            f.push(Finding::Violation(format!(
                "questions.toml: {:?} is not a program with questions on {portal}",
                a.name
            )));
        }
    }
    for l in live {
        let want = mine.iter().find(|a| norm(&a.name) == norm(&l.label)).map(|a| &a.answers);
        let mut changes = Vec::new();
        for (id, to) in want.into_iter().flatten() {
            let Some(q) = questions(&l.doc).find(|q| s(&q["id"]) == *id) else {
                f.push(Finding::Violation(format!("{}: no question {id} on the portal", l.label)));
                continue;
            };
            let from = readable(q);
            if from.as_deref().map(norm) == Some(norm(to)) {
                continue;
            }
            if TEXTS.contains(&kind(q).as_str()) {
                let max = int(&q["input"]["maxLength"]);
                let n = to.chars().count();
                if max > 0 && n as i64 > max {
                    f.push(Finding::Violation(format!(
                        "{}: question {id} ({}): {n} characters, the limit is {max}",
                        l.label,
                        text(q)
                    )));
                } else {
                    changes.push(Change {
                        question_id: int(&q["id"]),
                        question: text(q),
                        from,
                        to: to.clone(),
                        value: to.clone(),
                    });
                }
                continue;
            }
            if !CHOICES.contains(&kind(q).as_str()) {
                f.push(Finding::Violation(format!(
                    "{}: question {id} ({}) is a {} answer, which admit does not write (none was captured); enter it in the browser",
                    l.label,
                    text(q),
                    kind(q)
                )));
                continue;
            }
            let options = arr(&q["answerOptions"]);
            match options.iter().find(|o| norm(&s(&o["text"])) == norm(to)) {
                Some(o) => changes.push(Change {
                    question_id: int(&q["id"]),
                    question: text(q),
                    from,
                    to: s(&o["text"]),
                    value: s(&o["code"]),
                }),
                None => f.push(Finding::Violation(format!(
                    "{}: question {id}: {to:?} is not a choice ({})",
                    l.label,
                    options.iter().map(|o| format!("{:?}", s(&o["text"]))).collect::<Vec<_>>().join(", ")
                ))),
            }
        }
        // Required questions still blank once the planned answers are in.
        let mut chosen: Vec<String> = questions(&l.doc).flat_map(stored).collect();
        chosen.extend(changes.iter().map(|c| c.value.clone()));
        let blank: Vec<String> = questions(&l.doc)
            .filter(|q| q["input"]["required"].as_bool() == Some(true) && q["hidden"].as_bool() != Some(true))
            .filter(|q| visible(q, &chosen))
            .filter(|q| stored(q).is_empty() && !changes.iter().any(|c| c.question_id == int(&q["id"])))
            .filter(|q| !want.is_some_and(|w| w.contains_key(&s(&q["id"]))))
            .map(|q| format!("{} ({})", s(&q["id"]), text(q).chars().take(60).collect::<String>()))
            .collect();
        for q in questions(&l.doc).filter(|q| int(&q["input"]["maxLength"]) >= ESSAY_MIN_LIMIT) {
            let id = int(&q["id"]);
            let answer = changes.iter().find(|c| c.question_id == id).map(|c| c.to.clone()).or_else(|| readable(q));
            let Some(a) = answer.filter(|a| !a.trim().is_empty()) else { continue };
            let n = a.trim().chars().count();
            if n < ESSAY_MIN_ANSWER {
                f.push(Finding::Pending(format!(
                    "{}: question {id} ({}) is answered with {n} character(s) ({:?}) of {}: a placeholder?",
                    l.label,
                    text(q).chars().take(60).collect::<String>(),
                    a.trim(),
                    int(&q["input"]["maxLength"])
                )));
            }
        }
        if !blank.is_empty() {
            f.push(Finding::Pending(format!(
                "{}: {} required question(s) unanswered: {}",
                l.label,
                blank.len(),
                blank.join("; ")
            )));
        }
        if !changes.is_empty() {
            let complete = blank.is_empty();
            out.push(Save { program_id: l.id, program: l.label.clone(), doc: l.doc.clone(), changes, complete });
        }
    }
    (out, f)
}

/// An option as the form sends the chosen one.
fn sent_option(o: &Value) -> Value {
    json!({"id": o["id"], "seqNum": o["seqNum"], "expirationDate": o["expirationDate"], "code": o["code"]})
}

/// The save body: the document with `changes` swapped in, reshaped as the
/// form sends it (captured): no `overview`/`orgQuestionsOverview`, null
/// `impactRules`/`programIdsToDelete` as `{}`/`[]`, each group with
/// `additionalQuestions: []` and no `overview`, each question without its
/// `question` text, a blank `value` as `[""]`, and only the chosen option in
/// `answerOptions`.
pub fn body(doc: &Value, changes: &[Change]) -> Value {
    let mut b = doc.clone();
    let Some(top) = b.as_object_mut() else { return b };
    top.remove("overview");
    top.remove("orgQuestionsOverview");
    for (k, empty) in [("impactRules", json!({})), ("programIdsToDelete", json!([]))] {
        if top.get(k).is_none_or(Value::is_null) {
            top.insert(k.into(), empty);
        }
    }
    let groups = top.get_mut("questionGroups").and_then(Value::as_array_mut).into_iter().flatten();
    for g in groups.filter_map(Value::as_object_mut) {
        g.remove("overview");
        g.insert("additionalQuestions".into(), json!([]));
        let qs = g.get_mut("questions").and_then(Value::as_array_mut).into_iter().flatten();
        for q in qs.filter_map(Value::as_object_mut) {
            q.remove("question");
            let id = q.get("id").map(int).unwrap_or_default();
            let mut value: Vec<String> = q.get("value").map(|v| arr(v).iter().map(s).collect()).unwrap_or_default();
            if let Some(c) = changes.iter().find(|c| c.question_id == id) {
                value = vec![c.value.clone()];
            }
            let chosen: Vec<Value> = q
                .get("answerOptions")
                .map(|o| arr(o).iter().filter(|o| value.contains(&s(&o["code"]))).map(sent_option).collect())
                .unwrap_or_default();
            if value.iter().all(String::is_empty) {
                value = vec![String::new()];
            }
            q.insert("value".into(), json!(value));
            q.insert("answerOptions".into(), Value::Array(chosen));
        }
    }
    b
}

/// The impact check's body: every choice question's value, blank as `[""]`.
pub fn impact_body(save_body: &Value) -> Value {
    let mut m = Map::new();
    for q in questions(save_body).filter(|q| CHOICES.contains(&kind(q).as_str())) {
        m.insert(s(&q["id"]), q["value"].clone());
    }
    Value::Object(m)
}

/// What an impact response reports, if anything: programs the answers would
/// remove or conflict with.
fn impacts(r: &Value) -> Option<String> {
    let e = &r["extendedProfileResponse"];
    let removed = arr(&e["applicantTypeProgramSelectionImpactResponse"]["programSelectionImpacts"]);
    let conflicted = arr(&e["conflictedPrograms"]);
    let any = r["hasImpacts"].as_bool() == Some(true)
        || !removed.is_empty()
        || !conflicted.is_empty()
        || e["applicantTypeProgramSelectionImpactResponse"]["allDelete"].as_bool() == Some(true);
    any.then(|| format!("impacts {}, conflicts {}", Value::Array(removed.to_vec()), Value::Array(conflicted.to_vec())))
}

pub async fn apply(c: &Client, saves: &[Save]) -> Result<()> {
    for sv in saves {
        let b = body(&sv.doc, &sv.changes);
        let check = c
            .post(
                &format!(
                    "/extendedProfile/applicationProgramSelectionImpact?includeJumpstartValidation=false&programSelectedId={}",
                    sv.program_id
                ),
                &impact_body(&b),
            )
            .await?;
        if let Some(why) = impacts(&check) {
            return Err(ApiError::Shape(format!(
                "{}: the portal says these answers would affect other program selections ({why}); nothing saved - answer in the browser",
                sv.program
            )));
        }
        if !sv.complete {
            c.post(&format!("/programMaterials/{}/questionsPartialSave", sv.program_id), &b).await?;
            continue;
        }
        c.post(&format!("/programMaterials/{}/questions", sv.program_id), &b).await?;
        let section = int(&sv.doc["supplementalSectionIds"]["Questions"]);
        c.put_empty(&format!(
            "/updateProgramMaterialsStatus/programSelected/{}/section/{section}/optOut/false/completionStatus/true",
            sv.program_id
        ))
        .await?;
    }
    Ok(())
}

/// Every question with its id, type, answer and choices, for filling in
/// `questions.toml` (`open`: only required, visible, unanswered ones).
pub fn listing(live: &[LiveProgram], open: bool) -> String {
    let mut out = String::new();
    for l in live {
        let chosen: Vec<String> = questions(&l.doc).flat_map(stored).collect();
        let shown: Vec<&Value> = questions(&l.doc)
            .filter(|q| kind(q) != "sectiontextbox")
            .filter(|q| {
                !open
                    || (q["input"]["required"].as_bool() == Some(true)
                        && q["hidden"].as_bool() != Some(true)
                        && visible(q, &chosen)
                        && stored(q).is_empty())
            })
            .collect();
        if shown.is_empty() {
            continue;
        }
        out += &format!("\n{}\n", l.label);
        for q in shown {
            let required = if q["input"]["required"].as_bool() == Some(true) { "required " } else { "" };
            let max = int(&q["input"]["maxLength"]);
            let limit = if max > 0 { format!(", max {max} chars") } else { String::new() };
            out += &format!("  {} [{required}{}{limit}] {}\n", s(&q["id"]), kind(q), text(q));
            if let Some(a) = readable(q) {
                out += &format!("      = {a:?}\n");
            }
            if CHOICES.contains(&kind(q).as_str()) {
                for o in arr(&q["answerOptions"]) {
                    out += &format!("      - {:?}\n", s(&o["text"]));
                }
            }
        }
    }
    out
}

/// Every program's answered questions, for `questions.toml`.
pub fn pull(live: &[LiveProgram], portal: &str) -> Vec<ProgramAnswers> {
    live.iter()
        .map(|l| ProgramAnswers {
            name: l.label.clone(),
            portal: portal.to_string(),
            answers: questions(&l.doc).filter_map(|q| Some((s(&q["id"]), readable(q)?))).collect::<BTreeMap<_, _>>(),
        })
        .collect()
}

#[cfg(test)]
#[path = "tests/questions_tests.rs"]
mod tests;
