//! Has the portal's JSON changed under the readers?
//!
//! Every reader here takes fields with `s(&v["x"])`/`int(..)`, which turn a
//! renamed or removed field into `""` or `0` silently. So the shape of each
//! response the readers use (every key path and its JSON type, never a
//! value) is recorded once (`admit drift --record`) and later compared with
//! the live portal: a recorded field that is gone, or whose type changed, is
//! a Violation. Fields the portal adds are a Note.
//!
//! Paths are `$.programs[].deadline`. A key that is not an identifier is
//! data, not schema, and becomes `*`: maps keyed by id (`collegeMap`) or by
//! school name (`orgMap`, live 2026-10-07), whose keys change when a
//! program is added. A key is reported gone only when its parent is
//! present as an object, so an empty list or a null parent proves nothing
//! either way. `null` matches any type: Java serialises unset fields as null.

use std::collections::BTreeMap;

use admit_domain::Finding;
use serde_json::Value;

use crate::client::{Client, Result, arr, int};

/// Key path → JSON type (`object`, `array`, `string`, `number`, `bool`,
/// `null`, or `mixed` when elements of one list disagree).
pub type Shape = BTreeMap<String, String>;

/// Endpoint → its recorded shape.
pub type Shapes = BTreeMap<String, Shape>;

/// The fixed-path reads the readers make; the templated ones follow.
pub const GETS: &[&str] = &[
    "/navigation",
    "/getProgramNavigation",
    "/programsSelected",
    "/transcripts",
    "/authenticatedUser",
    "/academicHistory/courseWork",
    "/academicHistory/courseWork/prerequisites",
    "/academicHistory/colleges",
    "/academicHistory/highSchools",
    "/supportingInfo/achievements",
    "/supportingInfo/experiences",
    "/supportingInfo/licenses/blockName/license2",
    "/lookup/academicStatus",
    "/lookup/achievementTypes",
    "/lookup/collegeMinor",
    "/lookup/countries",
    "/lookup/degreeType",
    "/lookup/employmentStatusTypes",
    "/lookup/experienceTypes",
    "/lookup/license",
    "/lookup/month",
    "/lookup/subjects",
    "/lookup/termType",
    "/lookup/terms",
    "/lookup/year",
    "/programMaterials/prerequisites/transcripts",
];

/// The deadline search (read-only, though it is a POST).
pub const SEARCH: &str = "POST /search/filteredQueries";
/// Every selected program's recommendations, merged into one shape.
pub const EVALUATIONS: &str = "/programMaterials/evaluations/programSelected/{programSelectedId}";
/// Every program's prerequisite list (programs that have one), merged.
pub const PREREQUISITES: &str = "/programMaterials/{programSelectedId}/prerequisites";
/// Every program's own questions (programs that have them), merged.
pub const QUESTIONS: &str = "/programMaterials/{programSelectedId}/questions";
/// Every program's Documents section (programs that have one), merged.
pub const DOCUMENTS: &str = "/programMaterials/{programSelectedId}/documents";
/// Every college's terms and courses, merged.
pub const COURSE_SUMMARY: &str = "/academicHistory/courseWork/summary/{collegeId}";
/// Every experience's question blocks (an edit PUTs all of them back), merged.
pub const EXPERIENCE_BLOCKS: &str = "/questionBlock/supportingInfo/experiences/{experienceId}";
/// The high-school diploma document (POSTed back whole).
pub const HIGH_SCHOOL_INFO: &str = "/academicHistory/highSchoolInfo/{sectionId}";
/// Biographic and Contact Information blocks, merged.
pub const PERSONAL_BLOCKS: &str = "/applicationFormSubSectionBlocks/subSection/{personalInfoSectionId}";

/// The responses a section's plan and writes are built from. `apply`
/// compares these with the recording before writing the section: a body
/// built from a reshaped response is a wrong write, not a failed one.
pub fn reads_for(section: crate::Section) -> &'static [&'static str] {
    use crate::Section::*;
    match section {
        Achievements => &["/supportingInfo/achievements", "/lookup/achievementTypes", "/lookup/countries"],
        Licenses => &["/supportingInfo/licenses/blockName/license2", "/lookup/license", "/lookup/countries"],
        Experiences => &[
            "/supportingInfo/experiences",
            EXPERIENCE_BLOCKS,
            "/lookup/experienceTypes",
            "/lookup/employmentStatusTypes",
        ],
        Education => &[
            "/academicHistory/colleges",
            "/lookup/degreeType",
            "/lookup/collegeMinor",
            "/lookup/month",
            "/lookup/year",
        ],
        Coursework => &[
            "/academicHistory/courseWork",
            COURSE_SUMMARY,
            "/academicHistory/colleges",
            "/lookup/terms",
            "/lookup/termType",
            "/lookup/subjects",
            "/lookup/academicStatus",
            "/lookup/year",
        ],
        HighSchool => {
            &["/academicHistory/highSchools", "/navigation", HIGH_SCHOOL_INFO, "/lookup/month", "/lookup/year"]
        }
        Prerequisites => &["/getProgramNavigation", PREREQUISITES, "/programMaterials/prerequisites/transcripts"],
        Questions => &["/getProgramNavigation", QUESTIONS],
        Documents => &["/getProgramNavigation", DOCUMENTS],
        References => &["/getProgramNavigation", EVALUATIONS],
    }
}

