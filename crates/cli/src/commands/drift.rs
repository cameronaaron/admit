//! `admit drift`: the Liaison responses the readers use, against the shapes
//! recorded in `shapes/<portal>.json` (`--record` rewrites it from the live
//! portal). Keys and JSON types only, never values, so the file is safe to
//! commit. Slate pages are not watched here: a changed page already shows in
//! its audit (an answer key no question has, Review errors, a section
//! UNCHECKED).

use std::path::PathBuf;

use std::collections::BTreeSet;

use admit_liaison::drift::{self, Shapes};
use admit_liaison::{Portal, Section};
use anyhow::{Context, Result, bail};

use crate::ctx::{Ctx, print_findings};
use admit_domain::Finding;

pub fn shapes_path(ctx: &Ctx, portal: &str) -> PathBuf {
    ctx.base.join("shapes").join(format!("{portal}.json"))
}

pub fn read(path: &std::path::Path) -> Result<Shapes> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("no recording at {} (admit drift --record)", path.display()))?;
    serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))
}

pub async fn run(ctx: &Ctx, record: bool) -> Result<()> {
    let pc = ctx.portal_config()?;
    if pc.kind != "liaison" {
        bail!("drift watches Liaison JSON; a Slate portal's changes show in `admit -p {} audit`", pc.name);
    }
    let path = shapes_path(ctx, &pc.name);
    let live = drift::fetch(&ctx.portal().await?.client).await?;
    if record {
        std::fs::create_dir_all(path.parent().unwrap_or(&ctx.base))?;
        std::fs::write(&path, serde_json::to_string_pretty(&live).expect("string maps encode as JSON") + "\n")?;
        let paths: usize = live.values().map(|s| s.len()).sum();
        println!("recorded {} endpoint(s), {paths} key path(s) to {}", live.len(), path.display());
        return Ok(());
    }
    let f = drift::check(&read(&path)?, &live);
    print_findings(&f);
    let v = f.iter().filter(|x| x.is_violation()).count();
    println!("{}: {} endpoint(s) compared, {v} change(s) that break readers", pc.name, live.len());
    if v > 0 {
        std::process::exit(1);
    }
    Ok(())
}

/// Before any write: the responses the written sections' bodies are built
/// from, against the recording. A field renamed or retyped under a reader
/// turns into `""`/`0` and is written back that way, so a changed shape
/// stops the run before its first write. No recording, no writes.
pub async fn before_writing(ctx: &Ctx, portal: &Portal, sections: &BTreeSet<Section>) -> Result<()> {
    let path = shapes_path(ctx, &portal.name);
    let recorded = read(&path).context("writes are checked against the recorded response shapes first")?;
    let reads: BTreeSet<&str> = sections.iter().flat_map(|s| drift::reads_for(*s)).copied().collect();
    let reads: Vec<&str> = reads.into_iter().collect();
    let live = drift::fetch_only(&portal.client, Some(&reads)).await?;
    let changed: Vec<Finding> = drift::check_only(&recorded, &live).into_iter().filter(Finding::is_violation).collect();
    if !changed.is_empty() {
        print_findings(&changed);
        bail!(
            "{} response shape(s) the writes depend on changed since {} was recorded; nothing was written. Check the readers against the portal, then `admit drift --record`",
            changed.len(),
            path.display()
        );
    }
    Ok(())
}
