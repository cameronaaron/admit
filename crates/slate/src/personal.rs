//! What a Slate application says about the applicant, for comparison with
//! `personal.toml`. Read from the answers file, not the pages: `audit`
//! already proves the file is what the portal holds (an answer not yet
//! applied is reported there), so a difference here is a difference on
//! the portal too, or about to be.

use admit_domain::PostalAddress;
use admit_domain::validate::personal::Stated;

use crate::AnswersFile;
use crate::answer::Answer;

/// Export key → the `Personal` field it answers (UCLA's Graduate Division
/// keys, captured 2026-10-06).
const FIELDS: [(&str, &str); 10] = [
    ("sys:email", "email"),
    ("sys:birthdate", "birthdate"),
    ("sys:field:gd_birth_city", "birth_city"),
    ("sys:field:gd_birth_ca_county", "birth_county"),
    ("sys:field:gd_birth_state", "birth_state"),
    ("sys:field:gd_birth_country", "birth_country"),
    ("sys:citizenship1", "citizenship"),
    ("sys:field:gd_gender_self_identified", "gender_identity"),
    ("sys:field:gd_preferred_pronouns", "pronouns"),
    ("sys:field:gd_sex_orientation", "sexual_orientation"),
];

pub fn stated(file: &AnswersFile) -> Stated {
    let one = |key: &str| match file.answers.get(key) {
        Some(Answer::One(v)) => Some(v.clone()),
        Some(Answer::Many(v)) => Some(v.join(", ")),
        None => None,
    };
    let mut out = Stated::default();
    if let (Some(first), Some(last)) = (one("sys:first"), one("sys:last")) {
        let name = [first, one("sys:middle").unwrap_or_default(), last];
        out.fields.insert("legal_name", name.into_iter().filter(|p| !p.is_empty()).collect::<Vec<_>>().join(" "));
    }
    for (key, field) in FIELDS {
        if let Some(v) = one(key) {
            out.fields.insert(field, v);
        }
    }
    out.addresses = file
        .addresses
        .iter()
        .map(|a| PostalAddress {
            kind: a.kind.clone(),
            street: a.street.clone(),
            city: a.city.clone(),
            region: a.region.clone(),
            country: a.country.clone(),
            postal: a.postal.clone(),
        })
        .collect();
    out
}

#[cfg(test)]
#[path = "tests/personal_tests.rs"]
mod tests;
