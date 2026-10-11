//! Typos a dictionary can't see in the free-text fields reviewers read:
//! doubled punctuation ("security.. Credential", found 2026-10-06 in a
//! license), repeated words, and a space before punctuation.

use crate::{Finding, Profile};

/// The first slip in `text`, as (what, excerpt).
pub fn slip(text: &str) -> Option<(&'static str, String)> {
    let b = text.as_bytes();
    let excerpt = |i: usize| {
        let lo = text.floor_char_boundary(i.saturating_sub(25));
        let hi = text.ceil_char_boundary((i + 25).min(text.len()));
        format!("...{}...", &text[lo..hi])
    };
    for i in 1..b.len() {
        let (p, c) = (b[i - 1], b[i]);
        // ".." but not an ellipsis "...".
        if p == b'.' && c == b'.' && b.get(i + 1) != Some(&b'.') && (i < 2 || b[i - 2] != b'.') {
            return Some(("doubled period", excerpt(i)));
        }
        if p == b',' && c == b',' {
            return Some(("doubled comma", excerpt(i)));
        }
        if p == b' ' && matches!(c, b',' | b';' | b':') {
            return Some(("space before punctuation", excerpt(i)));
        }
    }
    let words: Vec<&str> = text.split_whitespace().collect();
    for w in words.windows(2) {
        let (a, b) = (w[0], w[1]);
        if a.len() > 1 && a.chars().all(char::is_alphabetic) && a.eq_ignore_ascii_case(b) {
            return Some(("repeated word", format!("\"{a} {b}\"")));
        }
    }
    None
}

pub(super) fn check(p: &Profile, out: &mut Vec<Finding>) {
    let mut fields: Vec<(String, &str)> = Vec::new();
    for a in &p.achievements.achievements {
        fields.push((format!("achievement {:?} name", a.name), &a.name));
        if let Some(d) = &a.description {
            fields.push((format!("achievement {:?} description", a.name), d));
        }
    }
    for l in &p.licenses.licenses {
        fields.push((format!("license {:?} title", l.title), &l.title));
        if let Some(d) = &l.description {
            fields.push((format!("license {:?} description", l.title), d));
        }
    }
    for x in &p.experiences.experiences {
        fields.push((format!("experience {:?} title", x.key()), &x.title));
        fields.push((format!("experience {:?} description", x.key()), &x.description));
    }
    for (what, text) in fields {
        if let Some((kind, at)) = slip(text) {
            out.push(Finding::Violation(format!("{what}: {kind} {at}")));
        }
    }
}

#[cfg(test)]
#[path = "tests/text_tests.rs"]
mod tests;
