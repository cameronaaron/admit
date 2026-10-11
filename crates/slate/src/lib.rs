//! Slate (Technolutions) application portals, e.g. UCLA Graduate Division
//! at `https://apply.grad.ucla.edu/apply/`.
//!
//! Slate has no JSON API: each section is a server-rendered page of
//! questions ([`page`]), saved by posting its form fields back. What `admit`
//! keeps here is the set of answers ([`answer::AnswersFile`]): `plan` diffs
//! them against the pages, `apply` posts each changed page whole (Slate
//! clears what a save leaves out) and then re-reads it to prove nothing
//! else moved, `pull` writes
//! the answered questions out, and `audit` reads the portal's own Review
//! page. Lists (institutions, employers, tests), uploads and the signature
//! are left to the browser: their saves were never captured.

pub mod answer;
pub mod client;
pub mod coverage;
pub mod institutions;
pub mod jobs;
pub mod page;
pub mod personal;

use std::collections::{BTreeMap, BTreeSet};

use admit_domain::Finding;

pub use answer::{Address, AddressAdd, Answer, AnswersFile, Change, Plan};
pub use client::{Client, Error, Result};
pub use page::{Issue, NavEntry, Page, Question};

pub struct Portal {
    pub name: String,
    pub client: Client,
}

impl Portal {
    /// The Review page: the menu, and what the portal says blocks submission.
    pub async fn review(&self) -> Result<(Vec<NavEntry>, Vec<Issue>)> {
        let html = self.client.get("review").await?;
        let issues = page::review(&html).ok_or_else(|| Error::Shape("review: not the Review page".into()))?;
        let nav = page::nav(&html);
        if nav.is_empty() {
            return Err(Error::Shape("review: no application menu on the page".into()));
        }
        Ok((nav, issues))
    }

    /// Every question page, in menu order.
    pub async fn pages(&self) -> Result<Vec<Page>> {
        let (nav, _) = self.review().await?;
        let mut out = Vec::new();
        for n in nav.iter().filter(|n| n.is_form()) {
            let html = self.client.get(&n.href).await?;
            let mut p = page::page(&n.href, &html);
            if p.title.is_empty() {
                p.title.clone_from(&n.title);
            }
            if let Some(w) = &mut p.addresses {
                w.form = page::page(&w.add, &self.client.get(&w.add).await?).questions;
            }
            out.push(p);
        }
        Ok(out)
    }

    /// Posts each page's changes, then re-reads the page: an answer that
    /// was not part of the change but differs afterwards stops the run
    /// (that would mean Slate cleared fields left out of the post).
    pub async fn apply(&self, plan: &Plan, before: &[Page]) -> Result<()> {
        let mut hrefs: Vec<&str> = plan.changes.iter().map(|c| c.page.as_str()).collect();
        hrefs.dedup();
        for href in hrefs {
            let changes: Vec<&Change> = plan.changes.iter().filter(|c| c.page == href).collect();
            let old = before
                .iter()
                .chain(&plan.school_pages)
                .find(|p| p.href == href)
                .ok_or_else(|| Error::Shape(format!("{href}: not among the pages read for the plan")))?;
            let mut post = answer::page_post(old, &changes).map_err(Error::Shape)?;
            if href.starts_with("frm?cmd=widget") {
                // A popup form commits like the address widget: `cmd=save`.
                post.fields.push(("cmd".into(), "save".into()));
            }
            self.client.save(href, &post.fields, &post.empty_files).await?;
            let after = page::page(href, &self.client.get(href).await?);
            let touched: BTreeSet<&str> = changes.iter().map(|c| c.question_id.as_str()).collect();
            let moved = collateral(Some(old), &after, &touched);
            if !moved.is_empty() {
                return Err(Error::Shape(format!(
                    "saving {href} changed answers it was not asked to change: {}",
                    moved.join("; ")
                )));
            }
            for c in changes {
                println!("  saved {} {}", c.page_title, c.key);
            }
        }
        for a in &plan.addresses {
            self.client.save(&a.post_to, &a.fields, &[]).await?;
            let after = page::page(&a.page, &self.client.get(&a.page).await?);
            let rows = after.addresses.as_ref().map(|w| w.rows.as_slice()).unwrap_or_default();
            if !rows.iter().any(|r| r.street.eq_ignore_ascii_case(&a.address.street)) {
                return Err(Error::Shape(format!(
                    "inserting the {} address answered OK but {} doesn't list it",
                    a.address.kind, a.page_title
                )));
            }
            let moved = collateral(before.iter().find(|p| p.href == a.page), &after, &BTreeSet::new());
            if !moved.is_empty() {
                return Err(Error::Shape(format!(
                    "inserting an address changed {}: {}",
                    a.page_title,
                    moved.join("; ")
                )));
            }
            println!("  saved {} {} address", a.page_title, a.address.kind);
        }
        Ok(())
    }

