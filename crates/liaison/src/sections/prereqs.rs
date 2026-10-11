//! Program prerequisites: which entered course counts for each requirement
//! of a program, from the rulings in `prerequisites.toml` (`portal = "<this
//! portal>"`, `name` = "Organization - Program" as the portal shows it,
//! requirement names as the portal lists them).
//!
//! Captured 2026-10-07 (California Baptist, 8 courses; Yale, 4):
//! - `GET /programMaterials/{programSelectedId}/prerequisites`: the
//!   requirements, each with its assigned `course[]`. Only programs whose
//!   `/getProgramNavigation` entry has `selectedProgramHasPreReqSection`.
//! - `GET /programMaterials/prerequisites/transcripts`: every entered course
//!   (`data.transcripts[].terms[].courses[]`), the list the form picks from.
//! - Assign = `POST /programMaterials/{programSelectedId}/prerequisites` with
//!   the GET document minus `instructions` and each requirement's `name` and
//!   `description`, and every assigned course as the form's eight fields
//!   ([`form_course`]; `gradeValue` is the course's credits). The browser
//!   adds one course per POST, each body the previous response plus one
//!   course; [`apply`] does the same.
//!
//! Assignments the profile doesn't name are left alone. A requirement whose
//! ruled courses total fewer credits than its `minCredits` is Pending.

use std::collections::{BTreeMap, BTreeSet};

use admit_domain::{CourseRef, Finding, PrerequisiteProgram};
use serde_json::{Value, json};

use crate::client::{Client, Result, arr, int, s};

/// An entered course, as the prerequisite form lists it.
#[derive(Debug, Clone, PartialEq)]
pub struct PoolCourse {
    pub id: i64,
    pub college: String,
    pub code: String,
    pub title: String,
    pub grade: Value,
    /// "3.00".
    pub credits: String,
    pub term: String,
    pub year: String,
}

pub fn pool(v: &Value) -> Vec<PoolCourse> {
    let mut out = Vec::new();
    for t in arr(&v["data"]["transcripts"]) {
        for term in arr(&t["terms"]) {
            for c in arr(&term["courses"]) {
                out.push(PoolCourse {
                    id: int(&c["id"]),
                    college: s(&t["college"]),
                    code: s(&c["code"]),
                    title: s(&c["title"]),
                    grade: c["grade"].clone(),
                    credits: s(&c["credits"]),
                    term: s(&term["term"]),
                    year: s(&term["year"]),
                });
            }
        }
    }
    out
}

/// A program's prerequisite document as the portal holds it.
#[derive(Debug, Clone, PartialEq)]
pub struct LiveProgram {
    pub id: i64,
    /// "Organization - Program".
    pub label: String,
    pub doc: Value,
}

pub async fn live(c: &Client) -> Result<(Vec<LiveProgram>, Vec<PoolCourse>)> {
    let mut programs = Vec::new();
    for p in arr(&c.get("/getProgramNavigation").await?) {
        if p["selectedProgramHasPreReqSection"].as_bool() != Some(true) {
            continue;
        }
        let id = int(&p["programSelectedId"]);
        programs.push(LiveProgram {
            id,
            label: format!("{} - {}", s(&p["orgName"]), s(&p["programName"])),
            doc: c.get(&format!("/programMaterials/{id}/prerequisites")).await?,
        });
    }
    if programs.is_empty() {
        return Ok((programs, vec![]));
    }
    Ok((programs, pool(&c.get("/programMaterials/prerequisites/transcripts").await?)))
}

/// One course to assign to one requirement.
#[derive(Debug, Clone, PartialEq)]
pub struct Assign {
    pub program_id: i64,
    pub program: String,
    pub requirement_id: i64,
    pub requirement: String,
    pub course: PoolCourse,
}

impl std::fmt::Display for Assign {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let c = &self.course;
        write!(
            f,
            "{}: {} <- {} {} ({} {} {})",
            self.program, self.requirement, c.college, c.code, c.title, c.term, c.year
        )
    }
}

fn same(a: &str, b: &str) -> bool {
    a.split_whitespace().eq(b.split_whitespace()) || a.trim().eq_ignore_ascii_case(b.trim())
}

