//! Declared answers (`profile/slate/<portal>.toml`) against the live pages:
//! what to change, and the form fields that change it.
//!
//! An answer is what the page shows, not Slate's internal value: the option
//! text for selects, radios and checkboxes (a list for checkboxes),
//! `YYYY-MM-DD` for dates, the text itself otherwise. The internal value is
//! accepted too.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{self, Write as _};

use admit_domain::Finding;
use serde::{Deserialize, Serialize};

use crate::page::{Input, Kind, Page, Question};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Answer {
    One(String),
    Many(Vec<String>),
}

impl fmt::Display for Answer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Answer::One(s) => write!(f, "{s:?}"),
            Answer::Many(v) => write!(f, "{v:?}"),
        }
    }
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnswersFile {
    #[serde(default)]
    pub answers: BTreeMap<String, Answer>,
    /// The address widget's entries (Basic Information).
    #[serde(default)]
    pub addresses: Vec<Address>,
    /// Academic History.
    #[serde(default)]
    pub institutions: Vec<crate::institutions::Institution>,
    /// Professional Experience.
    #[serde(default)]
    pub jobs: Vec<crate::jobs::Job>,
    /// Profile items or sections deliberately left off this application,
    /// with the reason (`"Test Scores" = "MECN requires no tests"`). The
    /// coverage audit reports anything neither on the portal nor here.
    #[serde(default)]
    pub excluded: BTreeMap<String, String>,
}

impl From<BTreeMap<String, Answer>> for AnswersFile {
    fn from(answers: BTreeMap<String, Answer>) -> AnswersFile {
        AnswersFile { answers, ..Default::default() }
    }
}

/// One address, as the "Insert Address" form takes it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Address {
    /// The type's value: `mailing` or `permanent`.
    pub kind: String,
    pub street: String,
    pub city: String,
    /// Region code (`CA`).
    pub region: String,
    /// Country code (`US`).
    pub country: String,
    pub postal: String,
}

/// An address to insert through the widget.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddressAdd {
    pub page: String,
    pub page_title: String,
    /// The widget popup, relative to `/apply/`.
    pub post_to: String,
    pub address: Address,
    pub fields: Vec<(String, String)>,
}

/// One question to change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    pub page: String,
    pub page_title: String,
    pub question_id: String,
    pub key: String,
    pub label: String,
    pub from: Option<Answer>,
    pub to: Answer,
    /// What to POST for it (besides `dirty=1`).
    pub fields: Vec<(String, String)>,
}

#[derive(Debug, Default)]
pub struct Plan {
    pub changes: Vec<Change>,
    pub addresses: Vec<AddressAdd>,
    /// Filled in by `Portal::plan_institutions` (needs the live list).
    pub institutions: Vec<crate::institutions::InstitutionAdd>,
    /// Filled in by `Portal::plan_jobs`.
    pub jobs: Vec<crate::jobs::JobAdd>,
    /// Per-institution popup forms (Additional Academic Info) that
    /// `changes` refer to, read for the plan.
    pub school_pages: Vec<Page>,
    pub findings: Vec<Finding>,
    /// Answers whose key no page has (each also a Violation in `findings`):
    /// a typo, or a page not shown yet (`apply --reveal`).
    pub unplaced: Vec<String>,
}

impl Plan {
    pub fn count(&self) -> usize {
        self.changes.len() + self.addresses.len() + self.institutions.len() + self.jobs.len()
    }

    pub fn render(&self) -> String {
        let mut out = String::new();
        if !self.jobs.is_empty() {
            out.push_str("Professional Experience (job)\n");
        }
        for a in &self.jobs {
            let j = &a.job;
            let _ = writeln!(
                out,
                "  + {} - {} ({}..{}, {} h/wk)",
                j.employer,
                j.title,
                j.from,
                j.to.as_deref().unwrap_or("present"),
                j.hours
            );
        }
        if !self.institutions.is_empty() {
            out.push_str("Academic History (aca)\n");
        }
        for a in &self.institutions {
            let i = &a.institution;
            let _ = writeln!(
                out,
                "  + {} ({}, {}) {}..{} {} {} [{}]",
                i.name,
                i.city,
                i.region,
                i.from,
                i.to,
                i.level,
                i.degree.as_deref().unwrap_or("no degree"),
                i.transcript
            );
        }
        for a in &self.addresses {
            let x = &a.address;
            let _ = writeln!(
                out,
                "{} ({})\n  + {} address: {}, {}, {} {}, {}",
                a.page_title, a.page, x.kind, x.street, x.city, x.region, x.postal, x.country
            );
        }
        let mut page = "";
        for c in &self.changes {
            if c.page_title != page {
                page = &c.page_title;
                let _ = writeln!(out, "{page} ({})", c.page);
            }
            let from = c.from.as_ref().map_or("(blank)".to_string(), |a| short(&a.to_string()));
            let _ = writeln!(out, "  ~ {}: {from} -> {}", c.key, short(&c.to.to_string()));
        }
        for f in &self.findings {
            let _ = writeln!(out, "{f}");
        }
        out
    }
}

