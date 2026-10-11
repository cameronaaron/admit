//! `admit sync`: fetch the transcript, install it if it changed, apply new
//! coursework, report.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use admit_domain::Finding;
use admit_liaison::Section;
use anyhow::{Context, Result, bail};

use crate::commands::portal::apply_converging;
use crate::ctx::{Ctx, expand, today};
use crate::sync;

pub async fn run(ctx: &Ctx, if_due: bool) -> Result<()> {
    let cfg = ctx.config.sync.clone().context("admit.toml has no [sync] section")?;
    let reports = ctx.base.join(&cfg.reports);
    std::fs::create_dir_all(&reports)?;
    let stamp = today();
    let last_file = reports.join("last-sync");
    let last = std::fs::read_to_string(&last_file)
        .ok()
        .and_then(|t| time::Date::parse(t.trim(), time::macros::format_description!("[year]-[month]-[day]")).ok());
    let reason = sync::due(stamp, last, &cfg.run_on, cfg.every_weeks)?;
    if if_due && reason.is_none() {
        println!("{stamp}: not due (last sync {})", last.map(|d| d.to_string()).unwrap_or_default());
        return Ok(());
    }
    let mut report = format!("admit sync {stamp} ({})\n\n", reason.as_deref().unwrap_or("run by hand"));
    let outcome = sync_inner(ctx, &cfg, &reports, &mut report).await;
    let (title, summary) = match &outcome {
        Ok(s) => ("admit sync", s.clone()),
        Err(e) => {
            report.push_str(&format!("\nFAILED: {e:#}\n"));
            ("admit sync failed", format!("{e:#}"))
        }
    };
    let path = reports.join(format!("sync-{stamp}.txt"));
    std::fs::write(&path, &report)?;
    if outcome.is_ok() {
        std::fs::write(&last_file, format!("{stamp}\n"))?;
    }
    sync::notify(title, &format!("{summary} (report: {})", path.display()));
    println!("{report}");
    outcome.map(drop)
}

/// The transcript this run should consider: a fresh download when `fetch` is
/// set (falling back to the inbox if that fails), else the newest inbox PDF.
async fn candidate_transcript(
    cfg: &sync::SyncConfig,
    reports: &Path,
    since: Option<std::time::SystemTime>,
    report: &mut String,
) -> Result<Option<PathBuf>> {
    candidate_with(cfg, reports, since, report, crate::commands::laccd::download_transcript()).await
}

async fn candidate_with(
    cfg: &sync::SyncConfig,
    reports: &Path,
    since: Option<std::time::SystemTime>,
    report: &mut String,
    download: impl std::future::Future<Output = Result<Vec<u8>>>,
) -> Result<Option<PathBuf>> {
    match cfg.fetch.as_deref() {
        Some("laccd") => {
            let fetched = async {
                let pdf = download.await?;
                let out = reports.join(format!("{}-{}.pdf", cfg.prefix, today()));
                std::fs::write(&out, pdf)?;
                anyhow::Ok(out)
            }
            .await;
            match fetched {
                Ok(p) => {
                    report.push_str(&format!("downloaded {}\n", p.display()));
                    return Ok(Some(p));
                }
                Err(e) => report.push_str(&format!("LACCD download failed ({e:#}); checking the inbox instead\n")),
            }
        }
        Some(other) => bail!("[sync] fetch = {other:?} is not supported (supported: \"laccd\")"),
        None => {}
    }
    let found = sync::newest_in_inbox(&expand(&cfg.inbox), &cfg.prefix, since)?;
    if found.is_none() {
        report.push_str(&format!(
            "no transcript newer than the installed one in {} - download a fresh unofficial transcript \
             (LACCD Student Portal -> Academic Records -> View Unofficial Transcript) into the inbox\n",
            cfg.inbox.display()
        ));
    }
    Ok(found)
}

