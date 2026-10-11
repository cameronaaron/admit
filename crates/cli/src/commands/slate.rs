//! plan / apply / pull / audit / session for a `kind = "slate"` portal. The
//! declared state is `profile/slate/<portal>.toml` (question answers); the
//! rest of the profile isn't written to Slate (see `admit_slate`).

use std::path::{Path, PathBuf};

use admit_domain::Finding;
use admit_slate::{AnswersFile, Error, answer};
use anyhow::{Context, Result, bail};

use crate::ctx::{Ctx, print_findings, settle, today};
use crate::{SessionCmd, session};

fn answers_path(ctx: &Ctx, portal: &str) -> PathBuf {
    ctx.profile_dir.join("slate").join(format!("{portal}.toml"))
}

fn answers(ctx: &Ctx) -> Result<AnswersFile> {
    let path = answers_path(ctx, ctx.portal_config()?.account());
    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("reading {} (create it with `admit -p <portal> pull`)", path.display()))?;
    toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))
}

/// Plan, apply and pull edit an application; a submitted one can't be.
fn editable(ctx: &Ctx) -> Result<&crate::ctx::PortalConfig> {
    let pc = ctx.portal_config()?;
    if pc.submitted {
        bail!("{} is submitted (submitted = true): only `admit -p {} status` and `audit` apply", pc.name, pc.name);
    }
    Ok(pc)
}

pub async fn plan(ctx: &Ctx) -> Result<()> {
    editable(ctx)?;
    let portal = ctx.slate().await?;
    let file = answers(ctx)?;
    let pages = portal.pages().await?;
    let mut plan = answer::plan(&pages, &file);
    portal.plan_institutions(&mut plan, &file.institutions).await?;
    portal.plan_school_details(&mut plan, &pages, &file.institutions).await?;
    portal.plan_jobs(&mut plan, &file.jobs).await?;
    print!("{}", plan.render());
    println!("{} change(s) to apply", plan.count());
    Ok(())
}

pub async fn apply(ctx: &Ctx, yes: bool, reveal: bool) -> Result<()> {
    editable(ctx)?;
    let portal = ctx.slate().await?;
    let want = answers(ctx)?;
    let mut pages = portal.pages().await?;
    let mut plan = full_plan(&portal, &pages, &want).await?;
    // Transcripts are read before anything is written.
    let mut transcripts = Vec::new();
    for add in &plan.institutions {
        let path = ctx.profile_dir.join(&add.institution.transcript);
        let bytes = std::fs::read(&path).with_context(|| format!("reading transcript {}", path.display()))?;
        let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        transcripts.push((bytes, name));
    }
    print!("{}", plan.render());
    if blocked(&plan, reveal) {
        bail!("fix the answers above before applying");
    }
    if plan.count() == 0 {
        if !plan.unplaced.is_empty() {
            bail!("{} answer(s) have no question, and nothing left to save could reveal one", plan.unplaced.len());
        }
        // `blocked` above already refused any violation an unplaced answer
        // doesn't account for, and an unplaced answer bailed: none remain.
        println!("nothing to apply; no changes remain");
        return still_open(ctx, &portal, &want).await;
    }
    if !yes {
        bail!("review the {} change(s) above, then re-run with --yes", plan.count());
    }
    let mut transcripts = transcripts.into_iter();
    // One round normally; with --reveal, again while a save showed a page
    // that places more answers.
    for round in 1..=REVEAL_ROUNDS {
        portal.apply(&plan, &pages).await.with_context(|| ctx.stopped(&portal.name))?;
        for (add, (bytes, name)) in plan.institutions.iter().zip(transcripts.by_ref()) {
            portal.add_institution(add, bytes, &name).await.with_context(|| ctx.stopped(&portal.name))?;
            println!("  added {} ({name})", add.institution.name);
        }
        for add in &plan.jobs {
            portal.add_job(add).await.with_context(|| ctx.stopped(&portal.name))?;
            println!("  added {} - {}", add.job.employer, add.job.title);
        }
        pages = portal.pages().await?;
        let again = full_plan(&portal, &pages, &want).await?;
        let revealed = again.unplaced.len() < plan.unplaced.len();
        if !(reveal && revealed && again.count() != 0 && round < REVEAL_ROUNDS) || blocked(&again, reveal) {
            plan = again;
            break;
        }
        print!(
            "revealed {} question(s); applying again:\n{}",
            plan.unplaced.len() - again.unplaced.len(),
            again.render()
        );
        plan = again;
    }
    if plan.count() != 0 {
        print!("{}", plan.render());
        bail!("applied, but {} change(s) remain: the portal did not converge", plan.count());
    }
    if !plan.unplaced.is_empty() {
        bail!("applied, but {} answer(s) still have no question: {}", plan.unplaced.len(), plan.unplaced.join(", "));
    }
    // What is still open is printed either way; a violation still fails the run.
    let open = still_open(ctx, &portal, &want).await;
    settle(true, &plan.findings).and(open)
}

