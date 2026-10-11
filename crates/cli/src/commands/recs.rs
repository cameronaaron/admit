//! `admit recs`: each program's recommendation letters, soonest deadline
//! first. NursingCAS reports counts only; who was asked is not read (no
//! capture of a requested letter exists yet).

use std::fmt::Write as _;

use admit_liaison::audit::Recs;
use anyhow::{Result, bail};
use time::Date;

use crate::ctx::{Ctx, today};

fn state(r: &Recs) -> &'static str {
    if r.received >= r.need {
        "DONE"
    } else if r.requested >= r.need {
        "WAITING"
    } else {
        "ASK"
    }
}

pub fn render(today: Date, mut rows: Vec<Recs>) -> String {
    rows.sort_by(|a, b| {
        (a.deadline.is_none(), a.deadline, &a.label).cmp(&(b.deadline.is_none(), b.deadline, &b.label))
    });
    let mut out = format!("recommendations {today}: requested/received of required\n");
    for r in &rows {
        let due = match r.deadline {
            Some(d) => format!("{d} ({} days)", (d - today).whole_days()),
            None => "submitted".into(),
        };
        let _ =
            writeln!(out, "  {:7}  {}/{} of {}  {:21}  {}", state(r), r.requested, r.received, r.need, due, r.label);
    }
    let ask: i64 = rows.iter().filter(|r| r.deadline.is_some()).map(|r| (r.need - r.requested).max(0)).sum();
    let _ = writeln!(out, "{ask} letter(s) still to request across unsubmitted programs");
    out
}

pub async fn run(ctx: &Ctx) -> Result<()> {
    let pc = ctx.portal_config()?;
    if pc.kind != "liaison" {
        bail!(
            "recs reads Liaison programs; a Slate portal's letters show on its Review page (`admit -p {} audit`)",
            pc.name
        );
    }
    let rows = admit_liaison::audit::recommendations(&ctx.portal().await?.client).await?;
    print!("{}", render(today(), rows));
    Ok(())
}

#[cfg(test)]
#[path = "tests/recs_tests.rs"]
mod tests;