/// The pool course a ruling names: its college label through `colleges`
/// (transcript label → portal institution, from `coursework.toml`).
fn resolve<'a>(
    c: &CourseRef,
    colleges: &BTreeMap<String, String>,
    pool: &'a [PoolCourse],
) -> std::result::Result<&'a PoolCourse, String> {
    let college = colleges.get(&c.college).map_or(c.college.as_str(), String::as_str);
    let found: Vec<&PoolCourse> =
        pool.iter().filter(|p| p.college.eq_ignore_ascii_case(college) && c.same_code(&p.code)).collect();
    match found.as_slice() {
        [one] => Ok(one),
        [] => Err(format!("{} {} is not entered on the portal under {college}", c.college, c.code)),
        many => Err(format!(
            "{} {} matches {} entered courses ({}); prerequisites can't tell which",
            c.college,
            c.code,
            many.len(),
            many.iter().map(|p| format!("{} {}", p.term, p.year)).collect::<Vec<_>>().join(", ")
        )),
    }
}

pub fn plan(
    rulings: &[PrerequisiteProgram],
    portal: &str,
    colleges: &BTreeMap<String, String>,
    live: &[LiveProgram],
    pool: &[PoolCourse],
) -> (Vec<Assign>, Vec<Finding>) {
    let (mut out, mut f) = (Vec::new(), Vec::new());
    let mine: Vec<&PrerequisiteProgram> = rulings.iter().filter(|r| r.portal.as_deref() == Some(portal)).collect();
    for r in &mine {
        if !live.iter().any(|l| same(&l.label, &r.name)) {
            f.push(Finding::Violation(format!(
                "prerequisites.toml: {:?} is not a program with prerequisites on {portal}",
                r.name
            )));
        }
    }
    for l in live {
        let Some(ruling) = mine.iter().find(|r| same(&l.label, &r.name)) else {
            f.push(Finding::Pending(format!(
                "{}: no prerequisite rulings in prerequisites.toml (`admit -p {portal} prereqs` prints its requirements)",
                l.label
            )));
            continue;
        };
        let reqs = arr(&l.doc["prerequisites"]);
        for want in &ruling.requirements {
            let Some(req) = reqs.iter().find(|q| same(&s(&q["name"]), &want.name)) else {
                f.push(Finding::Violation(format!("{}: no prerequisite named {:?} on the portal", l.label, want.name)));
                continue;
            };
            if want.deferred.is_some() {
                continue;
            }
            let assigned: BTreeSet<i64> = arr(&req["course"]).iter().map(|c| int(&c["id"])).collect();
            let mut credits = Some(0.0);
            for c in &want.courses {
                let found = resolve(c, colleges, pool);
                credits = match (&found, credits) {
                    (Ok(p), Some(sum)) => p.credits.parse::<f64>().ok().map(|x| sum + x),
                    _ => None,
                };
                match found {
                    Ok(p) if assigned.contains(&p.id) => {}
                    Ok(p) => out.push(Assign {
                        program_id: l.id,
                        program: l.label.clone(),
                        requirement_id: int(&req["id"]),
                        requirement: want.name.clone(),
                        course: p.clone(),
                    }),
                    Err(e) => f.push(Finding::Violation(format!("{}: {}: {e}", l.label, want.name))),
                }
            }
            // The portal states a minimum (`minCredits`); semester units,
            // like every course entered so far.
            if let (Some(have), Some(need)) = (credits, req["minCredits"].as_f64())
                && !want.courses.is_empty()
                && have < need
            {
                f.push(Finding::Pending(format!(
                    "{}: {}: the ruled course(s) total {have} credit(s); the program asks for {need}",
                    l.label, want.name
                )));
            }
        }
        for req in reqs {
            let name = s(&req["name"]);
            if ruling.requirements.iter().any(|w| same(&w.name, &name)) {
                continue;
            }
            if arr(&req["course"]).is_empty() {
                f.push(Finding::Pending(format!("{}: prerequisite {name:?} has no course and no ruling", l.label)));
            } else {
                f.push(Finding::Note(format!("{}: prerequisite {name:?} is assigned on the portal only", l.label)));
            }
        }
    }
    (out, f)
}

/// A course as the form sends it.
fn form_course(c: &PoolCourse) -> Value {
    json!({"id": c.id, "courseTitle": c.title, "courseCode": c.code, "collegeName": c.college, "grade": c.grade,
        "gradeValue": c.credits, "term": c.term, "termYear": c.year})
}

