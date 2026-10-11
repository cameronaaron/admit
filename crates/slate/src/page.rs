//! Reading a Slate application page: its navigation, its questions, and the
//! Review page's list of what blocks submission.
//!
//! Slate renders every question as
//! `<div class="form_question …" data-id="<guid>" data-type="select" data-export="sys:field:…">`
//! whose controls are named `form_<guid>` (dates add `_m`, `_d`, `_y`).
//! `data-export` is the stable, human-readable key; `data-readonly` marks a
//! formula copy (`data-formula="@sys:first"`) the browser fills in itself;
//! `data-invisible` marks a field the page's scripts own (institution codes).

use admit_html::{attr, has_attr, tag_end, unescape};

/// One form control: a name with its current value(s) and, for selects,
/// radios and checkboxes, the choices it offers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Input {
    pub name: String,
    pub choices: Vec<Choice>,
    /// Selected/checked choice values in page order, or the text typed in.
    /// Slate can mark two radios of one group `checked` (the default and
    /// the stored answer); the browser shows the last, so that's the answer.
    pub values: Vec<String>,
    /// `data-formula` (`@sys:first`): a value the browser computes from
    /// another question before posting.
    pub formula: Option<String>,
    /// `<input type="file">` (an upload slot, `material_<id>`).
    pub file: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choice {
    pub value: String,
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Text,
    Textarea,
    Select,
    Radio,
    Checkbox,
    /// `date` and `birthdate`: three selects, `_m`, `_d`, `_y`.
    Date,
    /// Addresses, secrets (SSN), uploads, widgets: read for completeness,
    /// never written.
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Question {
    pub id: String,
    pub kind: Kind,
    /// `data-type` as the page gave it.
    pub data_type: String,
    pub export: Option<String>,
    pub label: String,
    pub required: bool,
    pub readonly: bool,
    pub invisible: bool,
    pub inputs: Vec<Input>,
    /// `maxlength` of the text control.
    pub max_chars: Option<usize>,
    /// `words: N` in the textarea's `data-maxlength`.
    pub max_words: Option<usize>,
}

impl Question {
    /// The key answers are filed under: the export key, else the id.
    pub fn key(&self) -> &str {
        self.export.as_deref().unwrap_or(&self.id)
    }

    /// Whether `apply` may write it.
    pub fn writable(&self) -> bool {
        // Invisible fields are normally set by the page's scripts - the
        // Survey's institution codes come from the autocomplete (`admit
        // lookup`) - and must be written alongside the visible name, or
        // Review says "Institution not recognized".
        self.kind != Kind::Other && !self.readonly
    }

    /// The control named exactly `form_<id>`.
    pub fn main_input(&self) -> Option<&Input> {
        let name = format!("form_{}", self.id);
        self.inputs.iter().find(|i| i.name == name)
    }

    /// A date part: `m`, `d` or `y`.
    pub fn part(&self, suffix: &str) -> Option<&Input> {
        let name = format!("form_{}_{suffix}", self.id);
        self.inputs.iter().find(|i| i.name == name)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Page {
    /// Relative URL, `frm?<guid>`.
    pub href: String,
    pub title: String,
    pub questions: Vec<Question>,
    /// The address widget (Basic Information), if the page has one.
    pub addresses: Option<AddressWidget>,
}

/// A saved address, as the widget's table lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddressRow {
    /// "Mailing Address", "Permanent Address".
    pub kind: String,
    pub street: String,
    pub city: String,
    pub region: String,
    /// The country's name ("United States"); the save posts its code.
    pub country: String,
    pub postal: String,
}

/// Slate's address widget: a table of saved addresses and an "Insert
/// Address" popup form, saved separately from the page (captured
/// 2026-10-06: multipart POST to the popup's URL with the type, the
/// address parts, `_quality=0`, `_dirty=1`, `dirty=1`, `cmd=save`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddressWidget {
    /// The popup, relative to `/apply/`: `frm?cmd=widget&form=…&field=…&scope=…`.
    pub add: String,
    pub rows: Vec<AddressRow>,
    /// The popup's questions (`sys:address:type`, `sys:address_block`, …),
    /// filled in by [`crate::Portal::pages`].
    pub form: Vec<Question>,
}

/// The widget's table and "Insert Address" link.
pub fn address_widget(html: &str) -> Option<AddressWidget> {
    let i = html.find("class=\"widget_add\"")?;
    let start = html[..i].rfind('<')?;
    let add = attr(&html[start..=tag_end(html, start)?], "data-href")?;
    let add = add.strip_prefix("/apply/").unwrap_or(&add).to_string();
    let rows = html
        .split("<tr class=\"widget_table_row\"")
        .skip(1)
        .filter_map(|row| {
            let row = &row[..row.find("</tr>").unwrap_or(row.len())];
            let c: Vec<String> =
                row.split("<td").skip(1).map(|c| text(c.split_once('>').map_or("", |(_, r)| r))).collect();
            let [kind, street, city, region, country, postal] = <[String; 6]>::try_from(c).ok()?;
            Some(AddressRow { kind, street, city, region, country, postal })
        })
        .collect();
    Some(AddressWidget { add, rows, form: Vec::new() })
}

/// A link in the application's left-hand menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NavEntry {
    pub href: String,
    pub title: String,
}

