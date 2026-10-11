//! Supporting Information → Achievements (`/supportingInfo/achievements`).

use admit_domain::{Achievement, AchievementKind};
use serde_json::{Value, json};

use crate::client::{Client, Result, arr, int, opt, s};
use crate::lookup::Lookups;

const PATH: &str = "/supportingInfo/achievements";

fn type_text(k: AchievementKind) -> &'static str {
    match k {
        AchievementKind::Award => "Awards",
        AchievementKind::Honor => "Honors",
    }
}

pub fn from_live(r: &Value, lk: &Lookups) -> Achievement {
    Achievement {
        kind: if s(&r["achievement"]) == "Awards" { AchievementKind::Award } else { AchievementKind::Honor },
        name: s(&r["name"]),
        organization: opt(&r["organization"]),
        date: opt(&r["issuedDate"]),
        state: lk.us_states.text(int(&r["stateId"])),
        country: lk.countries.text(int(&r["countryId"])),
        description: opt(&r["description"]),
        ids: Default::default(),
    }
}

pub async fn live(c: &Client, lk: &Lookups) -> Result<Vec<(i64, Achievement)>> {
    let v = c.get(PATH).await?;
    Ok(arr(&v["achievements"]).iter().map(|r| (int(&r["id"]), from_live(r, lk))).collect())
}

pub fn payload(a: &Achievement, id: Option<i64>, lk: &Lookups) -> Result<Value> {
    let (country, state) = lk.state_id(&a.country, &a.state)?;
    Ok(json!({
        "id": id,
        "achievement": null,
        "achievementTypeId": lk.achievement_types.id(type_text(a.kind))?.to_string(),
        "description": a.description,
        "issuedDateStr": a.date,
        "name": a.name,
        "organization": a.organization,
        "countryId": country.to_string(),
        "stateId": state.to_string(),
    }))
}

pub async fn create(c: &Client, lk: &Lookups, a: &Achievement) -> Result<()> {
    c.post(PATH, &payload(a, None, lk)?).await.map(drop)
}

pub async fn update(c: &Client, lk: &Lookups, id: i64, a: &Achievement) -> Result<()> {
    c.put(&format!("{PATH}/{id}"), &payload(a, Some(id), lk)?).await.map(drop)
}

#[cfg(test)]
#[path = "tests/achievements_tests.rs"]
mod tests;