/// An already-assigned course from a GET or POST response, as the form
/// re-sends it: credits written "3.00" as the course list has them (the
/// response says "3.0").
fn resend(c: &Value) -> Value {
    let raw = s(&c["gradeValue"]);
    form_course(&PoolCourse {
        id: int(&c["id"]),
        college: s(&c["collegeName"]),
        code: s(&c["courseCode"]),
        title: s(&c["courseTitle"]),
        grade: c["grade"].clone(),
        credits: raw.parse::<f64>().map_or(raw.clone(), |x| format!("{x:.2}")),
        term: s(&c["term"]),
        year: s(&c["termYear"]),
    })
}

/// Numbers as the browser's `JSON.stringify` writes them: a whole float is
/// an integer (the GET's `minGrade: 3.0` is posted as `3`, Yale 2026-10-07).
fn js_numbers(v: &mut Value) {
    match v {
        Value::Number(n) => {
            if let Some(f) = n.as_f64().filter(|f| n.is_f64() && f.fract() == 0.0 && f.abs() < 9e15) {
                *v = json!(f as i64);
            }
        }
        Value::Array(a) => a.iter_mut().for_each(js_numbers),
        Value::Object(o) => o.values_mut().for_each(js_numbers),
        _ => {}
    }
}

/// The POST that adds `a` to the program document `doc`.
pub fn body(doc: &Value, a: &Assign) -> Value {
    let mut b = doc.clone();
    js_numbers(&mut b);
    if let Some(o) = b.as_object_mut() {
        o.remove("instructions");
    }
    // `get_mut`, not `b[...]`: indexing a non-object panics.
    if let Some(reqs) = b.get_mut("prerequisites").and_then(Value::as_array_mut) {
        for q in reqs {
            let mut courses: Vec<Value> = arr(&q["course"]).iter().map(resend).collect();
            if int(&q["id"]) == a.requirement_id {
                courses.push(form_course(&a.course));
            }
            if let Some(o) = q.as_object_mut() {
                o.remove("name");
                o.remove("description");
                o.insert("course".into(), Value::Array(courses));
            }
        }
    }
    b
}

/// One POST per course, each on the document the previous one returned.
pub async fn apply(c: &Client, assigns: &[Assign]) -> Result<()> {
    let mut docs: BTreeMap<i64, Value> = BTreeMap::new();
    for a in assigns {
        let path = format!("/programMaterials/{}/prerequisites", a.program_id);
        let doc = match docs.remove(&a.program_id) {
            Some(d) => d,
            None => c.get(&path).await?,
        };
        docs.insert(a.program_id, c.post(&path, &body(&doc, a)).await?);
    }
    Ok(())
}

/// Each live program's requirements as `prerequisites.toml` entries, with
/// the courses already assigned on the portal, for programs the file
/// doesn't rule yet. Courses are written with the transcript label
/// `colleges` maps to their institution.
pub fn to_toml(
    live: &[LiveProgram],
    portal: &str,
    colleges: &BTreeMap<String, String>,
    ruled: &[PrerequisiteProgram],
) -> String {
    let label_of = |college: &str| {
        colleges.iter().find(|(_, v)| v.eq_ignore_ascii_case(college)).map_or(college.to_string(), |(k, _)| k.clone())
    };
    // A JSON string is a valid TOML basic string.
    let q = |x: &str| Value::String(x.to_string()).to_string();
    let mut out = String::new();
    for l in
        live.iter().filter(|l| !ruled.iter().any(|r| r.portal.as_deref() == Some(portal) && same(&r.name, &l.label)))
    {
        out += &format!(
            "\n[[program]]\nname = {}\nportal = {}\nsource = \"the portal's prerequisite list\"\n",
            q(&l.label),
            q(portal)
        );
        for r in arr(&l.doc["prerequisites"]) {
            out += &format!("\n[[program.requirement]]\nname = {}\n", q(&s(&r["name"])));
            out += &format!(
                "# min grade {}, {} credit(s)\n",
                match s(&r["minGradeAlphabetic"]).as_str() {
                    "" => "unstated".to_string(),
                    g => g.to_string(),
                },
                s(&r["minCredits"])
            );
            let courses: Vec<String> = arr(&r["course"])
                .iter()
                .map(|c| {
                    format!(
                        "{{ college = {}, code = {} }}",
                        q(&label_of(&s(&c["collegeName"]))),
                        q(&s(&c["courseCode"]))
                    )
                })
                .collect();
            out += &format!("courses = [{}]\n", courses.join(", "));
        }
    }
    out
}

#[cfg(test)]
#[path = "tests/prereqs_tests.rs"]
mod tests;