fn short(s: &str) -> String {
    let s = s.replace('\n', "\\n");
    match s.char_indices().nth(90) {
        Some((i, _)) => format!("{}… ({} chars)", &s[..i], s.chars().count()),
        None => s,
    }
}

/// The question's current answer, as an [`Answer`] would state it; `None`
/// when blank (or a kind this module doesn't read).
pub fn current(q: &Question) -> Option<Answer> {
    match q.kind {
        Kind::Text | Kind::Textarea => {
            q.main_input()?.values.first().filter(|v| !v.is_empty()).map(|v| Answer::One(v.clone()))
        }
        Kind::Select | Kind::Radio => {
            let i = q.main_input()?;
            i.values.last().map(|v| Answer::One(choice_text(i, v)))
        }
        Kind::Checkbox => {
            let i = q.main_input()?;
            (!i.values.is_empty()).then(|| Answer::Many(i.values.iter().map(|v| choice_text(i, v)).collect()))
        }
        Kind::Date => {
            let part = |s| q.part(s).and_then(|i| i.values.first().cloned());
            Some(Answer::One(match q.part("d") {
                Some(_) => format!("{}-{}-{}", part("y")?, part("m")?, part("d")?),
                None => format!("{}-{}", part("y")?, part("m")?),
            }))
        }
        Kind::Other => None,
    }
}

fn choice_text(i: &Input, value: &str) -> String {
    i.choices.iter().find(|c| c.value == value).map_or_else(|| value.to_string(), |c| c.text.clone())
}

/// The choice value an answer names: its text (case and spacing aside) or
/// its value.
fn choose(i: &Input, want: &str) -> Result<String, String> {
    let norm = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase();
    let w = norm(want);
    i.choices.iter().find(|c| norm(&c.text) == w || c.value == want).map(|c| c.value.clone()).ok_or_else(|| {
        let some: Vec<&str> = i.choices.iter().take(12).map(|c| c.text.as_str()).collect();
        format!(
            "{want:?} is not a choice (choices: {}{})",
            some.join(" | "),
            if i.choices.len() > some.len() { " | …" } else { "" }
        )
    })
}

/// Text as Slate stores it: CRLF from the browser compares equal to LF.
fn same_text(a: &str, b: &str) -> bool {
    a.replace("\r\n", "\n").trim() == b.replace("\r\n", "\n").trim()
}

