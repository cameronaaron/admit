//! `admit audit` without `-p`: every portal at once. Deadlines across all applications,
//! soonest first, then what each portal still has open. A portal that can't
//! be read is a violation (UNCHECKED), never silently left out.

use std::fmt::Write as _;

use admit_domain::{Finding, validate};
use admit_liaison::audit::{Deadline, grade};
use anyhow::{Context, Result, bail};
use time::Date;

use crate::ctx::{Ctx, PortalConfig, today};

/// One application's deadline, labelled with its portal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Due {
    pub portal: String,
    pub deadline: Deadline,
}

/// One heading of the report: the profile, or a portal.
pub struct Group {
    pub name: String,
    pub findings: Vec<Finding>,
}

pub async fn run(ctx: &Ctx) -> Result<()> {
    let mut due = Vec::new();
    let profile = ctx.profile()?;
    let mut groups =
        vec![Group { name: "profile".into(), findings: super::portal::profile_findings(ctx, &profile).await }];
    for pc in &ctx.config.portals {
        let one = ctx.for_portal(&pc.name);
        let (dates, found) = match portal(&one, pc, &profile).await {
            Ok(x) => x,
            Err(e) => (vec![], vec![Finding::Violation(format!("UNCHECKED: {e:#}"))]),
        };
        due.extend(dates.into_iter().map(|deadline| Due { portal: pc.name.clone(), deadline }));
        groups.push(Group { name: pc.name.clone(), findings: found });
    }
    print!("{}", render(today(), due, &groups));
    if groups.iter().flat_map(|g| &g.findings).any(Finding::is_violation) {
        std::process::exit(1);
    }
    Ok(())
}

/// A portal's deadlines and open findings.
pub(crate) async fn portal(
    ctx: &Ctx,
    pc: &PortalConfig,
    profile: &admit_domain::Profile,
) -> Result<(Vec<Deadline>, Vec<Finding>)> {
    match pc.kind.as_str() {
        "liaison" => {
            let dates = ctx.portal().await?.deadlines().await?;
            Ok((dates, super::portal::findings(ctx, profile).await?))
        }
        "slate" => {
            let mut f = super::slate::findings(ctx, profile).await?;
            let dates = match slate_deadline(pc)? {
                Some(d) => {
                    f.push(grade(&d, today()));
                    vec![d]
                }
                None if pc.submitted => vec![],
                None => {
                    f.push(Finding::Pending(format!(
                        "{}: no deadline declared (deadline = \"YYYY-MM-DD\" under its [[portal]])",
                        pc.name
                    )));
                    vec![]
                }
            };
            Ok((dates, f))
        }
        k => bail!("unknown portal kind {k:?}"),
    }
}

/// Slate pages carry no deadline, so it is declared in `admit.toml`. A
/// submitted application has none left to meet.
fn slate_deadline(pc: &PortalConfig) -> Result<Option<Deadline>> {
    let Some(d) = pc.deadline.as_deref().filter(|_| !pc.submitted) else { return Ok(None) };
    let date = validate::parse_date(d).with_context(|| format!("{}: deadline {d:?} is not YYYY-MM-DD", pc.name))?;
    Ok(Some(Deadline { label: format!("{} application", pc.name), date, shown: d.to_string() }))
}

pub fn render(today: Date, mut due: Vec<Due>, groups: &[Group]) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "admit audit {today}: every portal (-p <name> for one, with notes)");
    due.sort_by(|a, b| {
        (a.deadline.date, &a.portal, &a.deadline.label).cmp(&(b.deadline.date, &b.portal, &b.deadline.label))
    });
    if !due.is_empty() {
        let _ = writeln!(out, "\ndeadlines");
        for d in &due {
            let days = (d.deadline.date - today).whole_days();
            let when = if days < 0 { format!("PASSED {} ago", -days) } else { format!("in {days} day(s)") };
            let _ = writeln!(out, "  {}  {:14}  {:11} {}", d.deadline.date, when, d.portal, d.deadline.label);
        }
    }
    let (mut violations, mut pending) = (0, 0);
    for g in groups {
        let v: Vec<&Finding> = g.findings.iter().filter(|f| f.is_violation()).collect();
        let p: Vec<&Finding> = g.findings.iter().filter(|f| matches!(f, Finding::Pending(_))).collect();
        let notes = g.findings.len() - v.len() - p.len();
        violations += v.len();
        pending += p.len();
        let _ = write!(out, "\n{}: {} violation(s), {} pending", g.name, v.len(), p.len());
        if notes > 0 {
            let _ = write!(out, ", {notes} note(s) (audit shows them)");
        }
        let _ = writeln!(out);
        for f in v.into_iter().chain(p) {
            let _ = writeln!(out, "  {f}");
        }
    }
    let _ = writeln!(out, "\n{violations} violation(s), {pending} pending across {} group(s)", groups.len());
    out
}

#[cfg(test)]
#[path = "tests/overview_tests.rs"]
mod tests;
