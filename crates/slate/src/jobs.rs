//! Professional Experience (`/apply/job`): one entry per employer role.
//!
//! The Add Employer popup (`job?id=`, captured 2026-10-06) is a plain form:
//! `employer` (≤256), `from_m`/`from_y`, `to_m`/`to_y` (`to_m` empty means
//! Present), `title` (≤64), `hours` (≤2 digits), `description` (≤1000). Its
//! Save calls `FW.Lazy.Commit(this)` with no extra command, and the form has
//! no enctype, so it posts urlencoded fields.

use admit_domain::Finding;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Job {
    pub employer: String,
    pub title: String,
    /// `YYYY-MM`.
    pub from: String,
    /// `YYYY-MM`; absent while current.
    pub to: Option<String>,
    pub hours: u32,
    #[serde(default)]
    pub description: String,
    /// The profile's title when this one is shortened for the form.
    pub profile_title: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobAdd {
    pub job: Job,
    pub fields: Vec<(String, String)>,
}

fn month_year(s: &str) -> Option<(String, String)> {
    let (y, m) = s.split_once('-')?;
    let m: u8 = m.parse().ok().filter(|m| (1..=12).contains(m))?;
    (y.len() == 4 && y.chars().all(|c| c.is_ascii_digit())).then(|| (format!("{m:02}"), y.to_string()))
}

/// The dates as the list shows them ("08/2017–05/2021", "08/2017–Present"
/// is assumed for a current role).
pub fn dates_label(j: &Job) -> Option<String> {
    let (fm, fy) = month_year(&j.from)?;
    let to = match &j.to {
        Some(t) => {
            let (tm, ty) = month_year(t)?;
            format!("{tm}/{ty}")
        }
        None => "Present".into(),
    };
    Some(format!("{fm}/{fy}\u{2013}{to}"))
}

fn norm(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

/// The roles `want` lists beyond what Professional Experience shows.
/// `listed` is each row's (employer, dates) text. Rows don't show the
/// title, so roles at one employer over the same dates are counted.
pub fn plan(listed: &[(String, String)], want: &[Job]) -> (Vec<JobAdd>, Vec<Finding>) {
    let mut adds = Vec::new();
    let mut findings = Vec::new();
    let mut seen: Vec<(String, String)> = Vec::new();
    for j in want {
        let bad = |why: String| Finding::Violation(format!("job {:?} at {:?}: {why}", j.title, j.employer));
        let Some(label) = dates_label(j) else {
            findings.push(bad("from/to must be YYYY-MM".into()));
            continue;
        };
        if let Some(t) = &j.to
            && t < &j.from
        {
            findings.push(bad("to is before from".into()));
            continue;
        }
        if j.title.chars().count() > 64 {
            findings.push(bad(format!("title is {} chars, the field takes 64", j.title.chars().count())));
            continue;
        }
        if j.description.chars().count() > 1000 {
            findings.push(bad(format!("description is {} chars, the field takes 1000", j.description.chars().count())));
            continue;
        }
        if j.hours > 99 {
            findings.push(bad("hours/week takes two digits".into()));
            continue;
        }
        let key = (norm(&j.employer), norm(&label));
        let on_page = listed.iter().filter(|(e, d)| (norm(e), norm(d)) == key).count();
        let planned = seen.iter().filter(|k| **k == key).count();
        seen.push(key);
        if planned < on_page {
            continue;
        }
        let (fm, fy) = month_year(&j.from).unwrap_or_default();
        let (tm, ty) = j.to.as_deref().and_then(month_year).unwrap_or_default();
        let fields = vec![
            ("employer".to_string(), j.employer.clone()),
            ("from_m".into(), fm),
            ("from_y".into(), fy),
            ("to_m".into(), tm),
            ("to_y".into(), ty),
            ("title".into(), j.title.clone()),
            ("hours".into(), j.hours.to_string()),
            ("description".into(), j.description.clone()),
        ];
        adds.push(JobAdd { job: j.clone(), fields });
    }
    (adds, findings)
}