/// The fields that set `q` to `want`, and whether that differs from now.
/// `Err` says why the answer can't be written.
pub fn encode(q: &Question, want: &Answer) -> Result<(bool, Vec<(String, String)>), String> {
    let name = format!("form_{}", q.id);
    let one = || match want {
        Answer::One(s) => Ok(s.as_str()),
        Answer::Many(_) => Err(format!("{} takes one answer, not a list", q.key())),
    };
    match q.kind {
        Kind::Text | Kind::Textarea => {
            let s = one()?;
            if let Some(max) = q.max_chars.filter(|&m| s.chars().count() > m) {
                return Err(format!("{} chars, the field takes {max}", s.chars().count()));
            }
            if let Some(max) = q.max_words.filter(|&m| s.split_whitespace().count() > m) {
                return Err(format!("{} words, the limit is {max}", s.split_whitespace().count()));
            }
            let now = q.main_input().and_then(|i| i.values.first()).map_or("", String::as_str);
            Ok((!same_text(now, s), vec![(name, s.to_string())]))
        }
        Kind::Select | Kind::Radio => {
            let i = q.main_input().ok_or("no control on the page")?;
            let v = choose(i, one()?)?;
            Ok((i.values.last() != Some(&v), vec![(name, v)]))
        }
        Kind::Checkbox => {
            let i = q.main_input().ok_or("no control on the page")?;
            let wanted: Vec<&str> = match want {
                Answer::One(s) => vec![s.as_str()],
                Answer::Many(v) => v.iter().map(String::as_str).collect(),
            };
            if wanted.is_empty() {
                // An unchecked box posts nothing, so "none" can't be sent.
                return Err("clearing every box is not supported; leave the question out instead".into());
            }
            let values = wanted.into_iter().map(|w| choose(i, w)).collect::<Result<Vec<_>, _>>()?;
            let now: BTreeSet<&String> = i.values.iter().collect();
            let differs = now != values.iter().collect();
            Ok((differs, values.into_iter().map(|v| (name.clone(), v)).collect()))
        }
        Kind::Date => {
            let s = one()?;
            let parts: Vec<&str> = s.split('-').collect();
            let wanted: Vec<(&str, &str)> = match (parts.as_slice(), q.part("d").is_some()) {
                ([y, m, d], true) => vec![("m", *m), ("d", *d), ("y", *y)],
                ([y, m], false) => vec![("m", *m), ("y", *y)],
                (_, true) => return Err(format!("{s:?} is not YYYY-MM-DD")),
                (_, false) => return Err(format!("{s:?} is not YYYY-MM")),
            };
            let mut fields = Vec::new();
            let mut differs = false;
            for (suffix, v) in wanted {
                let i = q.part(suffix).ok_or("date control missing")?;
                if !i.choices.iter().any(|c| c.value == v) {
                    return Err(format!("{s:?}: {v:?} is not offered for the {suffix} part"));
                }
                differs |= i.values.first().map(String::as_str) != Some(v);
                fields.push((i.name.clone(), v.to_string()));
            }
            Ok((differs, fields))
        }
        Kind::Other => Err(format!("{} questions are not written by admit", q.data_type)),
    }
}

/// The writable questions a key names (export key or question id).
fn resolve<'a>(pages: &'a [Page], key: &str) -> Vec<(&'a Page, &'a Question)> {
    pages
        .iter()
        .flat_map(|p| p.questions.iter().map(move |q| (p, q)))
        .filter(|(_, q)| q.writable() && (q.export.as_deref() == Some(key) || q.id == key))
        .collect()
}

/// The country a row names matches a code: the name itself, or the one
/// pairing seen on this portal ("United States" for `US`).
fn same_country(row: &str, code: &str) -> bool {
    row.eq_ignore_ascii_case(code) || (code.eq_ignore_ascii_case("US") && row == "United States")
}

fn same(a: &str, b: &str) -> bool {
    let n = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase();
    n(a) == n(b)
}

/// Addresses missing from the widget, as inserts. One that exists with
/// different parts is reported: editing an address was never captured.
fn plan_addresses(pages: &[Page], want: &[Address], out: &mut Plan) {
    if want.is_empty() {
        return;
    }
    let Some((page, w)) = pages.iter().find_map(|p| Some((p, p.addresses.as_ref()?))) else {
        out.findings.push(Finding::Violation("addresses are declared but no page has an address widget".into()));
        return;
    };
    let find = |export: &str| w.form.iter().find(|q| q.export.as_deref() == Some(export));
    let (Some(ty), Some(block)) = (find("sys:address:type"), find("sys:address_block")) else {
        out.findings
            .push(Finding::Violation(format!("{}: the address form has no type/address questions", page.title)));
        return;
    };
    for a in want {
        let Some(choice) = ty.main_input().and_then(|i| i.choices.iter().find(|c| c.value == a.kind)) else {
            out.findings.push(Finding::Violation(format!(
                "address kind {:?} is not offered ({})",
                a.kind,
                ty.main_input()
                    .map(|i| i.choices.iter().map(|c| c.value.as_str()).collect::<Vec<_>>().join(", "))
                    .unwrap_or_default()
            )));
            continue;
        };
        match w.rows.iter().find(|r| same(&r.kind, &choice.text)) {
            Some(r) => {
                let matches = same(&r.street, &a.street)
                    && same(&r.city, &a.city)
                    && same(&r.region, &a.region)
                    && same(&r.postal, &a.postal)
                    && same_country(&r.country, &a.country);
                if !matches {
                    out.findings.push(Finding::Pending(format!(
                        "{}: the portal's {} ({}, {}, {} {}) differs from the profile; edit it in the browser \
                         (editing an address is not automated)",
                        page.title, r.kind, r.street, r.city, r.region, r.postal
                    )));
                }
            }
            None => {
                let b = format!("form_{}", block.id);
                let fields = vec![
                    (format!("form_{}", ty.id), a.kind.clone()),
                    (format!("{b}_country"), a.country.clone()),
                    (format!("{b}_street"), a.street.clone()),
                    (format!("{b}_city"), a.city.clone()),
                    (format!("{b}_region"), a.region.clone()),
                    (format!("{b}_postal"), a.postal.clone()),
                    (format!("{b}_quality"), "0".into()),
                    (format!("{b}_dirty"), "1".into()),
                    ("cmd".into(), "save".into()),
                ];
                out.addresses.push(AddressAdd {
                    page: page.href.clone(),
                    page_title: page.title.clone(),
                    post_to: w.add.clone(),
                    address: a.clone(),
                    fields,
                });
            }
        }
    }
}