fn kind(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

/// `deadlineDisplay`, `_links`: a field. `74212`, `Yale University`: data.
fn field_name(k: &str) -> bool {
    k.chars().next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn record(s: &mut Shape, path: &str, k: &str) {
    let merged = match s.get(path).map(String::as_str) {
        None | Some("null") => k,
        Some(old) if old == k || k == "null" => return,
        Some(_) => "mixed",
    };
    s.insert(path.to_string(), merged.to_string());
}

fn add(s: &mut Shape, path: &str, v: &Value) {
    record(s, path, kind(v));
    match v {
        Value::Object(m) => {
            for (k, x) in m {
                let k = if field_name(k) { k.as_str() } else { "*" };
                add(s, &format!("{path}.{k}"), x);
            }
        }
        Value::Array(a) => {
            for x in a {
                add(s, &format!("{path}[]"), x);
            }
        }
        _ => {}
    }
}

pub fn shape(v: &Value) -> Shape {
    let mut s = Shape::new();
    add(&mut s, "$", v);
    s
}

/// Folds `other` into `into`, as if both were elements of one list.
pub fn merge(into: &mut Shape, other: &Shape) {
    for (p, k) in other {
        record(into, p, k);
    }
}

/// The path a key hangs from: `$.a[].b` → `$.a[]`. `None` for `$` and for
/// list elements (`$.a[]`), whose absence only means the list is empty.
fn object_parent(path: &str) -> Option<&str> {
    if path.ends_with("[]") {
        return None;
    }
    path.rfind('.').map(|i| &path[..i])
}

/// One endpoint's live shape against its recorded one.
pub fn compare(endpoint: &str, recorded: &Shape, live: &Shape) -> Vec<Finding> {
    let mut out = Vec::new();
    for (path, was) in recorded {
        match live.get(path).map(String::as_str) {
            Some(now) => {
                let comparable = |k: &str| k != "null" && k != "mixed";
                if comparable(was) && comparable(now) && was != now {
                    out.push(Finding::Violation(format!("{endpoint}: {path} was {was}, is now {now}")));
                }
            }
            None => {
                let Some(parent) = object_parent(path).filter(|p| live.get(*p).map(String::as_str) == Some("object"))
                else {
                    continue;
                };
                // The portal leaves an empty list out instead of sending `[]`
                // (`achievements` before any existed, 2026-10-06). A list is
                // only gone if a key the recording lacks took its place.
                let renamed = || live.keys().any(|k| !recorded.contains_key(k) && object_parent(k) == Some(parent));
                if was == "array" && !renamed() {
                    out.push(Finding::Note(format!("{endpoint}: {path} absent (an empty list is left out)")));
                } else {
                    out.push(Finding::Violation(format!("{endpoint}: {path} ({was}) is gone")));
                }
            }
        }
    }
    let new = live.keys().filter(|p| !recorded.contains_key(*p)).count();
    if new > 0 {
        out.push(Finding::Note(format!(
            "{endpoint}: {new} field(s) not in the recording (admit drift --record accepts them)"
        )));
    }
    out
}

/// Every endpoint's comparison. An endpoint recorded but not fetched, or
/// fetched but never recorded, is a Violation: the check would be blind.
pub fn check(recorded: &Shapes, live: &Shapes) -> Vec<Finding> {
    let mut out = Vec::new();
    for (endpoint, r) in recorded {
        match live.get(endpoint) {
            Some(l) => out.extend(compare(endpoint, r, l)),
            None => out.push(Finding::Violation(format!("{endpoint}: recorded but no longer read"))),
        }
    }
    for endpoint in live.keys().filter(|e| !recorded.contains_key(*e)) {
        out.push(Finding::Violation(format!("{endpoint}: read but never recorded (admit drift --record)")));
    }
    out
}

/// Every watched endpoint and its shape.
pub async fn fetch(c: &Client) -> Result<Shapes> {
    fetch_only(c, None).await
}

/// The shapes of `only` (every watched endpoint when `None`). A templated
/// endpoint fetches the list its ids come from even when that list isn't
/// asked for.
pub async fn fetch_only(c: &Client, only: Option<&[&str]>) -> Result<Shapes> {
    let want = |e: &str| only.is_none_or(|o| o.contains(&e));
    let needs = |e: &str, by: &[&str]| want(e) || by.iter().any(|b| want(b));
    let mut got: BTreeMap<&str, Value> = BTreeMap::new();
    for path in GETS {
        let feeds: &[&str] = match *path {
            "/getProgramNavigation" => &[EVALUATIONS, PREREQUISITES, QUESTIONS, DOCUMENTS],
            "/programsSelected" => &[SEARCH],
            "/academicHistory/courseWork" => &[COURSE_SUMMARY],
            "/supportingInfo/experiences" => &[EXPERIENCE_BLOCKS],
            "/navigation" => &[HIGH_SCHOOL_INFO, PERSONAL_BLOCKS],
            _ => &[],
        };
        if needs(path, feeds) {
            got.insert(*path, c.get(path).await?);
        }
    }
    let mut out: Shapes = got.iter().filter(|(p, _)| want(p)).map(|(p, v)| ((*p).to_string(), shape(v))).collect();
    if want(SEARCH) {
        out.insert(SEARCH.into(), shape(&crate::audit::selected_program_details(c, &got["/programsSelected"]).await?));
    }
    let mut merged = |endpoint: &str, docs: Vec<Value>| {
        if want(endpoint) {
            let mut sh = Shape::new();
            docs.iter().for_each(|d| merge(&mut sh, &shape(d)));
            out.insert(endpoint.into(), sh);
        }
    };
    let (mut ev, mut pre, mut qs, mut docs) = (vec![], vec![], vec![], vec![]);
    for p in got.get("/getProgramNavigation").map(arr).unwrap_or_default() {
        let id = int(&p["programSelectedId"]);
        let has = |name: &str| arr(&p["subSections"]).iter().any(|x| crate::client::s(&x["name"]) == name);
        if want(EVALUATIONS) {
            ev.push(c.get(&format!("/programMaterials/evaluations/programSelected/{id}")).await?);
        }
        if want(PREREQUISITES) && p["selectedProgramHasPreReqSection"].as_bool() == Some(true) {
            pre.push(c.get(&format!("/programMaterials/{id}/prerequisites")).await?);
        }
        if want(QUESTIONS) && has("Questions") {
            qs.push(c.get(&format!("/programMaterials/{id}/questions")).await?);
        }
        if want(DOCUMENTS) && has("Documents") {
            docs.push(c.get(&format!("/programMaterials/{id}/documents")).await?);
        }
    }
    let mut summaries = vec![];
    if want(COURSE_SUMMARY) {
        for col in got.get("/academicHistory/courseWork").map(arr).unwrap_or_default() {
            summaries.push(c.get(&format!("/academicHistory/courseWork/summary/{}", int(&col["id"]))).await?);
        }
    }
    let mut blocks = vec![];
    if want(EXPERIENCE_BLOCKS) {
        for e in got.get("/supportingInfo/experiences").map(|v| arr(&v["experiences"])).unwrap_or_default() {
            blocks.push(c.get(&format!("{}/{}", crate::experiences::BLOCKS, int(&e["id"]))).await?);
        }
    }
    let nav = got.get("/navigation").cloned().unwrap_or_default();
    let mut info = vec![];
    if want(HIGH_SCHOOL_INFO)
        && let Some(id) = crate::high_school::find_section(&nav, crate::high_school::SECTION)
    {
        info.push(c.get(&format!("/academicHistory/highSchoolInfo/{id}")).await?);
    }
    let mut personal = vec![];
    if want(PERSONAL_BLOCKS) {
        for name in ["biographicInfo", "contactInfo"] {
            if let Some(id) = crate::personal::section_id(&nav, name) {
                personal.push(c.get(&format!("/applicationFormSubSectionBlocks/subSection/{id}")).await?);
            }
        }
    }
    merged(EVALUATIONS, ev);
    merged(PREREQUISITES, pre);
    merged(QUESTIONS, qs);
    merged(DOCUMENTS, docs);
    merged(COURSE_SUMMARY, summaries);
    merged(EXPERIENCE_BLOCKS, blocks);
    merged(HIGH_SCHOOL_INFO, info);
    merged(PERSONAL_BLOCKS, personal);
    Ok(out)
}

/// `check` for a fetch of some endpoints only: each one fetched against its
/// recording; nothing is said about the endpoints not fetched.
pub fn check_only(recorded: &Shapes, live: &Shapes) -> Vec<Finding> {
    let mut out = Vec::new();
    for (endpoint, l) in live {
        match recorded.get(endpoint) {
            Some(r) => out.extend(compare(endpoint, r, l)),
            None => out.push(Finding::Violation(format!("{endpoint}: read but never recorded (admit drift --record)"))),
        }
    }
    out
}

#[cfg(test)]
#[path = "tests/drift_tests.rs"]
mod tests;
