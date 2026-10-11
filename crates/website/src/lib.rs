//! Cross-check the profile against the applicant's own website.
//!
//! The applicant's site publishes schema.org JSON-LD: a `Person` with
//! `hasCredential`, and `ItemList`s named "Professional Experience" and
//! "Certifications and Credentials". That structured data, not the rendered
//! page, is what this reads, so a redesign of the page doesn't break it.
//!
//! The site is a curated subset of the profile, so the check runs one way for
//! roles (every role on the site must be in the profile, with the same dates)
//! and both ways for credentials (they are few, and an expiry that disagrees
//! means one of the two is out of date).

use admit_domain::{Experience, Finding, License, Profile};
use serde_json::Value;
use time::Date;

#[derive(Debug, thiserror::Error)]
pub enum SiteError {
    #[error("fetching {url}: {source}")]
    Fetch { url: String, source: reqwest::Error },
    #[error("fetching {url}: HTTP {status}")]
    Http { url: String, status: u16 },
    #[error("no schema.org JSON-LD on {0}")]
    NoJsonLd(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Role {
    pub title: String,
    pub organization: String,
    /// `YYYY-MM-DD` as published.
    pub start: Option<String>,
    pub end: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Credential {
    pub name: String,
    pub organization: String,
    pub identifier: Option<String>,
    /// From "active (expires sep 2028)": (2028, 9).
    pub expires: Option<(i32, u8)>,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Published {
    pub roles: Vec<Role>,
    pub credentials: Vec<Credential>,
}

pub async fn fetch(url: &str) -> Result<String, SiteError> {
    let err = |source| SiteError::Fetch { url: url.into(), source };
    let resp = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .user_agent(concat!("admit/", env!("CARGO_PKG_VERSION")))
        .build()
        .unwrap_or_default()
        .get(url)
        .send()
        .await
        .map_err(err)?;
    if !resp.status().is_success() {
        return Err(SiteError::Http { url: url.into(), status: resp.status().as_u16() });
    }
    resp.text().await.map_err(err)
}

/// The JSON-LD blocks of a page.
fn json_ld(html: &str) -> Vec<Value> {
    let mut out = Vec::new();
    let mut at = 0;
    while let Some(i) = html[at..].find("application/ld+json").map(|i| i + at) {
        let Some(open) = html[i..].find('>').map(|o| i + o + 1) else { break };
        let Some(close) = html[open..].find("</script>").map(|c| open + c) else { break };
        out.extend(serde_json::from_str::<Value>(&html[open..close]).ok());
        at = close;
    }
    out
}

fn text(v: &Value) -> String {
    v.as_str().unwrap_or_default().trim().to_string()
}

fn opt_text(v: &Value) -> Option<String> {
    Some(text(v)).filter(|s| !s.is_empty())
}

/// "active (expires sep 2028)" → (2028, 9).
pub fn expiry(category: &str) -> Option<(i32, u8)> {
    let rest = &category[category.to_ascii_lowercase().find("expires ")? + "expires ".len()..];
    let mut words = rest.split(|c: char| !c.is_ascii_alphanumeric()).filter(|w| !w.is_empty());
    let month = words.next()?.to_ascii_lowercase();
    const M: [&str; 12] = ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"];
    let m = M.iter().position(|x| month.starts_with(x))? as u8 + 1;
    Some((words.next()?.parse().ok()?, m))
}

fn credential(v: &Value) -> Option<Credential> {
    Some(Credential {
        name: opt_text(&v["name"])?,
        organization: text(&v["recognizedBy"]["name"]),
        identifier: opt_text(&v["identifier"]),
        expires: v["credentialCategory"].as_str().and_then(expiry),
    })
}

pub fn parse(html: &str) -> Published {
    let mut p = Published::default();
    let mut nodes: Vec<Value> = Vec::new();
    for doc in json_ld(html) {
        match doc.get("@graph").and_then(Value::as_array) {
            Some(g) => nodes.extend(g.iter().cloned()),
            None => nodes.push(doc),
        }
    }
    for n in &nodes {
        let items =
            || n["itemListElement"].as_array().map(Vec::as_slice).unwrap_or_default().iter().map(|i| &i["item"]);
        match (n["@type"].as_str(), n["name"].as_str()) {
            (Some("Person"), _) => {
                p.credentials.extend(n["hasCredential"].as_array().into_iter().flatten().filter_map(credential));
            }
            (Some("ItemList"), Some("Certifications and Credentials")) => {
                p.credentials.extend(items().filter_map(credential));
            }
            (Some("ItemList"), Some("Professional Experience")) => p.roles.extend(items().filter_map(|i| {
                Some(Role {
                    title: opt_text(&i["roleName"])?,
                    organization: text(&i["worksFor"]["name"]),
                    start: opt_text(&i["startDate"]),
                    end: opt_text(&i["endDate"]),
                })
            })),
            _ => {}
        }
    }
    // The Person and the list publish the same credentials.
    let mut seen = std::collections::BTreeSet::new();
    p.credentials.retain(|c| seen.insert((norm(&c.name), c.identifier.clone().map(|i| norm(&i)))));
    p
}

/// `parse`, failing when the page carries none of the lists (wrong URL, or
/// the structured data was removed) instead of reporting everything missing.
pub fn published(url: &str, html: &str) -> Result<Published, SiteError> {
    let p = parse(html);
    if p.roles.is_empty() && p.credentials.is_empty() {
        return Err(SiteError::NoJsonLd(url.into()));
    }
    Ok(p)
}

/// Upper-cased alphanumerics, "&" read as "AND".
fn norm(s: &str) -> String {
    s.replace('&', " and ").chars().filter(char::is_ascii_alphanumeric).map(|c| c.to_ascii_uppercase()).collect()
}

fn words(s: &str) -> std::collections::BTreeSet<String> {
    s.replace('&', " and ")
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| w.len() > 2)
        .map(str::to_ascii_lowercase)
        .collect()
}

/// Jaccard similarity of the titles' words. (Share of the *smaller* title's
/// words was used first: it scored a long title that contains every word of
/// a short one as a perfect match, pairing the site's "Computational Biology
/// & Bioinformatics Researcher" with "Summer Science Research Institute
/// Bioinformatics and Computational Biology Researcher".)
fn overlap(a: &str, b: &str) -> f64 {
    let (a, b) = (words(a), words(b));
    let union = a.union(&b).count();
    if union == 0 { 0.0 } else { a.intersection(&b).count() as f64 / union as f64 }
}

fn same_org(a: &str, b: &str) -> bool {
    let (a, b) = (norm(a), norm(b));
    !a.is_empty() && !b.is_empty() && (a.contains(&b) || b.contains(&a))
}

fn ym(s: &str) -> Option<(i32, u8)> {
    let d = admit_domain::validate::parse_date(s)?;
    Some((d.year(), d.month() as u8))
}

fn find_license<'a>(c: &Credential, ls: &'a [License]) -> Option<&'a License> {
    // The credential ID is the strongest key: the profile keeps it in the
    // description ("Credential ID: E4097508"), which tells the two EMT
    // registrations apart.
    if let Some(id) = c.identifier.as_deref().map(norm).filter(|i| i.len() >= 4)
        && let Some(l) = ls.iter().find(|l| l.description.as_deref().is_some_and(|d| norm(d).contains(&id)))
    {
        return Some(l);
    }
    // Liaison truncates titles at 50 characters: compare as a prefix.
    let name = norm(&c.name);
    ls.iter().find(|l| {
        let t = norm(&l.title);
        !t.is_empty() && (name.starts_with(&t) || t.starts_with(&name)) && same_org(&l.organization, &c.organization)
    })
}

fn find_experience<'a>(r: &Role, xs: &'a [Experience]) -> Option<(&'a Experience, bool)> {
    let at_org: Vec<&Experience> = xs.iter().filter(|x| same_org(&x.organization, &r.organization)).collect();
    if let Some(x) = at_org.iter().find(|x| norm(&x.title) == norm(&r.title)) {
        return Some((x, true));
    }
    // Among similar titles, one that also starts in the same month wins.
    let same_start = |x: &Experience| r.start.as_deref().and_then(ym).is_some_and(|s| ym(&x.start) == Some(s));
    at_org
        .into_iter()
        .map(|x| (x, overlap(&x.title, &r.title)))
        .filter(|(_, s)| *s >= 0.5)
        .max_by(|a, b| (same_start(a.0), a.1).partial_cmp(&(same_start(b.0), b.1)).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(x, _)| (x, false))
}