pub fn plan(pages: &[Page], file: &AnswersFile) -> Plan {
    let mut out = Plan::default();
    plan_addresses(pages, &file.addresses, &mut out);
    let answers = &file.answers;
    // Questions sharing an export key share one stored value: answering two
    // of them by id leaves only the last, and the plan never converges.
    let mut by_export: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for key in answers.keys() {
        if let [(_, q)] = resolve(pages, key).as_slice()
            && let Some(e) = q.export.as_deref()
        {
            by_export.entry(e).or_default().push(key);
        }
    }
    let mut clashing: Vec<&str> = Vec::new();
    for (export, keys) in by_export.iter().filter(|(_, k)| k.len() > 1) {
        out.findings.push(Finding::Violation(format!(
            "answers {} all write {export:?}, one stored value; Slate keeps only the last, so answer one",
            keys.iter().map(|k| format!("{k:?}")).collect::<Vec<_>>().join(", ")
        )));
        clashing.extend(keys);
    }
    for (key, want) in answers {
        if clashing.contains(&key.as_str()) {
            continue;
        }
        let found = resolve(pages, key);
        let (page, q) = match found.as_slice() {
            [one] => *one,
            [] => {
                out.findings.push(Finding::Violation(format!("answer {key:?}: no writable question with that key")));
                out.unplaced.push(key.clone());
                continue;
            }
            many => {
                let ids: Vec<&str> = many.iter().map(|(_, q)| q.id.as_str()).collect();
                out.findings.push(Finding::Violation(format!(
                    "answer {key:?}: {} questions share that key; use the question id instead ({})",
                    many.len(),
                    ids.join(", ")
                )));
                continue;
            }
        };
        match encode(q, want) {
            Ok((false, _)) => {}
            Ok((true, fields)) => out.changes.push(Change {
                page: page.href.clone(),
                page_title: page.title.clone(),
                question_id: q.id.clone(),
                key: key.clone(),
                label: q.label.clone(),
                from: current(q),
                to: want.clone(),
                fields,
            }),
            Err(why) => out.findings.push(Finding::Violation(format!("answer {key:?} ({}): {why}", page.title))),
        }
    }
    // Page order, as the menu lists them.
    let order = |href: &str| pages.iter().position(|p| p.href == href).unwrap_or(usize::MAX);
    out.changes.sort_by_key(|c| order(&c.page));
    out
}