impl NavEntry {
    /// A question page (`frm?…`); the others are lists (`aca`, `job`,
    /// `tes`), the signature (`certify`) and `review`.
    pub fn is_form(&self) -> bool {
        self.href.starts_with("frm?")
    }
}

/// The menu (`<div id="menu">`): every `apply_navigate` link but Home.
pub fn nav(html: &str) -> Vec<NavEntry> {
    let Some(start) = html.find("<div id=\"menu\"") else { return Vec::new() };
    let end = html[start..].find("</ul>").map_or(html.len(), |i| i + start);
    let menu = &html[start..end];
    let mut out = Vec::new();
    let mut at = 0;
    while let Some(s) = menu[at..].find("<a ").map(|i| i + at) {
        let Some(e) = tag_end(menu, s) else { break };
        let close = menu[e..].find("</a>").map_or(menu.len(), |i| i + e);
        let tag = &menu[s..=e];
        if let Some(href) = attr(tag, "href").filter(|h| !h.starts_with('/') && !h.starts_with("http")) {
            out.push(NavEntry { href, title: text(&menu[e + 1..close]) });
        }
        at = close;
    }
    out
}

/// The page's `<h1>` and every question on it.
pub fn page(href: &str, html: &str) -> Page {
    let title = between(html, "<h1>", "</h1>").map(text).unwrap_or_default();
    let marker = "<div class=\"form_question";
    // A marker inside the previous question's opening tag (a quoted
    // attribute value) starts no question.
    let mut starts: Vec<(usize, Option<usize>)> = Vec::new();
    for (s, _) in html.match_indices(marker) {
        if starts.last().is_some_and(|&(_, e)| e.is_some_and(|e| s <= e)) {
            continue;
        }
        starts.push((s, tag_end(html, s)));
    }
    let mut questions = Vec::new();
    for (k, &(s, open_end)) in starts.iter().enumerate() {
        let end = starts.get(k + 1).map_or(html.len(), |&(next, _)| next);
        let Some(open_end) = open_end else { continue };
        let tag = &html[s..=open_end];
        let (Some(id), Some(data_type)) = (attr(tag, "data-id"), attr(tag, "data-type")) else { continue };
        if matches!(data_type.as_str(), "p" | "header" | "h2" | "h3") {
            continue;
        }
        questions.push(question(id, data_type, tag, &html[open_end + 1..end]));
    }
    Page { href: href.to_string(), title, questions, addresses: address_widget(html) }
}

fn question(id: String, data_type: String, tag: &str, body: &str) -> Question {
    let kind = match data_type.as_str() {
        "text" => Kind::Text,
        "textarea" => Kind::Textarea,
        "select" => Kind::Select,
        "radio" => Kind::Radio,
        "checkbox" => Kind::Checkbox,
        // `dateym` (a school's From/To) has only the `_m` and `_y` parts.
        "date" | "birthdate" | "dateym" => Kind::Date,
        _ => Kind::Other,
    };
    let prefix = format!("form_{id}");
    // Uploads name their controls `material_<id>` (file input, `_required`).
    let inputs =
        if data_type == "plugin:material" { inputs(body, &format!("material_{id}")) } else { inputs(body, &prefix) };
    let main = controls(body).find(|(_, t)| attr(t, "name").as_deref() == Some(prefix.as_str()));
    let max_chars = main.as_ref().and_then(|(_, t)| attr(t, "maxlength")).and_then(|v| v.parse().ok());
    let max_words = main.as_ref().and_then(|(_, t)| attr(t, "data-maxlength")).and_then(|v| {
        let i = v.find("words:")? + "words:".len();
        v[i..].trim_start().chars().take_while(char::is_ascii_digit).collect::<String>().parse().ok()
    });
    Question {
        label: label(body),
        required: attr(tag, "data-required").as_deref() == Some("1"),
        readonly: attr(tag, "data-readonly").as_deref() == Some("1"),
        invisible: attr(tag, "data-invisible").as_deref() == Some("1"),
        export: attr(tag, "data-export").filter(|e| !e.is_empty()),
        id,
        kind,
        data_type,
        inputs,
        max_chars,
        max_words,
    }
}

