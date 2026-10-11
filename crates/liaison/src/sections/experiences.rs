//! Supporting Information → Experiences, stored as question blocks
//! (`/questionBlock/supportingInfo/experiences[/{id}]`).
//!
//! Edits re-send every live block and change only the fields the profile
//! owns, so blocks this tool doesn't model (domains, competencies, the
//! "most important" flag, …) survive untouched.

use std::collections::BTreeMap;

use admit_domain::{Experience, ExperienceKind, Supervisor};
use serde_json::{Map, Value, json};

use crate::client::{Client, Result, arr, int, opt, s};
use crate::lookup::Lookups;

pub(crate) const LIST: &str = "/supportingInfo/experiences";
pub(crate) const BLOCKS: &str = "/questionBlock/supportingInfo/experiences";

fn kind_text(k: ExperienceKind) -> &'static str {
    match k {
        ExperienceKind::Employment => "Employment",
        ExperienceKind::PatientCare => "Patient/Healthcare Experience",
        ExperienceKind::Community => "Community Enrichment",
    }
}

fn kind_from(text: &str) -> ExperienceKind {
    match text {
        "Patient/Healthcare Experience" => ExperienceKind::PatientCare,
        "Community Enrichment" => ExperienceKind::Community,
        _ => ExperienceKind::Employment,
    }
}

type Blocks = BTreeMap<String, Value>;

fn blocks_of(v: &Value) -> Blocks {
    arr(&v["questionBlocks"])
        .iter()
        .filter(|b| !b["data"].is_null())
        .map(|b| (s(&b["id"]), b["data"].clone()))
        .collect()
}

fn u(v: &Value) -> u32 {
    u32::try_from(int(v)).unwrap_or(0)
}

pub fn from_blocks(b: &Blocks, lk: &Lookups) -> Experience {
    let null = Value::Null;
    let g = |block: &str, field: &str| b.get(block).map(|d| &d[field]).unwrap_or(&null);
    let current = g("employmentDates", "currentlyEmployed").as_bool().unwrap_or(false);
    Experience {
        kind: kind_from(&lk.experience_types.text(int(g("experienceType", "experienceTypeId")))),
        organization: s(g("organization", "name")),
        title: s(g("employmentDetails", "jobTitle")),
        state: lk.us_states.text(int(g("organization", "stateId"))),
        country: lk.countries.text(int(g("organization", "countryId"))),
        start: s(g("employmentDates", "startDate")),
        end: if current { None } else { opt(g("employmentDates", "endDate")) },
        current,
        status: lk.employment_status.text(int(g("employmentDates", "statusId"))),
        hours_per_week: u(g("employmentDetails", "averageWeeklyHours")),
        weeks: u(g("employmentDetails", "numberOfWeeks")),
        total_hours: u(g("employmentDetails", "totalHours")),
        compensated: g("employmentDetails", "compensated").as_bool().unwrap_or(false),
        volunteer: g("employmentDetails", "volunteer").as_bool().unwrap_or(false),
        description: s(g("employmentDetails", "jobDescription")),
        supervisor: Supervisor {
            first_name: s(g("supervisor", "firstName")),
            last_name: s(g("supervisor", "lastName")),
            title: s(g("supervisor", "jobTitle")),
            email: s(g("supervisor", "contactEmail")),
            phone: s(g("supervisor", "contactPhone")),
        },
        ids: Default::default(),
    }
}

/// Live experiences with their full block sets (needed to edit in place).
pub async fn live(c: &Client, lk: &Lookups) -> Result<Vec<(i64, Experience, Blocks)>> {
    let list = c.get(LIST).await?;
    let mut out = Vec::new();
    for r in arr(&list["experiences"]) {
        let id = int(&r["id"]);
        let b = blocks_of(&c.get(&format!("{BLOCKS}/{id}")).await?);
        out.push((id, from_blocks(&b, lk), b));
    }
    Ok(out)
}

fn set(b: &mut Blocks, block: &str, fields: Value) {
    let e = b.entry(block.to_string()).or_insert_with(|| Value::Object(Map::new()));
    if let (Value::Object(dst), Value::Object(src)) = (e, fields) {
        dst.extend(src);
    }
}

/// Writes the profile-owned fields of `x` into `b`.
pub fn apply_to_blocks(x: &Experience, b: &mut Blocks, lk: &Lookups) -> Result<()> {
    let (country, state) = lk.state_id(&x.country, &x.state)?;
    set(b, "experienceType", json!({ "experienceTypeId": lk.experience_types.id(kind_text(x.kind))? }));
    set(
        b,
        "organization",
        json!({ "name": x.organization, "stateId": state, "countryId": country,
                "stateValue": x.state, "countryValue": x.country }),
    );
    set(
        b,
        "supervisor",
        json!({ "firstName": x.supervisor.first_name, "lastName": x.supervisor.last_name,
                "jobTitle": x.supervisor.title, "contactEmail": x.supervisor.email,
                "contactPhone": x.supervisor.phone }),
    );
    set(
        b,
        "employmentDates",
        json!({ "statusId": lk.employment_status.id(&x.status)?, "startDate": x.start,
                "endDate": x.end, "currentlyEmployed": x.current }),
    );
    set(
        b,
        "employmentDetails",
        json!({ "compensated": x.compensated, "volunteer": x.volunteer,
                "averageWeeklyHours": x.hours_per_week, "numberOfWeeks": x.weeks,
                "totalHours": x.total_hours, "jobTitle": x.title, "jobDescription": x.description,
                "receivedAutomaticCredit": false, "releaseAuthorization": true }),
    );
    Ok(())
}

fn body(b: &Blocks) -> Value {
    json!({ "questionBlocks": b.iter().map(|(id, data)| json!({ "id": id, "data": data })).collect::<Vec<_>>() })
}

pub async fn create(c: &Client, lk: &Lookups, x: &Experience) -> Result<()> {
    let mut b = Blocks::new();
    apply_to_blocks(x, &mut b, lk)?;
    set(&mut b, "organization", json!({ "zip": "", "address": "", "address2": "", "city": "" }));
    c.post(BLOCKS, &body(&b)).await.map(drop)
}

pub async fn update(c: &Client, lk: &Lookups, id: i64, x: &Experience, live_blocks: &Blocks) -> Result<()> {
    let mut b = live_blocks.clone();
    apply_to_blocks(x, &mut b, lk)?;
    c.put(&format!("{BLOCKS}/{id}"), &body(&b)).await.map(drop)
}

#[cfg(test)]
#[path = "tests/experiences_tests.rs"]
mod tests;