    /// After submission: the status page's checklists and its latest update
    /// (date, and the letter's PDF path when there is one).
    pub async fn status(&self) -> Result<Status> {
        let html = self.client.get("status").await?;
        let items = page::checklists(&html);
        if items.is_empty() && !html.contains("part_checklist_") {
            return Err(Error::Shape("status: no checklist on the status page".into()));
        }
        let update = match page::update_posted(&html) {
            Some(posted) => {
                let pdf = page::update_pdf(&self.client.get("update").await?);
                Some(Update { posted, pdf: pdf.map(|(href, _)| href) })
            }
            None => None,
        };
        Ok(Status { items, update })
    }

    /// Academic History as it stands, and the Add Institution form's degree
    /// choices.
    pub async fn institutions(&self) -> Result<(Vec<String>, BTreeMap<String, Vec<page::Choice>>)> {
        let listed = page::institutions_listed(&self.client.get("aca").await?)
            .ok_or_else(|| Error::Shape("aca: not the Academic History page".into()))?;
        let degrees = page::degree_choices(&self.client.get("aca?id=").await?);
        if degrees.is_empty() {
            return Err(Error::Shape("aca?id=: no degree choices on the Add Institution form".into()));
        }
        Ok((listed, degrees))
    }

    /// Adds `plan.institutions` to `plan` (needs the live Academic History).
    pub async fn plan_institutions(&self, plan: &mut Plan, want: &[institutions::Institution]) -> Result<()> {
        if want.is_empty() {
            return Ok(());
        }
        let (listed, degrees) = self.institutions().await?;
        let (adds, findings) = institutions::plan(&listed, &degrees, want);
        plan.institutions = adds;
        plan.findings.extend(findings);
        Ok(())
    }

    /// Adds the per-school details (major, GPA) to `plan`: each school on
    /// the Additional Academic Info page has its own popup form, planned
    /// like any page. A school not yet added is skipped (add it first).
    pub async fn plan_school_details(
        &self,
        plan: &mut Plan,
        pages: &[Page],
        want: &[institutions::Institution],
    ) -> Result<()> {
        if want.iter().all(|i| i.details().is_empty()) {
            return Ok(());
        }
        let Some(info) = pages.iter().find(|p| p.title.trim().starts_with("Additional Academic Info")) else {
            plan.findings.push(Finding::Pending("no Additional Academic Information page to put GPAs on".into()));
            return Ok(());
        };
        let rows = page::widget_rows(&self.client.get(&info.href).await?);
        for i in want {
            let details = i.details();
            if details.is_empty() {
                continue;
            }
            let Some((_, href)) = rows.iter().find(|(name, _)| name.eq_ignore_ascii_case(&i.name)) else {
                plan.findings.push(Finding::Pending(format!("{}: not on Additional Academic Info yet", i.name)));
                continue;
            };
            let mut form = page::page(href, &self.client.get(href).await?);
            form.title = format!("{} ({})", info.title.trim(), i.name);
            let sub =
                answer::plan(std::slice::from_ref(&form), &AnswersFile { answers: details, ..Default::default() });
            plan.changes.extend(sub.changes);
            plan.findings.extend(sub.findings);
            plan.school_pages.push(form);
        }
        Ok(())
    }

    /// Every menu section, one line each (see [`coverage`]).
    pub async fn coverage(&self, file: &AnswersFile) -> Result<Vec<Finding>> {
        let (nav, issues) = self.review().await?;
        let mut entries = Vec::new();
        for n in &nav {
            let section = if n.href == "review" {
                coverage::Section::Review
            } else if n.href == "certify" {
                coverage::Section::Signature
            } else if n.is_form() {
                let mut p = page::page(&n.href, &self.client.get(&n.href).await?);
                p.title.clone_from(&n.title);
                coverage::Section::Questions(p)
            } else {
                match page::list_rows(&self.client.get(&n.href).await?) {
                    Some(rows) => coverage::Section::List {
                        rows,
                        declared: match n.href.as_str() {
                            "aca" => Some(file.institutions.len()),
                            "job" => Some(file.jobs.len()),
                            _ => None,
                        },
                    },
                    None => coverage::Section::Unknown,
                }
            };
            entries.push(coverage::Entry { title: n.title.clone(), href: n.href.clone(), section });
        }
        Ok(coverage::report(&entries, &issues, &file.excluded))
    }