/// `<label class="form_label">` or `<div class="form_label">`; failing that
/// (radios and checkboxes label each choice), the first `<label>`.
fn label(body: &str) -> String {
    for open in ["<label", "<div"] {
        let mut at = 0;
        while let Some(s) = body[at..].find(open).map(|i| i + at) {
            let Some(e) = tag_end(body, s) else { break };
            at = e;
            if attr(&body[s..=e], "class").is_some_and(|c| c.split_whitespace().any(|w| w == "form_label")) {
                let close = if open == "<label" { "</label>" } else { "</div>" };
                return body[e + 1..].find(close).map(|c| text(&body[e + 1..e + 1 + c])).unwrap_or_default();
            }
        }
    }
    between(body, "<label", "</label>").and_then(|l| l.split_once('>')).map(|(_, t)| text(t)).unwrap_or_default()
}

/// Every `<input>`, `<select>` and `<textarea>` tag, with its position.
fn controls(body: &str) -> impl Iterator<Item = (usize, &str)> {
    let mut at = 0;
    std::iter::from_fn(move || {
        let s = ["<input", "<select", "<textarea"].iter().filter_map(|t| body[at..].find(t).map(|i| i + at)).min()?;
        let e = tag_end(body, s)?;
        at = e + 1;
        Some((s, &body[s..=e]))
    })
}

/// The controls whose name starts with `prefix`, merged per name (a radio
/// group is one input with several choices). Disabled controls are kept:
/// Slate's scripts enable them, and the stored value is what matters.
fn inputs(body: &str, prefix: &str) -> Vec<Input> {
    let mut out: Vec<Input> = Vec::new();
    for (pos, tag) in controls(body) {
        let Some(name) = attr(tag, "name").filter(|n| n.starts_with(prefix)) else { continue };
        let lower = tag.to_ascii_lowercase();
        let (choices, values) = if lower.starts_with("<select") {
            let open_end = pos + tag.len();
            let close = body[open_end..].find("</select>").map_or(body.len(), |i| i + open_end);
            options(&body[open_end..close])
        } else if lower.starts_with("<textarea") {
            let open_end = pos + tag.len();
            let close = body[open_end..].find("</textarea>").map_or(body.len(), |i| i + open_end);
            let v = unescape(&body[open_end..close]);
            (Vec::new(), if v.is_empty() { vec![] } else { vec![v] })
        } else {
            let ty = attr(tag, "type").unwrap_or_default().to_ascii_lowercase();
            let value = attr(tag, "value").unwrap_or_default();
            if ty == "radio" || ty == "checkbox" {
                let text = attr(tag, "data-text").unwrap_or_else(|| value.clone());
                // Slate writes `checked="checked"` (all 29 in the 2026-10-06
                // captures); a bare `checked` is the same in HTML, so both count.
                let checked = has_attr(tag, "checked");
                (vec![Choice { value: value.clone(), text }], if checked { vec![value] } else { vec![] })
            } else {
                (Vec::new(), if value.is_empty() { vec![] } else { vec![value] })
            }
        };
        match out.iter_mut().find(|i| i.name == name) {
            Some(i) => {
                i.choices.extend(choices);
                i.values.extend(values);
            }
            None => out.push(Input {
                name,
                choices,
                values,
                formula: attr(tag, "data-formula"),
                file: attr(tag, "type").is_some_and(|t| t.eq_ignore_ascii_case("file")),
            }),
        }
    }
    out
}

/// `<option>`s: the non-empty choices and the selected values.
fn options(inner: &str) -> (Vec<Choice>, Vec<String>) {
    let mut choices = Vec::new();
    let mut selected = Vec::new();
    let mut at = 0;
    while let Some(s) = inner[at..].find("<option").map(|i| i + at) {
        let Some(e) = tag_end(inner, s) else { break };
        let close = inner[e..].find("</option>").map_or(inner.len(), |i| i + e);
        let tag = &inner[s..=e];
        let shown = text(&inner[e + 1..close]);
        let value = attr(tag, "value").unwrap_or_else(|| shown.clone());
        if has_attr(tag, "selected") && !value.is_empty() {
            selected.push(value.clone());
        }
        if !value.is_empty() {
            choices.push(Choice { value, text: attr(tag, "data-text").unwrap_or(shown) });
        }
        at = close;
    }
    (choices, selected)
}

