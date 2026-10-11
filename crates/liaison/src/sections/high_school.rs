//! Academic History → High School Attended.
//!
//! Two records: the attendance row (`/academicHistory/highSchools`, edited
//! with `PUT /{id}`) and the diploma question block
//! (`POST /academicHistory/highSchoolInfo?subSectionName=highSchoolAttended`,
//! the full block document with `graduationStatus` changed — captured working).
//! The attendance PUT re-sends the live row with the date fields changed; its
//! shape comes from the JS bundle, and `apply`'s re-plan verifies it.

use admit_domain::{Finding, HighSchool};
use serde_json::{Value, json};

use crate::client::{Client, Result, arr, int, s};
use crate::lookup::Lookups;

const ROWS: &str = "/academicHistory/highSchools";
const INFO: &str = "/academicHistory/highSchoolInfo";
pub(crate) const SECTION: &str = "highSchoolAttended";

#[derive(Debug, Clone)]
pub struct Live {
    pub rows: Vec<Value>,
    pub info_section: i64,
    pub info: Value,
}

pub(crate) fn find_section(v: &Value, name: &str) -> Option<i64> {
    match v {
        Value::Object(m) => {
            if m.get("name").and_then(Value::as_str) == Some(name) {
                return Some(int(&v["id"]));
            }
            m.values().find_map(|x| find_section(x, name))
        }
        Value::Array(a) => a.iter().find_map(|x| find_section(x, name)),
        _ => None,
    }
}

pub async fn live(c: &Client) -> Result<Live> {
    let rows = arr(&c.get(ROWS).await?["data"]).to_vec();
    let info_section = find_section(&c.get("/navigation").await?, SECTION).unwrap_or(0);
    let info = if info_section != 0 { c.get(&format!("{INFO}/{info_section}")).await? } else { Value::Null };
    Ok(Live { rows, info_section, info })
}

fn ym(year: &Value, month_text: &Value, lk: &Lookups) -> String {
    let m = lk.months.id(&s(month_text)).ok();
    // Month ids aren't 1..12 (May is 8); map through the month's position.
    let pos = m.and_then(|id| MONTH_IDS.iter().position(|x| *x == id)).map(|p| p + 1).unwrap_or(0);
    format!("{}-{pos:02}", s(year))
}

/// Liaison month ids in calendar order (observed from `/lookup/month`).
const MONTH_IDS: [i64; 12] = [1, 2, 3, 4, 8, 9, 10, 11, 12, 13, 14, 15];

fn month_id(ym: &str) -> Option<i64> {
    let m: usize = ym.get(5..7)?.parse().ok()?;
    MONTH_IDS.get(m.checked_sub(1)?).copied()
}

fn diploma_block(info: &Value) -> Value {
    arr(&info["questionBlocks"])
        .iter()
        .find(|b| s(&b["id"]) == "graduationStatus")
        .map(|b| b["data"].clone())
        .unwrap_or_default()
}

pub fn pull(l: &Live, lk: &Lookups) -> Vec<HighSchool> {
    let d = diploma_block(&l.info);
    l.rows
        .iter()
        .map(|r| HighSchool {
            name: s(&r["highSchoolName"]),
            from: ym(&r["fromYear"], &r["fromMonth"], lk),
            to: ym(&r["toYear"], &r["toMonth"], lk),
            diploma: s(&d["diplomaOrEquivalency"]),
            diploma_date: s(&d["diplomaOrEquivalencyDate"]),
            term_type: Some(s(&r["termType"])).filter(|t| !t.is_empty()),
        })
        .collect()
}

#[derive(Debug, Clone)]
pub enum Change {
    Attendance { row: Value, want: HighSchool, was: String },
    Diploma { info_section: i64, info: Value, want: HighSchool, was: String },
}

impl Change {
    pub fn describe(&self) -> String {
        match self {
            Change::Attendance { want, was, .. } => {
                format!("~ {} attended {} – {} (portal: {was})", want.name, want.from, want.to)
            }
            Change::Diploma { want, was, .. } => {
                format!("~ {} {} {} (portal: {was})", want.name, want.diploma, want.diploma_date)
            }
        }
    }
}