/// Every answered, writable question as an answers file, in page order,
/// each line commented with its label. A key shared by several questions
/// is written as the question id.
pub fn to_toml(pages: &[Page], header: &str) -> String {
    let mut out = format!("{header}\n[answers]\n");
    let mut uses: BTreeMap<&str, usize> = BTreeMap::new();
    for q in pages.iter().flat_map(|p| &p.questions).filter(|q| q.writable()) {
        if let Some(e) = &q.export {
            *uses.entry(e).or_default() += 1;
        }
    }
    for p in pages {
        let answered: Vec<(&Question, Answer)> =
            p.questions.iter().filter(|q| q.writable()).filter_map(|q| Some((q, current(q)?))).collect();
        if answered.is_empty() {
            continue;
        }
        let _ = writeln!(out, "\n# {}", comment(&p.title));
        for (q, a) in answered {
            let key = match &q.export {
                Some(e) if uses.get(e.as_str()) == Some(&1) => e.as_str(),
                _ => q.id.as_str(),
            };
            let value = match a {
                Answer::One(s) => toml::Value::String(s),
                Answer::Many(v) => toml::Value::Array(v.into_iter().map(toml::Value::String).collect()),
            };
            let label: String = comment(&q.label).chars().take(80).collect();
            let label = if label.is_empty() { String::new() } else { format!("  # {label}") };
            let _ = writeln!(out, "{} = {value}{label}", toml_key(key));
        }
    }
    for p in pages {
        let Some(w) = &p.addresses else { continue };
        let ty = w.form.iter().find(|q| q.export.as_deref() == Some("sys:address:type"));
        for r in &w.rows {
            let kind = ty
                .and_then(|q| q.main_input())
                .and_then(|i| i.choices.iter().find(|c| same(&c.text, &r.kind)))
                .map_or_else(|| r.kind.clone(), |c| c.value.clone());
            let country = if r.country == "United States" { "US".to_string() } else { r.country.clone() };
            let s = |v: &str| toml::Value::String(v.to_string());
            let _ = write!(
                out,
                "\n[[addresses]]  # {}\nkind = {}\nstreet = {}\ncity = {}\nregion = {}\ncountry = {}\npostal = {}\n",
                comment(&p.title),
                s(&kind),
                s(&r.street),
                s(&r.city),
                s(&r.region),
                s(&country),
                s(&r.postal)
            );
        }
    }
    out
}

/// Page text as a TOML comment, which may not hold control characters.
fn comment(text: &str) -> String {
    text.chars().filter(|c| !c.is_control()).collect()
}

/// `key` as a TOML key: a one-line basic string. `toml::Value`'s own
/// rendering turns a string with a newline into a `"""` string, which TOML
/// does not allow as a key.
fn toml_key(key: &str) -> String {
    let mut out = String::from("\"");
    for c in key.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if c.is_control() => {
                let _ = write!(out, "\\u{:04X}", u32::from(c));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// A page save: text fields, and upload slots sent empty.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Post {
    pub fields: Vec<(String, String)>,
    /// File inputs posted with no file, as a browser does when none is
    /// chosen (keeps what is uploaded; admit never uploads).
    pub empty_files: Vec<String>,
}

/// Everything the page's Continue button would post, with `changes` swapped
/// in. Slate clears a page's fields that a save leaves out (seen live
/// 2026-10-06: posting one changed select blanked the other ten answers on
/// Biographical Data), so a save must carry the whole page.
pub fn page_post(page: &Page, changes: &[&Change]) -> Result<Post, String> {
    let mut post = Post::default();
    let changed = |id: &str| changes.iter().find(|c| c.question_id == id);
    let out = &mut post.fields;
    for q in &page.questions {
        if let Some(c) = changed(&q.id) {
            out.extend(c.fields.iter().cloned());
            continue;
        }
        for i in &q.inputs {
            if i.file {
                post.empty_files.push(i.name.clone());
                continue;
            }
            if let Some(f) = &i.formula {
                out.push((i.name.clone(), formula(page, changes, f)?));
                continue;
            }
            match q.kind {
                // An unchecked box or radio posts nothing, as in the browser.
                Kind::Checkbox => out.extend(i.values.iter().map(|v| (i.name.clone(), v.clone()))),
                Kind::Radio => out.extend(i.values.last().map(|v| (i.name.clone(), v.clone()))),
                _ => out.push((i.name.clone(), i.values.last().cloned().unwrap_or_default())),
            }
        }
    }
    Ok(post)
}

/// A `data-formula`, as the browser computes it before posting: a plain
/// reference (`@sys:first`), or arithmetic over references and numbers
/// (Fee Waiver: `((@sys:app:gd_fee_waiver_household_number_text-1) * 10508) + 29526`).
/// `@<key>_text` is a choice question's selected text. A reference that is
/// blank or not a number leaves the result blank.
fn formula(page: &Page, changes: &[&Change], f: &str) -> Result<String, String> {
    let f = f.trim();
    if let Some(key) = f.strip_prefix('@').filter(|k| k.chars().all(ref_char)) {
        return reference(page, changes, key).ok_or(format!("formula {f:?}: no question {key} on {}", page.title));
    }
    let mut toks = Vec::new();
    let mut rest = f;
    while let Some(c) = rest.chars().next() {
        if c.is_whitespace() {
            rest = &rest[c.len_utf8()..];
        } else if "+-*/()".contains(c) {
            toks.push(Tok::Op(c));
            rest = &rest[1..];
        } else if c.is_ascii_digit() || c == '.' {
            let n = rest.find(|c: char| !(c.is_ascii_digit() || c == '.')).unwrap_or(rest.len());
            toks.push(Tok::Num(rest[..n].parse().map_err(|_| format!("formula {f:?}: bad number"))?));
            rest = &rest[n..];
        } else if c == '@' {
            let n = rest[1..].find(|c: char| !ref_char(c)).map_or(rest.len(), |n| n + 1);
            let key = &rest[1..n];
            let v =
                reference(page, changes, key).ok_or(format!("formula {f:?}: no question {key} on {}", page.title))?;
            match v.trim().parse::<f64>() {
                Ok(x) => toks.push(Tok::Num(x)),
                Err(_) => return Ok(String::new()),
            }
            rest = &rest[n..];
        } else {
            return Err(format!("formula {f:?}: unsupported {c:?}"));
        }
    }
    let mut at = 0;
    let x = expr(&toks, &mut at).ok_or(format!("formula {f:?}: does not parse"))?;
    if at != toks.len() {
        return Err(format!("formula {f:?}: does not parse"));
    }
    Ok(if x.fract() == 0.0 && x.abs() < 1e15 { format!("{}", x as i64) } else { format!("{x}") })
}

fn ref_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == ':'
}