/// A widget table's rows: each row's label and its popup (relative to
/// `/apply/`), e.g. Additional Academic Info's one form per institution.
pub fn widget_rows(html: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for row in html.split("<tr").skip(1) {
        let Some(i) = row.find("data-href=\"/apply/frm?cmd=widget") else { continue };
        let Some(start) = row[..i].rfind('<') else { continue };
        let Some(end) = tag_end(row, start) else { continue };
        let Some(href) = attr(&row[start..=end], "data-href") else { continue };
        if !href.contains("&id=") {
            continue;
        }
        let close = row[end..].find("</a>").map_or(row.len(), |k| k + end);
        out.push((text(&row[end + 1..close]), href.trim_start_matches("/apply/").to_string()));
    }
    out
}

/// Rows in a list section's table (`<div id="part_table">`: Academic
/// History, Professional Experience, Test Scores, Recommendations), or
/// `None` when the page has no such table.
pub fn list_rows(html: &str) -> Option<usize> {
    let table = &html[html.find("<div id=\"part_table\"")?..];
    let table = &table[..table.find("</table>").unwrap_or(table.len())];
    Some(table.find("<tbody>").map_or(0, |i| table[i..].matches("<tr").count()))
}

/// Professional Experience's rows as (employer, dates), or `None` if this
/// isn't that page.
pub fn jobs_listed(html: &str) -> Option<Vec<(String, String)>> {
    if !html.contains("<h1>Professional Experience</h1>") {
        return None;
    }
    let table = &html[html.find("<div id=\"part_table\"")?..];
    let table = &table[..table.find("</table>").unwrap_or(table.len())];
    let Some(body) = table.find("<tbody>").map(|i| &table[i..]) else { return Some(Vec::new()) };
    Some(
        body.split("<tr")
            .skip(1)
            .filter_map(|row| {
                let cells: Vec<String> =
                    row.split("<td").skip(1).map(|c| text(c.split_once('>').map_or("", |(_, r)| r))).collect();
                Some((cells.first()?.clone(), cells.get(1)?.clone()))
            })
            .filter(|(e, _)| !e.is_empty())
            .collect(),
    )
}

/// Academic History's institutions (first column of the first table), or
/// `None` if this isn't that page.
pub fn institutions_listed(html: &str) -> Option<Vec<String>> {
    if !html.contains("<h1>Academic History") {
        return None;
    }
    let table = &html[html.find("<div id=\"part_table\"")?..];
    let table = &table[..table.find("</table>").unwrap_or(table.len())];
    let Some(body) = table.find("<tbody>").map(|i| &table[i..]) else { return Some(Vec::new()) };
    Some(
        body.split("<tr")
            .skip(1)
            .filter_map(|row| row.split("<td").nth(1).map(|c| text(c.split_once('>').map_or("", |(_, r)| r))))
            .filter(|n| !n.is_empty())
            .collect(),
    )
}

/// Add Institution's degree choices per level (`U`, `G`), from the
/// `<div class="hidden level level_X"><select name="degree">` blocks.
pub fn degree_choices(html: &str) -> std::collections::BTreeMap<String, Vec<Choice>> {
    let mut out = std::collections::BTreeMap::new();
    for part in html.split("<div class=\"hidden level level_").skip(1) {
        let level: String = part.chars().take_while(char::is_ascii_alphanumeric).collect();
        let Some(s) = part.find("<select") else { continue };
        let inner = &part[s..part[s..].find("</select>").map_or(part.len(), |i| i + s)];
        let inner = &inner[inner.find('>').map_or(0, |i| i + 1)..];
        out.insert(level, options(inner).0);
    }
    out
}

/// One row of a status-portal checklist (`/apply/status`, after
/// submission).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChecklistItem {
    /// The checklist's heading ("Application Checklist").
    pub list: String,
    /// "Received", "Awaiting", "Waived", …
    pub status: String,
    pub detail: String,
    /// `MM/DD/YYYY`, or empty.
    pub date: String,
}

/// Every `<div id="part_checklist_…">` table on the status page.
pub fn checklists(html: &str) -> Vec<ChecklistItem> {
    let mut out = Vec::new();
    for part in html.split("<div id=\"part_checklist_").skip(1) {
        let part = &part[..part.find("</table>").unwrap_or(part.len())];
        let list = between(part, "<h3>", "</h3>").map(text).unwrap_or_default();
        for row in part.split("<tr class=\"plain\"").skip(1) {
            let cells: Vec<String> =
                row.split("<td").skip(1).map(|c| text(c.split_once('>').map_or("", |(_, r)| r))).collect();
            if let [_, status, detail, date, ..] = cells.as_slice() {
                out.push(ChecklistItem {
                    list: list.clone(),
                    status: status.clone(),
                    detail: detail.clone(),
                    date: date.clone(),
                });
            }
        }
    }
    out
}

