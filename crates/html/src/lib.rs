//! Just enough HTML reading for machine-generated portal pages: one tag's
//! attributes, entity decoding, and the inputs of each `<form>`. Not a
//! general parser - each reader is pinned by a test against a captured page
//! of the portal that relies on it.

/// A `<form>` with every `<input>` it carries (hidden or not), in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Form {
    pub id: Option<String>,
    pub action: String,
    pub fields: Vec<(String, String)>,
}

impl Form {
    pub fn has(&self, name: &str) -> bool {
        self.fields.iter().any(|(n, _)| n == name)
    }
}

/// Every form in `html`.
pub fn forms(html: &str) -> Vec<Form> {
    let lower = html.to_ascii_lowercase();
    let mut out = Vec::new();
    let mut at = 0;
    while let Some(start) = lower[at..].find("<form").map(|i| i + at) {
        let Some(open_end) = tag_end(html, start) else { break };
        let end = lower[open_end..].find("</form").map_or(html.len(), |i| i + open_end);
        let tag = &html[start..=open_end];
        let body = &html[open_end..end];
        let mut fields = Vec::new();
        let body_lower = &lower[open_end..end];
        let mut j = 0;
        while let Some(s) = body_lower[j..].find("<input").map(|i| i + j) {
            let e = tag_end(body, s).unwrap_or(body.len());
            let input = &body[s..e];
            if let Some(name) = attr(input, "name") {
                fields.push((name, attr(input, "value").unwrap_or_default()));
            }
            j = e;
        }
        out.push(Form { id: attr(tag, "id"), action: attr(tag, "action").unwrap_or_default(), fields });
        at = end.max(open_end + 1);
    }
    out
}

/// Index of the `>` closing the tag that opens at `start`, skipping quoted
/// attribute values (a value may legally contain `>`).
pub fn tag_end(s: &str, start: usize) -> Option<usize> {
    let mut quote = None;
    for (i, c) in s[start..].char_indices() {
        match (quote, c) {
            (None, '"' | '\'') => quote = Some(c),
            (Some(q), c) if c == q => quote = None,
            (None, '>') => return Some(start + i),
            _ => {}
        }
    }
    None
}

/// The first form that carries a field called `name`.
pub fn form_with(html: &str, name: &str) -> Option<Form> {
    forms(html).into_iter().find(|f| f.has(name))
}

/// Attribute `name` of one tag, entity-decoded. `None` when absent or bare
/// (`checked`: see [`has_attr`]).
pub fn attr(tag: &str, name: &str) -> Option<String> {
    attributes(tag).into_iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).and_then(|(_, v)| v).map(unescape)
}

/// Whether the tag carries attribute `name`, with or without a value: the
/// way to read boolean attributes (`checked`, `selected`).
pub fn has_attr(tag: &str, name: &str) -> bool {
    attributes(tag).iter().any(|(n, _)| n.eq_ignore_ascii_case(name))
}

/// One tag's attributes in order, values raw. Quote-aware: text inside a
/// quoted value is never read as an attribute (`title="x value=y"` has no
/// `value`, `data-note="was checked"` is not `checked`).
fn attributes(tag: &str) -> Vec<(&str, Option<&str>)> {
    let ends_name = |c: char| c.is_ascii_whitespace() || matches!(c, '=' | '>' | '/');
    let mut rest = tag.strip_prefix('<').map_or(tag, |t| t.trim_start_matches(|c: char| !ends_name(c)));
    let mut out = Vec::new();
    loop {
        rest = rest.trim_start_matches(|c: char| c.is_ascii_whitespace() || c == '/');
        if rest.is_empty() || rest.starts_with('>') {
            return out;
        }
        let end = rest.find(ends_name).unwrap_or(rest.len());
        let name = &rest[..end];
        rest = rest[end..].trim_start();
        // `rest` starts with neither whitespace, `/` nor `>`, so an empty
        // name means it starts with `=`: a stray one, whose value is read
        // and dropped below.
        let Some(after_eq) = rest.strip_prefix('=') else {
            out.push((name, None));
            continue;
        };
        let after_eq = after_eq.trim_start();
        let (value, next) = match after_eq.chars().next() {
            None => (None, after_eq),
            Some(q @ ('"' | '\'')) => {
                let inner = &after_eq[1..];
                let close = inner.find(q).unwrap_or(inner.len());
                (Some(&inner[..close]), inner.get(close + 1..).unwrap_or(""))
            }
            Some(_) => {
                let close =
                    after_eq.find(|c: char| c.is_whitespace() || c == '>' || c == '/').unwrap_or(after_eq.len());
                (Some(&after_eq[..close]), &after_eq[close..])
            }
        };
        if !name.is_empty() {
            out.push((name, value));
        }
        rest = next;
    }
}

/// The entities these pages actually use.
pub fn unescape(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let Some(semi) = rest.find(';').filter(|&n| n <= 10) else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let ent = &rest[1..semi];
        let ch = match ent {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ => ent
                .strip_prefix("#x")
                .or_else(|| ent.strip_prefix("#X"))
                .and_then(|h| u32::from_str_radix(h, 16).ok())
                .or_else(|| ent.strip_prefix('#').and_then(|d| d.parse().ok()))
                .and_then(char::from_u32),
        };
        match ch {
            Some(c) => {
                out.push(c);
                rest = &rest[semi + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
#[path = "tests/lib_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "tests/lib_malformed_tests.rs"]
mod malformed_tests;