/// How many times `apply --reveal` saves before it stops looking for new
/// pages (UCLA needs two: the program choice, then its pages).
const REVEAL_ROUNDS: usize = 3;

/// A violation stops the run, except, with `--reveal`, an unplaced answer.
fn blocked(plan: &answer::Plan, reveal: bool) -> bool {
    let violations = plan.findings.iter().filter(|f| f.is_violation()).count();
    violations > if reveal { plan.unplaced.len() } else { 0 }
}

async fn full_plan(
    portal: &admit_slate::Portal,
    pages: &[admit_slate::page::Page],
    want: &AnswersFile,
) -> Result<answer::Plan> {
    let mut plan = answer::plan(pages, want);
    portal.plan_institutions(&mut plan, &want.institutions).await?;
    portal.plan_school_details(&mut plan, pages, &want.institutions).await?;
    portal.plan_jobs(&mut plan, &want.jobs).await?;
    Ok(plan)
}

/// What is still left after an apply, every time: a clean re-plan only
/// means the answers file is on the portal, not that the application is
/// complete (see `admit_slate::coverage`).
async fn still_open(ctx: &Ctx, portal: &admit_slate::Portal, want: &AnswersFile) -> Result<()> {
    let mut left = portal.coverage(want).await?;
    left.extend(admit_slate::coverage::profile_gaps(&ctx.profile()?, want));
    let open: Vec<&Finding> = left.iter().filter(|f| !matches!(f, Finding::Note(_))).collect();
    if open.is_empty() {
        println!("every section accounted for");
    } else {
        println!("still open ({}):", open.len());
        for f in open {
            println!("  {f}");
        }
    }
    Ok(())
}

pub async fn pull(ctx: &Ctx, force: bool) -> Result<()> {
    let pc = editable(ctx)?;
    if let Some(of) = &pc.same_as {
        bail!("{} follows {of}'s answers file (same_as); pull from {of}", pc.name);
    }
    let portal = ctx.slate().await?;
    let path = answers_path(ctx, &portal.name);
    if path.exists() && !force {
        println!("kept {} (exists; --force to overwrite)", path.display());
        return Ok(());
    }
    let header = format!(
        "# {} answers, pulled by `admit -p {} pull`; this file is the source of truth now.\n\
         # Keys are the questions' export keys (or ids). Values are what the page shows:\n\
         # option text (a list for checkboxes), YYYY-MM-DD for dates. Uploads, addresses,\n\
         # the SSN, institutions, employers and tests are not kept here.",
        portal.name, portal.name
    );
    let text = answer::to_toml(&portal.pages().await?, &header);
    ensure_parent(&path)?;
    std::fs::write(&path, text)?;
    println!("wrote {}", path.display());
    Ok(())
}

pub async fn audit(ctx: &Ctx) -> Result<()> {
    let profile = ctx.profile()?;
    let mut f = crate::commands::portal::profile_findings(ctx, &profile).await;
    f.extend(findings(ctx, &profile).await?);
    report(f)
}

/// The portal's side of `audit`: Review (or, once submitted, the status
/// checklists), per-section coverage, and answers not yet applied.
pub async fn findings(ctx: &Ctx, profile: &admit_domain::Profile) -> Result<Vec<Finding>> {
    let pc = ctx.portal_config()?;
    let portal = ctx.slate().await?;
    let mut f = crate::prerequisites::findings(profile, &ctx.profile_dir, Some(pc.account()), today());
    if pc.submitted {
        f.extend(portal.status().await?.findings(&portal.name));
        return Ok(f);
    }
    f.extend(portal.audit().await?);
    // Completeness is per section, never inferred from Review alone: every
    // menu entry gets a line, and every profile role/college must be on the
    // portal or excluded with a reason.
    let file = answers(ctx)?;
    f.extend(portal.coverage(&file).await?);
    f.extend(admit_slate::coverage::profile_gaps(profile, &file));
    let stated = admit_slate::personal::stated(&file);
    f.extend(admit_domain::validate::personal::compare(&profile.personal, &portal.name, &stated));
    let pages = portal.pages().await?;
    let mut plan = answer::plan(&pages, &file);
    portal.plan_institutions(&mut plan, &file.institutions).await?;
    portal.plan_school_details(&mut plan, &pages, &file.institutions).await?;
    portal.plan_jobs(&mut plan, &file.jobs).await?;
    let pending = plan.count();
    f.extend(plan.findings);
    if pending != 0 {
        f.push(Finding::Pending(format!("{pending} answer(s) not yet applied (admit plan)")));
    }
    Ok(f)
}

