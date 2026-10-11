//! Academic History → Colleges Attended: the degrees listed under each college.
//!
//! A college is edited with one PUT carrying the full degree list, so the
//! plan for a college is "send this list": matched degrees keep their ids,
//! new ones have none, and live degrees the profile doesn't mention are sent
//! back unchanged (never dropped).

use admit_domain::{College, Degree, DegreeStatus, Finding, PortalIds};
use serde_json::{Value, json};

use crate::client::{Client, Result, arr, int, opt, s};
use crate::lookup::Lookups;
use crate::reconcile::{Step, diff};

const COLLEGES: &str = "/academicHistory/colleges";
/// Observed `degreeStatusId`s (no lookup endpoint was seen for them).
const AWARDED: i64 = 431;
const EXPECTED: i64 = 430;

pub fn degree_from_live(d: &Value) -> Degree {
    Degree {
        kind: s(&d["type"]),
        major: s(&d["major"]),
        second_major: opt(&d["secondMajor"]),
        minor: opt(&d["minor"]),
        status: if int(&d["degreeStatusId"]) == EXPECTED { DegreeStatus::Expected } else { DegreeStatus::Awarded },
        month: s(&d["month"]),
        year: s(&d["year"]).parse().unwrap_or(0),
        note: None,
    }
}

pub async fn live(c: &Client) -> Result<Vec<Value>> {
    Ok(arr(&c.get(COLLEGES).await?["collegesList"]).to_vec())
}

pub fn pull(colleges: &[Value]) -> Vec<College> {
    colleges
        .iter()
        .map(|c| College { name: s(&c["college"]), degrees: arr(&c["degrees"]).iter().map(degree_from_live).collect() })
        .collect()
}

/// A college whose degree list must be re-sent.
#[derive(Debug, Clone)]
pub struct CollegeUpdate {
    pub college: Value,
    pub changes: Vec<String>,
    /// Desired degrees paired with the live id they replace (None: new).
    pub degrees: Vec<(Option<i64>, Degree)>,
    /// Live degrees not in the profile, re-sent as they are.
    pub kept: Vec<Value>,
}

static NO_IDS: std::sync::LazyLock<PortalIds> = std::sync::LazyLock::new(PortalIds::new);

pub fn plan(desired: &[College], live: &[Value]) -> (Vec<CollegeUpdate>, Vec<Finding>) {
    let (mut updates, mut findings) = (Vec::new(), Vec::new());
    for want in desired {
        let Some(college) = live.iter().find(|c| s(&c["college"]).eq_ignore_ascii_case(&want.name)) else {
            findings.push(Finding::Pending(format!(
                "college {:?} is not on the application: add it in the portal (it starts a transcript requirement)",
                want.name
            )));
            continue;
        };
        let live_degrees: Vec<(i64, Degree)> =
            arr(&college["degrees"]).iter().map(|d| (int(&d["id"]), degree_from_live(d))).collect();
        // Notes are for people; compare without them.
        let want_cmp: Vec<Degree> = want.degrees.iter().map(|d| Degree { note: None, ..d.clone() }).collect();
        let d = diff("", &want_cmp, &live_degrees, Degree::key, |_| &NO_IDS);
        for extra in &d.extras {
            findings.push(Finding::Note(format!(
                "{}: degree {extra} is on the portal but not in the profile (kept)",
                want.name
            )));
        }
        if d.steps.is_empty() {
            continue;
        }
        let changes = d
            .steps
            .iter()
            .map(|st| match st {
                Step::Create(x) => format!("+ {}", x.key()),
                Step::Update { fields, desired, .. } => format!("~ {} ({})", desired.key(), fields.join(", ")),
            })
            .collect();
        let updated: Vec<i64> =
            d.steps.iter().filter_map(|st| if let Step::Update { id, .. } = st { Some(*id) } else { None }).collect();
        let matched_unchanged: Vec<(i64, Degree)> = {
            // Desired degrees that matched a live one with no changes keep that id.
            let mut out = Vec::new();
            for w in &want_cmp {
                if let Some((id, _)) = live_degrees.iter().find(|(id, l)| l == w && !updated.contains(id)) {
                    out.push((*id, w.clone()));
                }
            }
            out
        };
        let mut degrees: Vec<(Option<i64>, Degree)> =
            matched_unchanged.iter().map(|(i, x)| (Some(*i), x.clone())).collect();
        for st in &d.steps {
            match st {
                Step::Create(x) => degrees.push((None, x.clone())),
                Step::Update { id, desired, .. } => degrees.push((Some(*id), desired.clone())),
            }
        }
        let used: Vec<i64> = degrees.iter().filter_map(|(i, _)| *i).collect();
        let kept = arr(&college["degrees"]).iter().filter(|x| !used.contains(&int(&x["id"]))).cloned().collect();
        updates.push(CollegeUpdate { college: college.clone(), changes, degrees, kept });
    }
    (updates, findings)
}

