//! `admit ready` and `admit remind`: what still stands between each
//! application and submission, per program, from the same findings `audit`
//! reports.
//!
//! A blocker is any Violation or Pending finding. Notes are not, and neither
//! is a deadline that is merely coming up (that Pending is the reason to
//! finish, not something to fix); a passed deadline is. A finding belongs to
//! the program its message starts with ("Org - Program: …" or
//! "Org - Program [INCOMPLETE]: …"); every other one is portal-wide and
//! blocks all of that portal's programs. A program with no readable deadline
//! has no row, so its blockers count as portal-wide.

use std::fmt::Write as _;

use admit_domain::Finding;
use admit_liaison::audit::{Deadline, grade};
use anyhow::Result;
use time::Date;

use crate::ctx::{Ctx, PortalConfig, today};

/// One program and what blocks it alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Program {
    pub deadline: Deadline,
    pub blockers: Vec<Finding>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Readiness {
    pub portal: String,
    pub submitted: bool,
    /// Why the portal could not be read; nothing else is known then.
    pub error: Option<String>,
    /// Blockers no single program's label claims.
    pub wide: Vec<Finding>,
    /// Soonest deadline first.
    pub programs: Vec<Program>,
}

impl Readiness {
    pub fn program_ready(&self, p: &Program) -> bool {
        self.error.is_none() && self.wide.is_empty() && p.blockers.is_empty()
    }

    pub fn ready(&self) -> bool {
        self.error.is_none() && self.wide.is_empty() && self.programs.iter().all(|p| p.blockers.is_empty())
    }
}

fn about(message: &str, label: &str) -> bool {
    message.strip_prefix(label).is_some_and(|rest| rest.starts_with(':') || rest.starts_with(" ["))
}

pub fn assess(
    portal: &str,
    submitted: bool,
    mut deadlines: Vec<Deadline>,
    findings: Vec<Finding>,
    today: Date,
) -> Readiness {
    let upcoming: Vec<Finding> =
        deadlines.iter().map(|d| grade(d, today)).filter(|f| matches!(f, Finding::Pending(_))).collect();
    deadlines.sort_by(|a, b| (a.date, &a.label).cmp(&(b.date, &b.label)));
    let mut programs: Vec<Program> =
        deadlines.into_iter().map(|deadline| Program { deadline, blockers: vec![] }).collect();
    let mut wide = Vec::new();
    for f in findings.into_iter().filter(|f| !matches!(f, Finding::Note(_)) && !upcoming.contains(f)) {
        match programs.iter_mut().find(|p| about(f.message(), &p.deadline.label)) {
            Some(p) => p.blockers.push(f),
            None => wide.push(f),
        }
    }
    Readiness { portal: portal.to_string(), submitted, error: None, wide, programs }
}

fn when(date: Date, today: Date) -> String {
    let days = (date - today).whole_days();
    if days < 0 { format!("PASSED {} day(s) ago", -days) } else { format!("in {days} day(s)") }
}

fn list(out: &mut String, indent: &str, f: &[Finding]) {
    for x in f {
        let _ = writeln!(out, "{indent}{x}");
    }
}

