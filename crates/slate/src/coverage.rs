//! Completeness, section by section. Every entry in the application's menu
//! gets exactly one line: what it holds and whether anything is left, so
//! "done" is never inferred from the Review page alone (which says nothing
//! about an empty list it doesn't require) or from blank questions alone
//! (which don't see lists). A section admit doesn't understand is reported
//! as UNCHECKED, a violation, never skipped.
//!
//! Also: everything the profile holds that the application could take
//! (paid roles, colleges) must be on the portal or listed under
//! `[excluded]` in the answers file with a reason.

use std::collections::BTreeMap;

use admit_domain::{ExperienceKind, Finding, Profile};

use crate::answer::{AnswersFile, current};
use crate::page::{Issue, Page};

/// What one menu section turned out to be.
pub enum Section {
    Questions(Page),
    /// A list (`aca`, `job`, `tes`, `ref`) with its row count, and how many
    /// rows the answers file declares for it, when it declares any.
    List {
        rows: usize,
        declared: Option<usize>,
    },
    /// `certify`: complete when Review stops listing it.
    Signature,
    /// `review` itself.
    Review,
    /// Anything else: not checked.
    Unknown,
}

pub struct Entry {
    pub title: String,
    pub href: String,
    pub section: Section,
}

pub fn report(entries: &[Entry], issues: &[Issue], excluded: &BTreeMap<String, String>) -> Vec<Finding> {
    let mut out = Vec::new();
    for e in entries {
        let errors: Vec<&str> =
            issues.iter().filter(|i| i.error && i.href == e.href).map(|i| i.message.as_str()).collect();
        let t = &e.title;
        let todo = |what: String| Finding::Pending(format!("section {t}: TODO {what}"));
        let ok = |what: String| Finding::Note(format!("section {t}: ok {what}"));
        let line = match &e.section {
            Section::Unknown => Finding::Violation(format!(
                "section {t} ({}): UNCHECKED - admit doesn't read this kind of page",
                e.href
            )),
            _ if !errors.is_empty() => todo(format!("(Review: {})", errors.join("; "))),
            Section::Review => ok(format!("({} warning(s))", issues.iter().filter(|i| !i.error).count())),
            Section::Signature => ok("(signed)".into()),
            Section::List { rows, declared } => match (rows, declared) {
                (r, Some(d)) if r < d => todo(format!("({r} of {d} declared rows on the portal)")),
                (0, _) => match excluded.get(t.as_str()) {
                    Some(why) => ok(format!("(empty: {why})")),
                    None => todo("(empty - fill it, or record why under [excluded] in the answers file)".into()),
                },
                (r, _) => ok(format!("({r} row(s))")),
            },
            Section::Questions(p) => {
                let writable: Vec<_> = p.questions.iter().filter(|q| q.writable()).collect();
                let answered = writable.iter().filter(|q| current(q).is_some()).count();
                let required_blank: Vec<&str> =
                    writable.iter().filter(|q| q.required && current(q).is_none()).map(|q| q.key()).collect();
                let uploads = p.questions.iter().filter(|q| q.data_type == "plugin:material").count();
                let widgets = p.questions.iter().filter(|q| q.data_type == "plugin:widget").count();
                let mut what = format!("({answered} answered, {} blank", writable.len() - answered);
                if uploads > 0 {
                    what.push_str(&format!(", {uploads} upload slot(s)"));
                }
                if widgets > 0 {
                    // Widget entries (addresses, per-school details) live in
                    // popups; Review reports what they miss.
                    what.push_str(&format!(", {widgets} widget(s) checked via Review"));
                }
                if !required_blank.is_empty() {
                    // Review is the authority on required questions; one it
                    // doesn't flag is conditional on another answer.
                    what.push_str(&format!("; required but not flagged by Review: {}", required_blank.join(", ")));
                }
                what.push(')');
                ok(what)
            }
        };
        out.push(line);
    }
    out
}

fn norm(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

/// Profile items the application could hold but the answers file neither
/// declares nor excludes: paid roles (Professional Experience) and colleges
/// (Academic History).
pub fn profile_gaps(profile: &Profile, file: &AnswersFile) -> Vec<Finding> {
    let mut out = Vec::new();
    let excluded = |k: &str| file.excluded.keys().any(|e| norm(e) == norm(k));
    for x in &profile.experiences.experiences {
        if x.kind != ExperienceKind::Employment || !x.compensated {
            continue;
        }
        let declared = file.jobs.iter().any(|j| {
            norm(&j.employer) == norm(&x.organization)
                && (norm(&j.title) == norm(&x.title)
                    || j.profile_title.as_deref().is_some_and(|p| norm(p) == norm(&x.title)))
        });
        let key = format!("{} - {}", x.organization, x.title);
        if !declared && !excluded(&key) {
            out.push(Finding::Pending(format!(
                "profile role {key:?} is not in the answers file's [[jobs]] (add it, or exclude it with a reason)"
            )));
        }
    }
    for c in &profile.education.colleges {
        let declared = file
            .institutions
            .iter()
            .any(|i| norm(&i.name) == norm(&c.name) || i.profile.as_deref().is_some_and(|p| norm(p) == norm(&c.name)));
        if !declared && !excluded(&c.name) {
            out.push(Finding::Pending(format!(
                "profile college {:?} is not in the answers file's [[institutions]] (add it, or exclude it with a reason)",
                c.name
            )));
        }
    }
    out
}