/// How findings name the site; there is one, `[website] url` in admit.toml.
const SITE: &str = "the website";

pub fn compare(p: &Profile, site: &Published, today: Date) -> Vec<Finding> {
    let mut out = Vec::new();
    let ls = &p.licenses.licenses;
    let mut matched = std::collections::BTreeSet::new();
    for c in &site.credentials {
        let Some(l) = find_license(c, ls) else {
            out.push(Finding::Pending(format!(
                "{SITE} lists {:?} ({}) but licenses.toml does not",
                c.name, c.organization
            )));
            continue;
        };
        matched.insert(l.title.clone());
        let mine = l.valid_until.as_deref().and_then(ym);
        match (mine, c.expires) {
            (Some(a), Some(b)) if a != b => out.push(Finding::Violation(format!(
                "license {:?}: licenses.toml says valid until {}, {SITE} says expires {}-{:02}",
                l.title,
                l.valid_until.as_deref().unwrap_or_default(),
                b.0,
                b.1
            ))),
            (None, Some(b)) => out.push(Finding::Violation(format!(
                "license {:?}: no valid_until in licenses.toml, {SITE} says expires {}-{:02}",
                l.title, b.0, b.1
            ))),
            _ => {}
        }
        if let Some((y, m)) = c.expires
            && (y, m) < (today.year(), today.month() as u8)
        {
            out.push(Finding::Violation(format!("{SITE} shows {:?} as active but it expired {y}-{m:02}", c.name)));
        }
    }
    for l in ls.iter().filter(|l| !matched.contains(&l.title)) {
        out.push(Finding::Note(format!("license {:?} is not on {SITE}", l.title)));
    }

    let xs = &p.experiences.experiences;
    for r in &site.roles {
        let Some((x, exact)) = find_experience(r, xs) else {
            out.push(Finding::Pending(format!(
                "{SITE} lists {:?} at {} but experiences.toml does not",
                r.title, r.organization
            )));
            continue;
        };
        if !exact {
            out.push(Finding::Note(format!(
                "{} role titled {:?} on {SITE}, {:?} in experiences.toml",
                r.organization, r.title, x.title
            )));
        }
        let differs = |field: &str, mine: Option<&str>, theirs: Option<&str>, out: &mut Vec<Finding>| {
            if let (Some(a), Some(b)) = (mine.and_then(ym), theirs.and_then(ym))
                && a != b
            {
                out.push(Finding::Violation(format!(
                    "experience {:?}: {field} {}-{:02} in experiences.toml, {}-{:02} on {SITE}",
                    x.key(),
                    a.0,
                    a.1,
                    b.0,
                    b.1
                )));
            }
        };
        differs("start", Some(&x.start), r.start.as_deref(), &mut out);
        differs("end", x.end.as_deref(), r.end.as_deref(), &mut out);
    }
    out
}

#[cfg(test)]
#[path = "tests/lib_tests.rs"]
mod tests;