    /// Adds `plan.jobs` (needs the live Professional Experience list).
    pub async fn plan_jobs(&self, plan: &mut Plan, want: &[jobs::Job]) -> Result<()> {
        if want.is_empty() {
            return Ok(());
        }
        let listed = page::jobs_listed(&self.client.get("job").await?)
            .ok_or_else(|| Error::Shape("job: not the Professional Experience page".into()))?;
        let (adds, findings) = jobs::plan(&listed, want);
        plan.jobs = adds;
        plan.findings.extend(findings);
        Ok(())
    }

    /// Posts one role, then checks Professional Experience shows one more
    /// row for its employer and dates.
    pub async fn add_job(&self, add: &jobs::JobAdd) -> Result<()> {
        let count = |rows: &[(String, String)]| {
            let label = jobs::dates_label(&add.job).unwrap_or_default();
            rows.iter().filter(|(e, d)| e.eq_ignore_ascii_case(&add.job.employer) && *d == label).count()
        };
        let before = page::jobs_listed(&self.client.get("job").await?).unwrap_or_default();
        self.client.save_form("job?id=", &add.fields).await?;
        let after = page::jobs_listed(&self.client.get("job").await?)
            .ok_or_else(|| Error::Shape("job: not the Professional Experience page".into()))?;
        if count(&after) != count(&before) + 1 {
            return Err(Error::Shape(format!(
                "adding {} at {} answered OK but Professional Experience doesn't show it (rows: {:?})",
                add.job.title, add.job.employer, after
            )));
        }
        Ok(())
    }

    /// Posts one institution with its transcript, then checks Academic
    /// History lists it.
    pub async fn add_institution(
        &self,
        add: &institutions::InstitutionAdd,
        transcript: Vec<u8>,
        file_name: &str,
    ) -> Result<()> {
        self.client.save_file("aca?id=", &add.fields, "file1", file_name, transcript).await?;
        let (listed, _) = self.institutions().await?;
        let name = add.institution.name.to_lowercase();
        if !listed.iter().any(|l| l.to_lowercase().contains(&name)) {
            return Err(Error::Shape(format!(
                "adding {} answered OK but Academic History doesn't list it (lists: {})",
                add.institution.name,
                listed.join("; ")
            )));
        }
        Ok(())
    }

    /// The Review page as findings: errors block submission.
    pub async fn audit(&self) -> Result<Vec<Finding>> {
        let (_, issues) = self.review().await?;
        Ok(issues
            .into_iter()
            .map(|i| {
                let m = format!("{}: {}: {}", self.name, i.section, i.message);
                if i.error { Finding::Violation(m) } else { Finding::Pending(m) }
            })
            .collect())
    }
}

pub struct Status {
    pub items: Vec<page::ChecklistItem>,
    pub update: Option<Update>,
}

pub struct Update {
    /// As the page says it ("June 2, 2026").
    pub posted: String,
    /// `update?cmd=stream&id=…`, relative to `/apply/`.
    pub pdf: Option<String>,
}

impl Status {
    /// Awaiting items are still to do; the latest update is worth reading.
    pub fn findings(&self, portal: &str) -> Vec<Finding> {
        let mut f: Vec<Finding> = self
            .items
            .iter()
            .filter(|i| i.status.eq_ignore_ascii_case("awaiting"))
            .map(|i| Finding::Pending(format!("{portal}: {}: {} awaited", i.list, i.detail)))
            .collect();
        if let Some(u) = &self.update {
            f.push(Finding::Note(format!("{portal}: an update was posted {} (admit -p {portal} status)", u.posted)));
        }
        f
    }
}

/// Questions answered on `before` whose answer differs on `after`, other
/// than the `touched` ones.
pub fn collateral(before: Option<&Page>, after: &Page, touched: &BTreeSet<&str>) -> Vec<String> {
    let Some(before) = before else { return Vec::new() };
    before
        .questions
        .iter()
        .filter(|q| !touched.contains(q.id.as_str()))
        .filter_map(|q| {
            let was = answer::current(q)?;
            let now = after.questions.iter().find(|a| a.id == q.id).and_then(answer::current);
            (now.as_ref() != Some(&was))
                .then(|| format!("{} {was} -> {}", q.key(), now.map_or("(blank)".to_string(), |n| n.to_string())))
        })
        .collect()
}
