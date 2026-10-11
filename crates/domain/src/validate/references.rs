//! Recommenders (`references.toml`): what the Liaison evaluation form checks
//! before it sends a request (yup schema in the applicant bundle, 2026-10).

use time::Date;

use super::{dupes, parse_date, too_long};
use crate::{Finding, Profile};

/// "Personal Message to Your Evaluator", `maxChars: 500`.
pub const MESSAGE_MAX: usize = 500;
/// The email field's `fieldMaxLength`.
pub const EMAIL_MAX: usize = 100;

pub(super) fn check(p: &Profile, today: Date, out: &mut Vec<Finding>) {
    let r = &p.references;
    if !r.recommenders.is_empty() && !r.consent {
        out.push(Finding::Violation(
            "references.toml: consent = true is required: each request agrees that the reference and the schools may contact them by email"
                .into(),
        ));
    }
    for x in &r.recommenders {
        let what = format!("recommender {:?}", x.email);
        if x.first_name.trim().is_empty() || x.last_name.trim().is_empty() {
            out.push(Finding::Violation(format!("{what}: first and last name are required")));
        }
        let (local, domain) = x.email.split_once('@').unwrap_or_default();
        if local.is_empty() || !domain.contains('.') || x.email.contains(char::is_whitespace) {
            out.push(Finding::Violation(format!("{what}: not an email address")));
        }
        too_long(&what, "email", Some(&x.email), EMAIL_MAX, out);
        if x.message.trim().is_empty() {
            out.push(Finding::Violation(format!("{what}: the personal message is required")));
        }
        too_long(&what, "message", Some(&x.message), MESSAGE_MAX, out);
        match parse_date(&x.due) {
            None => out.push(Finding::Violation(format!("{what}: due {:?} is not YYYY-MM-DD", x.due))),
            Some(d) if d < today => out.push(Finding::Violation(format!("{what}: due {d} is past"))),
            Some(_) => {}
        }
        if x.programs.is_empty() {
            out.push(Finding::Violation(format!("{what}: no programs")));
        }
        dupes(x.programs.iter().cloned(), &format!("{what}: program"), out);
    }
    dupes(r.recommenders.iter().map(|x| x.email.to_lowercase()), "recommender", out);
}
