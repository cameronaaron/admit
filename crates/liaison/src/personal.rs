//! What NursingCAS holds about the applicant, read for comparison with
//! `personal.toml` (read-only; these sections are not written).
//!
//! Biographic and Contact Information are question blocks
//! (`GET /applicationFormSubSectionBlocks/subSection/{id}`, ids from
//! `/navigation` `personalInfo.sections[].name`). Each answer is
//! `value: [..]`, an option id for a dropdown. A GET does not carry the
//! chosen option of a lookup-backed dropdown (country, state, county): its
//! one `answerOptions` entry names the table (`LK_Languages`, observed
//! 2026-10-06; only the PUT response echoes the option, with a null
//! `text`), so those resolve through `/lookup/*`. The account email is
//! `/authenticatedUser` `email`.

use admit_domain::PostalAddress;
use admit_domain::validate::personal::Stated;
use serde_json::Value;

use crate::client::{Client, Result, arr, int, s};
use crate::lookup::{self, Lookups, Table};

/// `shortFieldName` → the question.
fn questions(section: &Value) -> Vec<(&str, &Value)> {
    arr(&section["questionBlocks"])
        .iter()
        .flat_map(|b| arr(&b["questions"]))
        .filter_map(|q| q["shortFieldName"].as_str().map(|n| (n, q)))
        .collect()
}

fn value(q: &Value) -> String {
    arr(&q["value"]).first().map(s).unwrap_or_default()
}

/// The chosen option of a dropdown or radio, as text: its own `text`, else
/// `table`'s name for the id.
fn chosen(q: &Value, table: Option<&Table>) -> String {
    let v = value(q);
    let Ok(id) = v.parse::<i64>() else { return v };
    let opt = arr(&q["answerOptions"]).iter().find(|o| int(&o["id"]) == id);
    match (opt.map(|o| s(&o["text"])).filter(|t| !t.is_empty()), table) {
        (Some(t), _) => t,
        (None, Some(t)) => t.text(id),
        (None, None) => v,
    }
}

/// The chosen option's `code` (`US`, `US-CA`): its own, else `table`'s.
fn code(q: &Value, table: &Table) -> String {
    let Ok(id) = value(q).parse::<i64>() else { return String::new() };
    let own = arr(&q["answerOptions"]).iter().find(|o| int(&o["id"]) == id).map(|o| s(&o["code"]));
    own.filter(|c| !c.is_empty()).unwrap_or_else(|| table.code(id))
}

/// Lookup tables for the dropdowns whose options carry no text.
pub struct Names<'a> {
    pub countries: &'a Table,
    pub us_states: &'a Table,
    /// Counties of the birth state.
    pub counties: &'a Table,
}

pub fn stated(bio: &Value, contact: &Value, email: &str, n: &Names) -> Stated {
    let mut out = Stated::default();
    let bio = questions(bio);
    let get = |name: &str| bio.iter().find(|(k, _)| *k == name).map(|(_, q)| *q);
    if let (Some(f), Some(l)) = (get("first_name"), get("last_name")) {
        let parts = [value(f), get("middle_name").map(value).unwrap_or_default(), value(l)];
        let name = parts.iter().filter(|p| !p.is_empty()).cloned().collect::<Vec<_>>().join(" ");
        out.fields.insert("legal_name", name);
    }
    if !email.is_empty() {
        out.fields.insert("email", email.to_string());
    }
    let fields: [(&str, &'static str, Option<&Table>); 7] = [
        ("birth_date", "birthdate", None),
        ("birth_city", "birth_city", None),
        ("birth_county", "birth_county", Some(n.counties)),
        ("birth_state", "birth_state", Some(n.us_states)),
        ("birth_country", "birth_country", Some(n.countries)),
        ("gender", "sex", None),
        ("pronounsPreferred", "pronouns", None),
    ];
    for (portal, ours, table) in fields {
        if let Some(q) = get(portal) {
            out.fields.insert(ours, chosen(q, table));
        }
    }
    let contact = questions(contact);
    let get = |name: &str| contact.iter().find(|(k, _)| *k == name).map(|(_, q)| *q);
    // The current address is where mail goes; a permanent one is asked only
    // when it differs (`p*` fields, blank otherwise).
    for (kind, prefix) in [("mailing", ""), ("permanent", "p")] {
        let f = |name: &str| get(&format!("{prefix}{name}"));
        let street = f("street_address_1").map(value).unwrap_or_default();
        if street.is_empty() {
            continue;
        }
        let line2 = f("street_address_2").map(value).unwrap_or_default();
        out.addresses.push(PostalAddress {
            kind: kind.into(),
            street: if line2.is_empty() { street } else { format!("{street} {line2}") },
            city: f("city").map(value).unwrap_or_default(),
            region: f("state_id")
                .map(|q| code(q, n.us_states))
                .unwrap_or_default()
                .trim_start_matches("US-")
                .to_string(),
            country: f("country_id").map(|q| code(q, n.countries)).unwrap_or_default(),
            postal: f("zip").map(value).unwrap_or_default(),
        });
    }
    out
}

/// The `/navigation` id of a personal-information section (`biographicInfo`).
pub fn section_id(nav: &Value, name: &str) -> Option<i64> {
    arr(&nav["personalInfo"]["sections"]).iter().find(|x| s(&x["name"]) == name).map(|x| int(&x["id"]))
}

pub async fn read(c: &Client, lk: &Lookups) -> Result<Stated> {
    let nav = c.get("/navigation").await?;
    let section = |name: &str| {
        section_id(&nav, name)
            .ok_or_else(|| crate::ApiError::Shape(format!("/navigation lists no personalInfo section {name:?}")))
    };
    let path = |id: i64| format!("/applicationFormSubSectionBlocks/subSection/{id}");
    let bio = c.get(&path(section("biographicInfo")?)).await?;
    let contact = c.get(&path(section("contactInfo")?)).await?;
    let email = s(&c.get("/authenticatedUser").await?["email"]);
    let state = questions(&bio).into_iter().find(|(k, _)| *k == "birth_state").map(|(_, q)| value(q));
    let counties = match state.filter(|v| !v.is_empty()) {
        Some(id) => lookup::fetch(c, "county", &format!("/lookup/counties?stateId={id}")).await?,
        None => Table::default(),
    };
    Ok(stated(
        &bio,
        &contact,
        &email,
        &Names { countries: &lk.countries, us_states: &lk.us_states, counties: &counties },
    ))
}

#[cfg(test)]
#[path = "tests/personal_tests.rs"]
mod tests;