fn report(f: Vec<Finding>) -> Result<()> {
    print_findings(&f);
    let v = f.iter().filter(|x| x.is_violation()).count();
    println!("{} finding(s), {v} violation(s)", f.len());
    if v > 0 {
        std::process::exit(1);
    }
    Ok(())
}

/// The status page's checklists; the latest update letter is saved to
/// `reports/<portal>-update-<posted>.pdf`.
pub async fn status(ctx: &Ctx) -> Result<()> {
    let portal = ctx.slate().await?;
    let st = portal.status().await?;
    let mut list = "";
    for i in &st.items {
        if i.list != list {
            list = &i.list;
            println!("{list}");
        }
        println!("  {:9} {:10} {}", i.status, i.date, i.detail);
    }
    match &st.update {
        Some(u) => {
            println!("update posted {}", u.posted);
            if let Some(pdf) = &u.pdf {
                let dir = ctx.base.join("reports");
                std::fs::create_dir_all(&dir)?;
                let slug: String = u.posted.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
                let path = dir.join(format!("{}-update-{slug}.pdf", portal.name));
                std::fs::write(&path, portal.client.get_bytes(pdf).await?)?;
                println!("saved {}", path.display());
            }
        }
        None => println!("no update posted"),
    }
    Ok(())
}

pub async fn session(ctx: &Ctx, action: &SessionCmd) -> Result<()> {
    let pc = ctx.portal_config()?;
    let name = pc.account().to_string();
    let base = pc.base_url.clone();
    match action {
        SessionCmd::Set => {
            session::set_from_stdin(&name)?;
            check(ctx, pc.submitted).await
        }
        SessionCmd::Login => {
            session::set_credentials_from_stdin(&name)?;
            let (user, password) = session::credentials(&name).context("credentials were not stored")?;
            session::put(&name, &admit_slate::Client::login(&base, &user, &password).await?)?;
            check(ctx, pc.submitted).await
        }
        SessionCmd::Check => check(ctx, pc.submitted).await,
        SessionCmd::Clear => {
            session::clear(&name)?;
            println!("forgot the {name} session");
            Ok(())
        }
    }
}

fn ensure_parent(path: &Path) -> Result<()> {
    let Some(parent) = path.parent() else {
        bail!("answers path has a parent");
    };
    std::fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))
}

async fn check(ctx: &Ctx, submitted: bool) -> Result<()> {
    let portal = ctx.slate().await?;
    if submitted {
        let st = portal.status().await?;
        println!("{} session works: {} checklist item(s) on the status page", portal.name, st.items.len());
        return Ok(());
    }
    for a in admit_slate::page::applications(&portal.client.get("").await?) {
        println!("  {}  {} ({})", a.id, a.name, a.status);
    }
    match portal.review().await {
        Ok((nav, issues)) => {
            println!(
                "{} session works: {} sections, {} blocking error(s) on Review",
                portal.name,
                nav.len(),
                issues.iter().filter(|i| i.error).count()
            );
            Ok(())
        }
        Err(e @ Error::Expired { .. }) => bail!("{e}"),
        Err(e) => Err(e.into()),
    }
}

/// What to write in the answers file: each writable question's key, label,
/// current answer and (for choices) what it accepts.
pub async fn questions(ctx: &Ctx, open: bool) -> Result<()> {
    editable(ctx)?;
    let portal = ctx.slate().await?;
    for p in portal.pages().await? {
        let qs: Vec<_> = p
            .questions
            .iter()
            .filter(|q| q.writable())
            .map(|q| (q, answer::current(q)))
            .filter(|(_, a)| !open || a.is_none())
            .collect();
        if qs.is_empty() {
            continue;
        }
        println!("# {} ({})", p.title, p.href);
        for (q, a) in qs {
            println!(
                "{}{} = {}   # {}",
                if q.required { "* " } else { "  " },
                q.key(),
                a.map_or("(blank)".to_string(), |a| a.to_string()),
                q.label.chars().take(120).collect::<String>()
            );
            let choices: Vec<&str> = q
                .inputs
                .iter()
                .filter(|i| i.name == format!("form_{}", q.id))
                .flat_map(|i| i.choices.iter().map(|c| c.text.as_str()))
                .collect();
            if !choices.is_empty() && q.kind != admit_slate::page::Kind::Date {
                println!("      choices: {}", choices.join(" | "));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "tests/slate_tests.rs"]
mod tests;