async fn sync_inner(ctx: &Ctx, cfg: &sync::SyncConfig, reports: &Path, report: &mut String) -> Result<String> {
    let installed = ctx.profile_dir.join(&cfg.transcript);
    let since = std::fs::metadata(&installed).and_then(|m| m.modified()).ok();
    let mut summary = Vec::new();
    let labels = &admit_sources::laccd::COLLEGES;
    match candidate_transcript(cfg, reports, since, report).await? {
        Some(new) => {
            // Install only a transcript that parses, reconciles, and says
            // something the installed one does not.
            let parsed = admit_sources::parse_file("laccd", &new, labels)
                .with_context(|| format!("{} did not parse; left the installed transcript alone", new.display()))?;
            let old = if installed.exists() { admit_sources::parse_file("laccd", &installed, labels)? } else { vec![] };
            let changes = sync::transcript_changes(&old, &parsed);
            if changes.is_empty() {
                report.push_str(&format!("transcript unchanged ({} courses)\n", parsed.len()));
                summary.push("transcript unchanged".to_string());
            } else {
                ensure_parent(&installed)?;
                if let Some(prev) = since {
                    let d = time::OffsetDateTime::from(prev).date();
                    std::fs::copy(&installed, installed.with_file_name(format!("laccd-{d}.pdf")))?;
                }
                std::fs::copy(&new, &installed)?;
                report.push_str(&format!("installed {} ({} courses); changes:\n", new.display(), parsed.len()));
                for c in &changes {
                    report.push_str(&format!("  {c}\n"));
                }
                summary.push(format!("{} transcript change(s)", changes.len()));
            }
        }
        None => summary.push("no new transcript".to_string()),
    }
    // NursingCAS sign-in is not automated (no capture of it exists), so an
    // expired cookie stops here; the transcript work above is already kept,
    // and the next daily run retries because this one is not recorded.
    let so_far = summary.join("; ");
    let portal_part = apply_coursework(ctx, report, &mut summary).await;
    portal_part.with_context(|| format!("{so_far}; NursingCAS not updated"))?;
    Ok(summary.join("; "))
}

/// Creates the directory that will hold `path`. A path with no parent is the
/// filesystem root, which already exists.
fn ensure_parent(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    Ok(())
}

/// Unattended runs never correct course codes: a mistyped-code match is a
/// heuristic, so corrections wait for `admit apply` with a person reading the
/// plan. Everything else transcript-driven (new courses, terms, institutions,
/// still-attending flags) is applied.
fn hold_corrections(p: &mut admit_liaison::Plan) {
    p.course_edits.clear();
}

async fn apply_coursework(ctx: &Ctx, report: &mut String, summary: &mut Vec<String>) -> Result<()> {
    let mut profile = ctx.profile()?;
    // Transcripts not on this machine (CI checks out no PDFs; only the LACCD
    // one is downloaded) are skipped, not reported as unreadable.
    profile.coursework.transcripts.retain(|t| {
        let here = ctx.profile_dir.join(&t.path).exists();
        if !here {
            report.push_str(&format!("skipped {} (not present here)\n", t.path));
        }
        here
    });
    let portal = ctx.portal().await?;
    let lk = portal.lookups().await?;
    let only: BTreeSet<Section> = [Section::Coursework].into();
    let mut plan = portal.plan(&profile, &ctx.profile_dir, &only, &lk).await?;
    let held = plan.course_edits.len();
    report.push_str(&plan.render());
    hold_corrections(&mut plan);
    let last = if plan.change_count() > 0 {
        crate::commands::drift::before_writing(ctx, &portal, &plan.sections()).await?;
        let again = apply_converging(ctx, &portal, &profile, &only, &lk, plan, hold_corrections).await?;
        report.push_str(&format!("\nafter apply:\n{}", again.render()));
        anyhow::ensure!(again.change_count() == 0, "{} change(s) remain after apply", again.change_count());
        summary.push("coursework updated".to_string());
        again
    } else {
        plan
    };
    if held > 0 {
        summary.push(format!("{held} course correction(s) held for `admit apply`"));
    }
    let pending = last.findings.iter().filter(|f| !matches!(f, Finding::Note(_))).count();
    if pending > 0 {
        summary.push(format!("{pending} item(s) need you - see report"));
    }
    Ok(())
}

#[cfg(test)]
#[path = "tests/sync_tests.rs"]
mod tests;