#[derive(Debug, Clone, Copy)]
enum Tok {
    Num(f64),
    Op(char),
}

fn expr(t: &[Tok], at: &mut usize) -> Option<f64> {
    let mut x = term(t, at)?;
    while let Some(Tok::Op(op @ ('+' | '-'))) = t.get(*at).copied() {
        *at += 1;
        let y = term(t, at)?;
        x = if op == '+' { x + y } else { x - y };
    }
    Some(x)
}

fn term(t: &[Tok], at: &mut usize) -> Option<f64> {
    let mut x = atom(t, at)?;
    while let Some(Tok::Op(op @ ('*' | '/'))) = t.get(*at).copied() {
        *at += 1;
        let y = atom(t, at)?;
        x = if op == '*' { x * y } else { x / y };
    }
    Some(x)
}

fn atom(t: &[Tok], at: &mut usize) -> Option<f64> {
    match t.get(*at).copied()? {
        Tok::Num(x) => {
            *at += 1;
            Some(x)
        }
        Tok::Op('-') => {
            *at += 1;
            atom(t, at).map(|x| -x)
        }
        Tok::Op('(') => {
            *at += 1;
            let x = expr(t, at)?;
            matches!(t.get(*at), Some(Tok::Op(')'))).then(|| *at += 1)?;
            Some(x)
        }
        Tok::Op(_) => None,
    }
}

/// A question's value as it will be posted (`<export>`), or its selected
/// choice's text (`<export>_text`).
fn reference(page: &Page, changes: &[&Change], key: &str) -> Option<String> {
    let find = |k: &str| page.questions.iter().find(|q| !q.readonly && q.export.as_deref() == Some(k));
    let (q, text) = match find(key) {
        Some(q) => (q, false),
        None => (find(key.strip_suffix("_text")?)?, true),
    };
    let value = match changes.iter().find(|c| c.question_id == q.id) {
        Some(c) => c.fields.first().map(|(_, v)| v.clone()).unwrap_or_default(),
        None => q.main_input().and_then(|i| i.values.last().cloned()).unwrap_or_default(),
    };
    if !text {
        return Some(value);
    }
    let i = q.main_input()?;
    Some(i.choices.iter().find(|c| c.value == value).map_or(value, |c| c.text.clone()))
}

#[cfg(test)]
#[path = "tests/answer_formula_boundaries.rs"]
mod formula_boundaries;