pub fn plan(want: &[HighSchool], l: &Live, lk: &Lookups) -> (Vec<Change>, Vec<Finding>) {
    let (mut out, mut findings) = (Vec::new(), Vec::new());
    let have = pull(l, lk);
    for w in want {
        let Some(i) = have.iter().position(|h| h.name.eq_ignore_ascii_case(&w.name)) else {
            findings.push(Finding::Pending(format!(
                "high school {:?} is not on the application: add it in the portal",
                w.name
            )));
            continue;
        };
        let h = &have[i];
        if (h.from.as_str(), h.to.as_str()) != (w.from.as_str(), w.to.as_str()) {
            out.push(Change::Attendance {
                row: l.rows[i].clone(),
                want: w.clone(),
                was: format!("{} – {}", h.from, h.to),
            });
        }
        if (h.diploma.as_str(), h.diploma_date.as_str()) != (w.diploma.as_str(), w.diploma_date.as_str()) {
            out.push(Change::Diploma {
                info_section: l.info_section,
                info: l.info.clone(),
                want: w.clone(),
                was: format!("{} {}", h.diploma, h.diploma_date),
            });
        }
    }
    (out, findings)
}

/// The attendance form's PUT body (applicant bundle, 2026-10-06): the live row
/// with the dates replaced, plus what the form always sends and the stored
/// row may lack - `termTypeId`, and `graduated` with the graduation month and
/// year ids. Re-sending the bare row was rejected 422 "one or more required
/// questions are not answered" (2026-10-06, the row had no term type).
pub fn attendance_body(row: &Value, want: &HighSchool, lk: &Lookups) -> Result<Value> {
    let mut r = row.clone();
    for (side, v) in [("from", &want.from), ("to", &want.to)] {
        let (y, m) = (v.get(..4).unwrap_or_default(), month_id(v).unwrap_or(0));
        r[format!("{side}YearId")] = json!(lk.years.id(y)?);
        r[format!("{side}Year")] = json!(y);
        r[format!("{side}MonthId")] = json!(m);
        r[format!("{side}Month")] = json!(lk.months.text(m));
    }
    if int(&r["termTypeId"]) == 0 {
        let t = want.term_type.as_deref().ok_or_else(|| {
            crate::client::ApiError::Shape(format!(
                "{}: the portal has no term type for this school - set term_type in high_school.toml",
                want.name
            ))
        })?;
        r["termTypeId"] = json!(lk.term_types.id(t)?);
    }
    let graduated = !want.diploma_date.is_empty();
    r["graduated"] = json!(graduated);
    if graduated {
        r["monthGraduatedId"] = json!(month_id(&want.diploma_date));
        r["yearGraduatedId"] = json!(lk.years.id(want.diploma_date.get(..4).unwrap_or_default())?);
    }
    Ok(r)
}

pub async fn apply(c: &Client, lk: &Lookups, ch: &Change) -> Result<()> {
    match ch {
        Change::Attendance { row, want, .. } => {
            c.put(&format!("{ROWS}/{}", int(&row["id"])), &attendance_body(row, want, lk)?).await.map(drop)
        }
        Change::Diploma { info_section: _, info, want, .. } => {
            let mut doc = info.clone();
            // `get_mut`, not `doc[...]`: indexing a non-object panics.
            if let Some(blocks) = doc.get_mut("questionBlocks").and_then(Value::as_array_mut) {
                for b in blocks.iter_mut().filter(|b| s(&b["id"]) == "graduationStatus") {
                    b["data"] = json!({ "diplomaOrEquivalency": want.diploma,
                                        "diplomaOrEquivalencyDate": want.diploma_date,
                                        "highSchoolName": want.name });
                }
            }
            c.post(&format!("{INFO}?subSectionName={SECTION}"), &doc).await.map(drop)
        }
    }
}

#[cfg(test)]
#[path = "tests/high_school_tests.rs"]
mod tests;
