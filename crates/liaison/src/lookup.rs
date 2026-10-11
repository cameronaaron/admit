//! Liaison's `/lookup/*` tables: display text ↔ numeric id.

use std::collections::HashMap;

use crate::client::{ApiError, Client, Result, arr, int, s};

#[derive(Debug, Default, Clone)]
pub struct Table {
    name: String,
    by_text: HashMap<String, i64>,
    by_id: HashMap<i64, String>,
    /// The row's `code` (`US`, `US-CA`), where the table has one.
    codes: HashMap<i64, String>,
}

impl Table {
    pub fn from_rows(name: &str, rows: &serde_json::Value) -> Table {
        let mut t = Table { name: name.into(), ..Default::default() };
        for r in arr(rows) {
            let (id, text) = (int(&r["id"]), s(&r["text"]));
            t.by_text.insert(text.to_lowercase(), id);
            t.by_id.insert(id, text);
            t.codes.insert(id, s(&r["code"]));
        }
        t
    }

    pub fn id(&self, text: &str) -> Result<i64> {
        self.by_text
            .get(&text.to_lowercase())
            .copied()
            .ok_or_else(|| ApiError::Shape(format!("{text:?} is not a {} option on this portal", self.name)))
    }

    pub fn text(&self, id: i64) -> String {
        self.by_id.get(&id).cloned().unwrap_or_default()
    }

    pub fn code(&self, id: i64) -> String {
        self.codes.get(&id).cloned().unwrap_or_default()
    }
}

pub async fn fetch(c: &Client, name: &str, path: &str) -> Result<Table> {
    Ok(Table::from_rows(name, &c.get(path).await?))
}

/// The tables the profile sections need, fetched once per run.
pub struct Lookups {
    pub countries: Table,
    pub us_states: Table,
    pub achievement_types: Table,
    pub experience_types: Table,
    pub employment_status: Table,
    pub license_types: Table,
    pub degree_types: Table,
    pub majors: Table,
    pub months: Table,
    pub years: Table,
    pub terms: Table,
    pub term_types: Table,
    pub academic_status: Table,
    pub subjects: Table,
}

pub const US: i64 = 222;

impl Lookups {
    pub async fn fetch(c: &Client) -> Result<Lookups> {
        Ok(Lookups {
            countries: fetch(c, "country", "/lookup/countries").await?,
            us_states: fetch(c, "US state", &format!("/lookup/states?countryId={US}")).await?,
            achievement_types: fetch(c, "achievement type", "/lookup/achievementTypes").await?,
            experience_types: fetch(c, "experience type", "/lookup/experienceTypes").await?,
            employment_status: fetch(c, "employment status", "/lookup/employmentStatusTypes").await?,
            license_types: fetch(c, "license type", "/lookup/license").await?,
            degree_types: fetch(c, "degree type", "/lookup/degreeType").await?,
            majors: fetch(c, "major", "/lookup/collegeMinor").await?,
            months: fetch(c, "month", "/lookup/month").await?,
            years: fetch(c, "year", "/lookup/year").await?,
            terms: fetch(c, "term", "/lookup/terms").await?,
            term_types: fetch(c, "term type", "/lookup/termType").await?,
            academic_status: fetch(c, "academic status", "/lookup/academicStatus").await?,
            subjects: fetch(c, "subject", "/lookup/subjects").await?,
        })
    }

    /// State lookups are per country; only US states are loaded.
    pub fn state_id(&self, country: &str, state: &str) -> Result<(i64, i64)> {
        let country_id = self.countries.id(country)?;
        if country_id != US {
            return Err(ApiError::Shape(format!("only United States addresses are supported so far (got {country})")));
        }
        Ok((country_id, self.us_states.id(state)?))
    }
}
