//! `personal.toml` against what a portal holds about the applicant. Each
//! portal adapter reads its own forms into a [`Stated`]; this compares, so
//! every portal is held to the same answers (a birth city typed one way on
//! NursingCAS and another on UCLA reads as a careless or false answer).

use std::collections::BTreeMap;

use crate::{Finding, Personal, PostalAddress};

/// What one portal says, keyed by the `Personal` field it answers:
/// `legal_name`, `email`, `birthdate`, `birth_city`, `birth_county`,
/// `birth_state`, `birth_country`, `citizenship`, `sex`, `gender_identity`,
/// `pronouns`, `sexual_orientation`. A field the portal
/// doesn't ask is absent; one it asks and is blank is "".
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Stated {
    pub fields: BTreeMap<&'static str, String>,
    /// `kind` is `mailing` or `permanent`; region and country are codes.
    pub addresses: Vec<PostalAddress>,
}

pub fn compare(p: &Personal, portal: &str, s: &Stated) -> Vec<Finding> {
    let want: [(&str, Option<String>); 12] = [
        ("legal_name", p.legal_name.clone()),
        ("email", p.email.clone()),
        ("birthdate", p.birthdate.clone()),
        ("birth_city", p.birth_city.clone()),
        ("birth_county", p.birth_county.clone()),
        ("birth_state", p.birth_state.clone()),
        ("birth_country", p.birth_country.clone()),
        ("citizenship", p.citizenship.clone()),
        ("sex", p.sex.clone()),
        ("gender_identity", p.gender_identity.clone()),
        ("pronouns", p.pronouns.clone()),
        ("sexual_orientation", p.sexual_orientation.clone()),
    ];
    let mut out = Vec::new();
    for (field, want) in want {
        let (Some(want), Some(got)) = (want, s.fields.get(field)) else { continue };
        diff(&mut out, portal, field, &want, got);
    }
    for a in &p.address {
        let Some(got) = s.addresses.iter().find(|b| b.kind == a.kind) else { continue };
        let what = format!("{} address", a.kind);
        for (part, want, got) in [
            ("street", &a.street, &got.street),
            ("city", &a.city, &got.city),
            ("region", &a.region, &got.region),
            ("country", &a.country, &got.country),
        ] {
            diff(&mut out, portal, &format!("{what} {part}"), want, got);
        }
        // A ZIP without the +4 is the same place.
        let zip5 = |z: &str| z.chars().take(5).collect::<String>();
        if zip5(&a.postal) != zip5(&got.postal) || got.postal.is_empty() {
            diff(&mut out, portal, &format!("{what} postal code"), &a.postal, &got.postal);
        }
    }
    out
}

fn diff(out: &mut Vec<Finding>, portal: &str, field: &str, want: &str, got: &str) {
    if got.trim().is_empty() {
        out.push(Finding::Pending(format!("{portal}: {field} is blank; personal.toml says {want:?}")));
    } else if norm(got) != norm(want) {
        out.push(Finding::Violation(format!("{portal}: {field} is {got:?}; personal.toml says {want:?}")));
    } else if got.trim() != want.trim() && got.trim().eq_ignore_ascii_case(want.trim()) {
        // Same words, different capitals: reviewers see it as typed.
        out.push(Finding::Note(format!("{portal}: {field} is written {got:?}; personal.toml has {want:?}")));
    }
}

/// Lower case, single spaces, no `.`/`,`, no trailing "County".
fn norm(s: &str) -> String {
    let s: String = s.chars().filter(|c| !matches!(c, '.' | ',')).collect::<String>().to_lowercase();
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    s.strip_suffix(" county").map(str::to_string).unwrap_or(s)
}

#[cfg(test)]
#[path = "tests/personal_tests.rs"]
mod tests;