/// The status page's "An update to your application was last posted
/// <date>." line, if there is one.
pub fn update_posted(html: &str) -> Option<String> {
    let i = html.find("was last posted ")? + "was last posted ".len();
    let date = &html[i..i + html[i..].find(['<', '.'])?];
    Some(text(date)).filter(|d| !d.is_empty())
}

/// The update page's PDF (`update?cmd=stream&id=<id>`), relative to
/// `/apply/`, with the date it was released.
pub fn update_pdf(html: &str) -> Option<(String, String)> {
    let i = html.find("id=\"pager_download\"")?;
    let start = html[..i].rfind('<')?;
    let href = attr(&html[start..=tag_end(html, start)?], "data-href")?;
    let released = html
        .find("class=\"update_released\"")
        .and_then(|j| html[j..].find('>').map(|k| j + k + 1))
        .and_then(|j| html[j..].find("</p>").map(|k| text(&html[j..j + k])))
        .unwrap_or_default();
    Some((href, released))
}

/// A row of the `/apply/` list ("Your Applications").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Application {
    pub id: String,
    pub name: String,
    pub status: String,
}

/// Every application the account has started, from `/apply/`. Each row's
/// link is `data-href="?id=<id>&amp;cmd=detail"`.
pub fn applications(html: &str) -> Vec<Application> {
    let mut out = Vec::new();
    for row in html.split("<tr").skip(1) {
        let Some(link) = row.find("data-href=").and_then(|i| attr(&row[i..], "data-href")) else {
            continue;
        };
        let Some(id) = link.strip_prefix("?id=").and_then(|r| r.split('&').next()).filter(|i| !i.is_empty()) else {
            continue;
        };
        let cells: Vec<String> =
            row.split("<td").skip(1).map(|c| text(c.split_once('>').map_or("", |(_, r)| r))).collect();
        out.push(Application {
            id: id.to_string(),
            name: cells.first().cloned().unwrap_or_default(),
            status: cells.get(1).cloned().unwrap_or_default(),
        });
    }
    out
}

/// One row of the Review page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issue {
    /// Section title as the menu shows it.
    pub section: String,
    pub href: String,
    pub message: String,
    /// An error blocks submission; a warning is the portal's "potential problem".
    pub error: bool,
}

/// The Review page's error and warning tables, or `None` if this isn't the
/// Review page (so a changed page can't read as "no errors").
pub fn review(html: &str) -> Option<Vec<Issue>> {
    if !html.contains("<h1>Review</h1>") {
        return None;
    }
    let mut out = Vec::new();
    let mut at = 0;
    while let Some(s) = html[at..].find("<tbody>").map(|i| i + at) {
        let e = html[s..].find("</tbody>").map_or(html.len(), |i| i + s);
        let before = &html[..s];
        let error = before.rfind("<p class=\"error\"") > before.rfind("<p class=\"warning\"");
        for row in html[s..e].split("<tr").skip(1) {
            let cells: Vec<&str> = row.split("<td").skip(1).collect();
            let [sec, msg, ..] = cells.as_slice() else { continue };
            let href = sec.find("<a ").and_then(|i| attr(&sec[i..], "href")).unwrap_or_default();
            let cell = |c: &str| text(c.split_once('>').map_or("", |(_, r)| r));
            out.push(Issue { section: cell(sec), href, message: cell(msg), error });
        }
        at = e;
    }
    Some(out)
}

/// The text of every `<p class="error">` on a page: how Slate states why
/// it refused something (sign-in shows "This browser does not appear to be
/// configured to accept cookies." this way, captures/ucla/live_login.html).
pub fn errors(html: &str) -> Vec<String> {
    html.split("<p class=\"error\"")
        .skip(1)
        .filter_map(|p| {
            let body = &p[p.find('>')? + 1..];
            Some(text(&body[..body.find("</p>")?]))
        })
        .filter(|t| !t.is_empty())
        .collect()
}

fn between<'a>(s: &'a str, open: &str, close: &str) -> Option<&'a str> {
    let i = s.find(open)? + open.len();
    Some(&s[i..i + s[i..].find(close)?])
}

/// Markup to plain text: tags dropped (`<br>` as a space), entities
/// decoded, whitespace collapsed.
pub fn text(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut depth = false;
    for c in html.chars() {
        match c {
            '<' => {
                depth = true;
                out.push(' ');
            }
            '>' if depth => depth = false,
            _ if !depth => out.push(c),
            _ => {}
        }
    }
    unescape(&out).split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
#[path = "tests/page_tests.rs"]
mod tests;
