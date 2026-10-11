//! Supporting Information → Licensure and Certifications.
//!
//! Saved as multipart with a JSON `model` part; the same POST edits when
//! `id` is an existing record (that's how the web form does it).

use admit_domain::{License, LicenseKind};
use serde_json::{Value, json};

use crate::client::{Client, Result, arr, int, opt, s};
use crate::lookup::Lookups;

/// NursingCAS uses the `license2` block; other CASs may use another.
const LIST: &str = "/supportingInfo/licenses/blockName/license2";
const SAVE: &str = "/supportingInfo/licenses";

fn kind_text(k: LicenseKind) -> &'static str {
    match k {
        LicenseKind::Certification => "Certification",
        LicenseKind::Licensure => "Licensure",
    }
}

pub fn from_live(r: &Value) -> License {
    License {
        kind: if s(&r["typeName"]) == "Licensure" { LicenseKind::Licensure } else { LicenseKind::Certification },
        title: s(&r["title"]),
        organization: s(&r["organization"]),
        description: opt(&r["description"]),
        valid_until: opt(&r["validUntilDateString"]),
        ids: Default::default(),
    }
}

pub async fn live(c: &Client) -> Result<Vec<(i64, License)>> {
    let v = c.get(LIST).await?;
    Ok(arr(&v["licenses"]).iter().map(|r| (int(&r["id"]), from_live(r))).collect())
}

pub fn model(l: &License, id: i64, lk: &Lookups) -> Result<Value> {
    Ok(json!({
        "id": id,
        "title": l.title,
        "typeId": lk.license_types.id(kind_text(l.kind))?.to_string(),
        "licenseTypeId": null,
        "typeName": kind_text(l.kind),
        "organization": l.organization,
        "description": l.description,
        "nursingValidUntilDate": l.valid_until,
    }))
}

pub async fn save(c: &Client, lk: &Lookups, id: Option<i64>, l: &License) -> Result<()> {
    c.post_model(SAVE, &model(l, id.unwrap_or(0), lk)?).await.map(drop)
}

#[cfg(test)]
#[path = "tests/licenses_tests.rs"]
mod tests;