fn degree_body(id: Option<i64>, d: &Degree, lk: &Lookups) -> Result<Value> {
    let mut b = json!({
        "degreeStatusId": if d.status == DegreeStatus::Expected { EXPECTED } else { AWARDED },
        "typeId": lk.degree_types.id(&d.kind)?,
        "monthId": lk.months.id(&d.month)?,
        "yearId": lk.years.id(&d.year.to_string())?,
        "majorId": lk.majors.id(&d.major)?,
        "year": d.year,
        "monthCode": null,
        "month": d.month,
    });
    if let Some(m) = &d.second_major {
        b["secondMajorId"] = json!(lk.majors.id(m)?);
    }
    if let Some(m) = &d.minor {
        b["minorId"] = json!(lk.majors.id(m)?);
    }
    if let Some(i) = id {
        b["id"] = json!(i);
    }
    Ok(b)
}

/// An untouched live degree in the shape the web app re-sends.
fn kept_body(d: &Value) -> Value {
    let mut b = json!({
        "degreeStatusId": d["degreeStatusId"], "typeId": d["typeId"], "monthId": d["monthId"],
        "yearId": d["yearId"], "majorId": d["majorId"], "year": s(&d["year"]).parse::<i64>().unwrap_or(0),
        "monthCode": null, "month": d["month"], "id": d["id"],
    });
    for k in ["secondMajorId", "minorId"] {
        if int(&d[k]) != 0 {
            b[k] = d[k].clone();
        }
    }
    b
}

/// The PUT body the web app sends when saving a college (captured 2026-10).
pub fn college_body(u: &CollegeUpdate, lk: &Lookups) -> Result<Value> {
    let mut degrees: Vec<Value> = u.kept.iter().map(kept_body).collect();
    for (id, d) in &u.degrees {
        degrees.push(degree_body(*id, d, lk)?);
    }
    Ok(college_form(&u.college, degrees))
}

/// Attendance resave adds no degrees, so building the body cannot fail a lookup.
pub(crate) fn college_resave_body(college: &Value) -> Value {
    let degrees = arr(&college["degrees"]).iter().map(kept_body).collect();
    college_form(college, degrees)
}

fn college_form(c: &Value, degrees: Vec<Value>) -> Value {
    // The form sends "yes" only with degrees, and refuses isDegree "yes"
    // without any ("one or more required questions are not answered",
    // observed 2026-10-06 re-saving Pierce, which has none).
    let yes_no = if degrees.is_empty() { "no" } else { "yes" };
    let mut b = json!({
        "id": c["id"], "countryId": "", "college": c["college"], "isPrimary": c["isPrimary"],
        "termTypeId": c["termTypeId"], "stillAttending": c["stillAttending"],
        "startTermId": c["collegeStartTermId"], "startMonthId": c["collegeStartMonthId"],
        "startYearId": c["collegeStartYearId"], "isDegree": yes_no, "collegeCode": c["collegeCode"],
        "collegeStartMonthId": c["collegeStartMonthId"], "collegeStartTermId": c["collegeStartTermId"],
        "collegeStartYearId": c["collegeStartYearId"], "degrees": degrees, "degreeObtained": yes_no,
        "auEnabled": false, "optOut": false,
    });
    if c["stillAttending"].as_bool() != Some(true) {
        for side in ["Term", "Month", "Year"] {
            let v = c[format!("collegeEnd{side}Id")].clone();
            b[format!("end{side}Id")] = v.clone();
            b[format!("collegeEnd{side}Id")] = v;
        }
    }
    b
}

pub async fn apply(c: &Client, lk: &Lookups, u: &CollegeUpdate) -> Result<()> {
    c.put(&format!("{COLLEGES}/{}", int(&u.college["id"])), &college_body(u, lk)?).await.map(drop)
}

#[cfg(test)]
#[path = "tests/education_tests.rs"]
mod tests;