/// The `ready` report and whether everything unsubmitted is ready.
pub fn render_ready(today: Date, profile: &[Finding], all: &[Readiness]) -> (String, bool) {
    let mut out = format!("admit ready {today}\n");
    let profile_bad: Vec<Finding> = profile.iter().filter(|f| f.is_violation()).cloned().collect();
    let mut ok = profile_bad.is_empty();
    if !profile_bad.is_empty() {
        let _ = writeln!(
            out,
            "\nprofile: {} violation(s), shared by every application (admit validate)",
            profile_bad.len()
        );
    }
    for r in all {
        if let Some(e) = &r.error {
            ok = false;
            let _ = writeln!(out, "\n{}: UNCHECKED: {e}", r.portal);
            continue;
        }
        if r.submitted {
            let _ = writeln!(out, "\n{}: SUBMITTED, {} item(s) awaited", r.portal, r.wide.len());
            list(&mut out, "  ", &r.wide);
            continue;
        }
        ok &= r.ready();
        let n = r.programs.iter().filter(|p| r.program_ready(p)).count();
        let verdict = if r.ready() { "READY".to_string() } else { "NOT READY".to_string() };
        let _ = writeln!(out, "\n{}: {verdict}, {n} of {} program(s) ready", r.portal, r.programs.len());
        if !r.wide.is_empty() {
            let _ = writeln!(out, "  every program: {} blocking", r.wide.len());
            list(&mut out, "    ", &r.wide);
        }
        for p in &r.programs {
            let d = &p.deadline;
            let state = if r.program_ready(p) { "READY".into() } else { format!("{} blocking", p.blockers.len()) };
            let _ = writeln!(out, "  {} (due {}, {}): {state}", d.label, d.shown, when(d.date, today));
            list(&mut out, "    ", &p.blockers);
        }
    }
    (out, ok)
}

/// Deadlines within `days` (or passed) whose program is not ready, and
/// portals that could not be read. Empty text when there is nothing to say.
pub fn render_remind(today: Date, days: i64, all: &[Readiness]) -> String {
    let mut out = String::new();
    let mut due: Vec<(&Readiness, &Program)> = all
        .iter()
        .filter(|r| !r.submitted)
        .flat_map(|r| r.programs.iter().map(move |p| (r, p)))
        .filter(|(r, p)| (p.deadline.date - today).whole_days() <= days && !r.program_ready(p))
        .collect();
    due.sort_by_key(|(r, p)| (p.deadline.date, r.portal.clone(), p.deadline.label.clone()));
    for (r, p) in due {
        let d = &p.deadline;
        let _ = writeln!(
            out,
            "{} ({}) {}: {} - NOT READY, {} blocking here, {} for every {} program",
            d.date,
            when(d.date, today),
            r.portal,
            d.label,
            p.blockers.len(),
            r.wide.len(),
            r.portal
        );
        list(&mut out, "  ", &p.blockers);
        list(&mut out, "  ", &r.wide);
    }
    for r in all {
        if let Some(e) = &r.error {
            let _ = writeln!(out, "{}: UNCHECKED, its deadlines are unknown: {e}", r.portal);
        }
    }
    out
}

/// The profile's findings and each portal's readiness (`-p` picks one).
async fn collect(ctx: &Ctx) -> Result<(Vec<Finding>, Vec<Readiness>)> {
    let profile = ctx.profile()?;
    let pf = super::portal::profile_findings(ctx, &profile).await;
    let portals: Vec<&PortalConfig> = match &ctx.portal_name {
        Some(_) => vec![ctx.portal_config()?],
        None => ctx.config.portals.iter().collect(),
    };
    let mut all = Vec::new();
    for pc in portals {
        let one = ctx.for_portal(&pc.name);
        all.push(match super::overview::portal(&one, pc, &profile).await {
            Ok((dates, found)) => assess(&pc.name, pc.submitted, dates, found, today()),
            Err(e) => Readiness { portal: pc.name.clone(), error: Some(format!("{e:#}")), ..Default::default() },
        });
    }
    Ok((pf, all))
}

pub async fn ready(ctx: &Ctx) -> Result<()> {
    let (profile, all) = collect(ctx).await?;
    let (text, ok) = render_ready(today(), &profile, &all);
    print!("{text}");
    if !ok {
        std::process::exit(1);
    }
    Ok(())
}

pub async fn remind(ctx: &Ctx, days: i64) -> Result<()> {
    let (_, all) = collect(ctx).await?;
    let text = render_remind(today(), days, &all);
    if text.is_empty() {
        println!("no unready application is due within {days} day(s)");
        return Ok(());
    }
    print!("{text}");
    std::process::exit(1);
}

#[cfg(test)]
#[path = "tests/ready_tests.rs"]
mod tests;
