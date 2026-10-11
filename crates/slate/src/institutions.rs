//! Academic History (`/apply/aca`): one entry per institution attended,
//! each with its transcript.
//!
//! The Add Institution popup (`aca?id=`) picks a school from UCLA's list by
//! region → city → school (`Client::fetch_schools`); choosing one fills the
//! hidden `key`, `name`, `city_keyed`, `region_keyed` (and `country_keyed`
//! from the region). A school not on the list is entered as "Other": `name`
//! plus an address (`country`, `city`, `region`). The form then takes the
//! dates attended, the level (`type` U/G), one degree with its conferred
//! date, and the transcript (`file1`), and is committed with `cmd=save`
//! (`aca.js` and the form, fetched 2026-10-06).

use std::collections::BTreeMap;

use admit_domain::Finding;
use serde::{Deserialize, Serialize};

use crate::page::Choice;

/// One institution as the answers file declares it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Institution {
    /// The name on UCLA's list (`admit schools`), or the school's own name
    /// when it isn't listed.
    pub name: String,
    /// The college's name in education.toml when UCLA's list names it
    /// differently ("UNIVERSITY OF MASSACHUSETTS - AMHERST").
    pub profile: Option<String>,
    /// UCLA's school key; absent for a school entered as "Other".
    pub key: Option<String>,
    pub city: String,
    /// Region code (`CA`).
    pub region: String,
    /// Country code (`US`).
    pub country: String,
    /// `YYYY-MM`.
    pub from: String,
    pub to: String,
    /// `U` (undergraduate) or `G` (graduate).
    pub level: String,
    /// A degree as the form lists it ("Bachelor of Arts"); absent: none.
    pub degree: Option<String>,
    /// `YYYY-MM`, required with a degree.
    pub conferred: Option<String>,
    /// The transcript to upload, relative to the profile directory.
    pub transcript: String,
    /// Additional Academic Info, per school (its own popup form):
    /// `sys:school:major`.
    pub major: Option<String>,
    /// `sys:school:field:gd_school_gpa`, as the transcript states it.
    pub gpa: Option<String>,
    /// `sys:school:field:gd_gpa_school_scale` ("4.0").
    pub gpa_scale: Option<String>,
}

impl Institution {
    /// The school-detail answers for its Additional Academic Info form.
    pub fn details(&self) -> std::collections::BTreeMap<String, crate::answer::Answer> {
        use crate::answer::Answer;
        // Blank dates are omitted. `from`/`to` are strings, so a school with
        // nothing filled in would otherwise still look like it had details,
        // and `Portal::plan_school_details` could never see an empty map.
        let dated = |s: &str| (!s.is_empty()).then(|| s.to_string());
        let from = dated(&self.from);
        let to = dated(&self.to);
        [
            ("sys:school:major", self.major.as_ref()),
            ("sys:school:field:gd_school_gpa", self.gpa.as_ref()),
            ("sys:school:field:gd_gpa_school_scale", self.gpa_scale.as_ref()),
            ("sys:school:from", from.as_ref()),
            ("sys:school:to", to.as_ref()),
        ]
        .into_iter()
        .filter_map(|(k, v)| Some((k.to_string(), Answer::One(v?.clone()))))
        .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstitutionAdd {
    pub institution: Institution,
    pub fields: Vec<(String, String)>,
}

fn month_year(s: &str) -> Option<(String, String)> {
    let (y, m) = s.split_once('-')?;
    let ok = y.len() == 4 && y.chars().all(|c| c.is_ascii_digit()) && matches!(m.parse::<u8>(), Ok(1..=12));
    ok.then(|| (format!("{:02}", m.parse::<u8>().unwrap_or_default()), y.to_string()))
}

fn norm(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

/// The institutions `want` lists that Academic History doesn't, with the
/// fields their Add Institution post needs. `listed` is what the page shows;
/// `degrees` the form's degree choices per level.
pub fn plan(
    listed: &[String],
    degrees: &BTreeMap<String, Vec<Choice>>,
    want: &[Institution],
) -> (Vec<InstitutionAdd>, Vec<Finding>) {
    let mut adds = Vec::new();
    let mut findings = Vec::new();
    for i in want {
        // Dates are checked for every school, listed or not: a listed one's
        // go to its Additional Academic Info form (`details`), where a bad
        // or blank date would otherwise be written or silently skipped.
        let bad = |why: String| Finding::Violation(format!("institution {:?}: {why}", i.name));
        let (Some((fm, fy)), Some((tm, ty))) = (month_year(&i.from), month_year(&i.to)) else {
            findings.push(bad("from/to must be YYYY-MM".into()));
            continue;
        };
        if (fy.as_str(), fm.as_str()) > (ty.as_str(), tm.as_str()) {
            findings.push(bad("from is after to".into()));
            continue;
        }
        if listed.iter().any(|l| norm(l).contains(&norm(&i.name))) {
            continue;
        }
        let Some(choices) = degrees.get(&i.level) else {
            findings.push(bad(format!("level {:?} is not U or G", i.level)));
            continue;
        };
        let degree = match &i.degree {
            None => None,
            Some(d) => match choices.iter().find(|c| norm(&c.text) == norm(d)) {
                Some(c) => Some(c.value.clone()),
                None => {
                    let names: Vec<&str> = choices.iter().map(|c| c.text.as_str()).collect();
                    findings.push(bad(format!("degree {d:?} is not offered ({})", names.join(" | "))));
                    continue;
                }
            },
        };
        let conferred = match (&degree, i.conferred.as_deref().map(month_year)) {
            (Some(_), Some(Some(c))) => Some(c),
            (Some(_), _) => {
                findings.push(bad("a degree needs conferred = \"YYYY-MM\"".into()));
                continue;
            }
            (None, _) => None,
        };
        let mut fields = vec![
            ("key".to_string(), i.key.clone().unwrap_or_default()),
            ("name".into(), i.name.clone()),
            ("city_keyed".into(), if i.key.is_some() { i.city.clone() } else { String::new() }),
            ("region_keyed".into(), if i.key.is_some() { i.region.clone() } else { String::new() }),
            ("country_keyed".into(), i.country.clone()),
        ];
        if i.key.is_none() {
            // "Other": the address block the form enables for it.
            fields.extend([
                ("country".to_string(), i.country.clone()),
                ("city".into(), i.city.clone()),
                ("region".into(), i.region.clone()),
                ("address_lat".into(), String::new()),
                ("address_lng".into(), String::new()),
                ("address_quality".into(), String::new()),
                ("address_dirty".into(), "1".into()),
                ("address_override".into(), "0".into()),
            ]);
        }
        fields.extend([
            ("from_m".to_string(), fm),
            ("from_y".into(), fy),
            ("to_m".into(), tm),
            ("to_y".into(), ty),
            ("type".into(), i.level.clone()),
            ("degree".into(), degree.unwrap_or_default()),
        ]);
        if let Some((cm, cy)) = conferred {
            fields.extend([("conferred_m".to_string(), cm), ("conferred_y".into(), cy)]);
        }
        fields.push(("cmd".into(), "save".into()));
        adds.push(InstitutionAdd { institution: i.clone(), fields });
    }
    (adds, findings)
}
