//! The profile ↔ portal commands: validate, plan, apply, pull, audit, session.

use std::collections::BTreeSet;
use std::path::Path;

use admit_domain::{Finding, Profile, validate};
use admit_liaison::{ApiError, Client, Plan, Portal, Section};
use anyhow::{Context, Result, bail};

use crate::ctx::{Ctx, print_findings, sections, settle, today};
use crate::{SessionCmd, session};

pub fn transcript(file: &Path, parser: &str) -> Result<()> {
    let labels: &[&str] = match parser {
        "laccd" => &admit_sources::laccd::COLLEGES,
        "conncoll" => &["Trinity Christian College", "Calif St Univ Northridg", "UMASS Amherst"],
        _ => &[],
    };
    let courses = admit_sources::parse_file(parser, file, labels)?;
    for c in &courses {
        println!(
            "{} {:6} | {:26} | {:14} | {:44} | {:>5} | {:3}{}",
            c.year,
            c.season,
            c.college,
            c.code,
            c.title,
            c.credits,
            if c.grade.is_empty() { "-" } else { &c.grade },
            if c.noncredit { " (noncredit)" } else { "" }
        );
    }
    println!("{} courses; totals reconcile with the transcript", courses.len());
    Ok(())
}

pub fn validate(ctx: &Ctx) -> Result<()> {
    let f = validate::validate(&ctx.profile()?, today());
    print_findings(&f);
    println!("{} finding(s), {} violation(s)", f.len(), f.iter().filter(|x| x.is_violation()).count());
    if f.iter().any(Finding::is_violation) {
        std::process::exit(1);
    }
    Ok(())
}

pub async fn plan(ctx: &Ctx, only: &[String]) -> Result<()> {
    let portal = ctx.portal().await?;
    let lk = portal.lookups().await?;
    let plan = portal.plan(&ctx.profile()?, &ctx.profile_dir, &sections(only)?, &lk).await?;
    print!("{}", plan.render());
    println!("{} change(s) to apply", plan.change_count());
    Ok(())
}

/// The profile reduced to what an `apply --only` touches, so a problem in a
/// section that isn't being written (overlapping experience hours) doesn't
/// block correcting another (a course code). Education stays alongside high
/// school: the cross-check of degree dates against the diploma needs both.
pub fn scoped(p: &Profile, only: &BTreeSet<Section>) -> Profile {
    if only.is_empty() {
        return p.clone();
    }
    let on = |s: Section| only.contains(&s);
    Profile {
        achievements: if on(Section::Achievements) { p.achievements.clone() } else { Default::default() },
        licenses: if on(Section::Licenses) { p.licenses.clone() } else { Default::default() },
        experiences: if on(Section::Experiences) { p.experiences.clone() } else { Default::default() },
        education: if on(Section::Education) || on(Section::HighSchool) {
            p.education.clone()
        } else {
            Default::default()
        },
        coursework: if on(Section::Coursework) { p.coursework.clone() } else { Default::default() },
        high_school: if on(Section::HighSchool) { p.high_school.clone() } else { Default::default() },
        personal: p.personal.clone(),
        prerequisites: p.prerequisites.clone(),
        questions: p.questions.clone(),
        documents: p.documents.clone(),
        references: if on(Section::References) { p.references.clone() } else { Default::default() },
        // No portal section writes it.
        transcript_orders: Default::default(),
    }
}

pub async fn apply(ctx: &Ctx, only: &[String], yes: bool) -> Result<()> {
    let profile = ctx.profile()?;
    let only = sections(only)?;
    let blocking: Vec<Finding> =
        validate::validate(&scoped(&profile, &only), today()).into_iter().filter(Finding::is_violation).collect();
    if !blocking.is_empty() {
        print_findings(&blocking);
        bail!("fix the profile violations above before applying (admit validate)");
    }
    // A partial diff cannot prove convergence. Check all selected sources
    // before any writes, while leaving unrelated --only sections independent.
    if only.is_empty() || only.contains(&Section::Coursework) {
        for src in &profile.coursework.transcripts {
            admit_liaison::coursework::read(src, &ctx.profile_dir).map_err(anyhow::Error::msg)?;
        }
    }
    let portal = ctx.portal().await?;
    let lk = portal.lookups().await?;
    let plan = portal.plan(&profile, &ctx.profile_dir, &only, &lk).await?;
    print!("{}", plan.render());
    if plan.change_count() == 0 {
        return settle(false, &plan.findings);
    }
    if !yes {
        bail!("review the {} change(s) above, then re-run with --yes", plan.change_count());
    }
    // Coursework covers the second pass for new institutions too.
    crate::commands::drift::before_writing(ctx, &portal, &plan.sections()).await?;
    let again = apply_converging(ctx, &portal, &profile, &only, &lk, plan, |_| {})
        .await
        .with_context(|| ctx.stopped(&portal.name))?;
    if again.change_count() != 0 {
        print!("{}", again.render());
        bail!("applied, but {} change(s) remain: the portal did not converge", again.change_count());
    }
    settle(true, &again.findings)
}

pub async fn pull(ctx: &Ctx, force: bool) -> Result<()> {
    let portal = ctx.portal().await?;
    let lk = portal.lookups().await?;
    let live = portal.pull(&lk).await?;
    std::fs::create_dir_all(&ctx.profile_dir)?;
    let files = [
        ("high_school.toml", toml::to_string_pretty(&live.high_school)),
        ("achievements.toml", toml::to_string_pretty(&live.achievements)),
        ("licenses.toml", toml::to_string_pretty(&live.licenses)),
        ("experiences.toml", toml::to_string_pretty(&live.experiences)),
        ("education.toml", toml::to_string_pretty(&live.education)),
        ("questions.toml", toml::to_string_pretty(&live.questions)),
    ];
    for (name, body) in files {
        // Infallible for these types: the encoder rejects only non-string
        // keys, a `None` inside a sequence and integers past `i64`, and the
        // profile types have none of them.
        let body = body.expect("profile files encode as TOML");
        let path = ctx.profile_dir.join(name);
        if path.exists() && !force {
            println!("kept {} (exists; --force to overwrite)", path.display());
            continue;
        }
        std::fs::write(
            &path,
            format!("# Pulled from {} by `admit pull`. This file is the source of truth now.\n\n{body}", portal.name),
        )?;
        println!("wrote {}", path.display());
    }
    Ok(())
}

/// Programs with prerequisites that `prerequisites.toml` doesn't rule yet,
/// as entries to append: every requirement, with the courses already
/// assigned on the portal.
pub async fn prereqs(ctx: &Ctx) -> Result<()> {
    let profile = ctx.profile()?;
    let portal = ctx.portal().await?;
    let (live, _) = admit_liaison::prereqs::live(&portal.client).await?;
    let colleges = profile.coursework.colleges();
    let text = admit_liaison::prereqs::to_toml(&live, &portal.name, &colleges, &profile.prerequisites.programs);
    if text.is_empty() {
        println!("every program with prerequisites on {} is ruled in prerequisites.toml", portal.name);
    } else {
        print!("# Append to profile/prerequisites.toml. Courses already assigned on the portal are filled in.\n{text}");
    }
    Ok(())
}

/// Each program's own questions, for filling in `questions.toml`.
pub async fn questions(ctx: &Ctx, open: bool) -> Result<()> {
    let portal = ctx.portal().await?;
    let live = admit_liaison::questions::live(&portal.client).await?;
    let text = admit_liaison::questions::listing(&live, open);
    if text.is_empty() {
        println!("no {}program questions on {}", if open { "unanswered required " } else { "" }, portal.name);
    } else {
        print!("# questions.toml: [[program]] name/portal, then [program.answers] \"<id>\" = \"<choice text>\"{text}");
    }
    Ok(())
}

/// Each program's document slots, for filling in `documents.toml`.
pub async fn documents(ctx: &Ctx, open: bool) -> Result<()> {
    let portal = ctx.portal().await?;
    let live = admit_liaison::documents::live(&portal.client).await?;
    let text = admit_liaison::documents::listing(&live, open);
    if text.is_empty() {
        println!("no {}program documents on {}", if open { "missing required " } else { "" }, portal.name);
    } else {
        print!(
            "# documents.toml: [[program]] name/portal, then [program.files] \"<id>\" = \"<path, from the profile directory>\"{text}"
        );
    }
    Ok(())
}

/// Every course on the portal with its subject (the category NursingCAS
/// computes science and other GPAs from).
pub async fn courses(ctx: &Ctx) -> Result<()> {
    let portal = ctx.portal().await?;
    for col in admit_liaison::coursework::live(&portal.client).await? {
        for t in &col.terms {
            for x in t["courses"].as_array().map(Vec::as_slice).unwrap_or_default() {
                let str_of = |v: &serde_json::Value| v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string());
                println!(
                    "{} | {} {} | {} | {} | {} | {}{}",
                    col.name,
                    str_of(&t["term"]),
                    str_of(&t["year"]),
                    str_of(&x["code"]),
                    str_of(&x["title"]),
                    str_of(&x["credits"]),
                    str_of(&x["subject"]),
                    if x["isScience"].as_bool() == Some(true) { " (science)" } else { "" }
                );
            }
        }
    }
    Ok(())
}

pub async fn audit(ctx: &Ctx) -> Result<()> {
    let profile = ctx.profile()?;
    let mut f = profile_findings(ctx, &profile).await;
    f.extend(findings(ctx, &profile).await?);
    print_findings(&f);
    let v = f.iter().filter(|x| x.is_violation()).count();
    println!("{} finding(s), {v} violation(s)", f.len());
    if v > 0 {
        std::process::exit(1);
    }
    Ok(())
}

/// The portal-independent checks: the profile itself and the website.
/// Audits load the profile once and pass it to every check, so one report
/// never mixes two versions of a file edited mid-run.
pub async fn profile_findings(ctx: &Ctx, profile: &Profile) -> Vec<Finding> {
    let mut f = validate::validate(profile, today());
    for c in crate::prerequisites::unknown_colleges(profile) {
        f.push(Finding::Violation(format!("prerequisites.toml: no transcript in coursework.toml is labelled {c:?}")));
    }
    f.extend(crate::prerequisites::findings(profile, &ctx.profile_dir, None, today()));
    match crate::commands::website::findings(ctx, profile).await {
        Ok(site) => f.extend(site.into_iter().flatten()),
        Err(e) => f.push(Finding::Pending(format!("website not checked: {e:#}"))),
    }
    f
}

/// The Liaison portal's own audit plus what the profile has that it lacks.
pub async fn findings(ctx: &Ctx, profile: &Profile) -> Result<Vec<Finding>> {
    let portal = ctx.portal().await?;
    let mut f = portal.audit(profile, today()).await?;
    f.extend(crate::prerequisites::findings(profile, &ctx.profile_dir, Some(&portal.name), today()));
    let lk = portal.lookups().await?;
    // A section that can't be read is reported, not allowed to hide the rest.
    match admit_liaison::personal::read(&portal.client, &lk).await {
        Ok(stated) => f.extend(validate::personal::compare(&profile.personal, &portal.name, &stated)),
        Err(e) => f.push(Finding::Violation(format!("{}: personal details UNCHECKED: {e}", portal.name))),
    }
    let plan = portal.plan(profile, &ctx.profile_dir, &BTreeSet::new(), &lk).await?;
    f.extend(plan.findings.iter().cloned());
    if plan.change_count() > 0 {
        f.push(Finding::Pending(format!("{} profile change(s) not yet applied (admit plan)", plan.change_count())));
    }
    Ok(f)
}

pub async fn session(ctx: &Ctx, action: &SessionCmd) -> Result<()> {
    let pc = ctx.portal_config()?;
    let name = pc.name.clone();
    let base = pc.base_url.clone();
    match action {
        SessionCmd::Set => {
            session::set_from_stdin(&name)?;
            check_session(ctx).await?;
        }
        SessionCmd::Login => {
            session::set_credentials_from_stdin(&name)?;
            let (user, password) = session::credentials(&name).context("credentials were not stored")?;
            session::put(&name, &Client::login(&base, &user, &password).await?)?;
            check_session(ctx).await?;
        }
        SessionCmd::Check => check_session(ctx).await?,
        SessionCmd::Clear => {
            session::clear(&name)?;
            println!("forgot the {name} session");
        }
    }
    Ok(())
}

/// Applies `plan` (its sections already checked by `drift::before_writing`); when it added institutions, plans and applies once more so
/// their coursework (unplannable until they exist) goes in too. `hold`
/// removes from every re-plan what the caller won't apply (unattended sync
/// holds course corrections).
pub async fn apply_converging(
    ctx: &Ctx,
    portal: &Portal,
    profile: &Profile,
    only: &BTreeSet<Section>,
    lk: &admit_liaison::lookup::Lookups,
    plan: Plan,
    hold: impl Fn(&mut Plan),
) -> Result<Plan> {
    let dir = &ctx.profile_dir;
    portal.apply(&plan, lk).await?;
    let mut again = portal.plan(profile, dir, only, lk).await?;
    hold(&mut again);
    if !plan.new_colleges.is_empty() && again.change_count() > 0 && again.new_colleges.is_empty() {
        print!("second pass for the new institutions:\n{}", again.render());
        portal.apply(&again, lk).await?;
        again = portal.plan(profile, dir, only, lk).await?;
        hold(&mut again);
    }
    Ok(again)
}

async fn check_session(ctx: &Ctx) -> Result<()> {
    let portal = ctx.portal().await?;
    match portal.client.get("/navigation").await {
        Ok(_) => {
            println!("{} session works", portal.name);
            Ok(())
        }
        Err(e @ ApiError::Expired { .. }) => bail!("{e}"),
        Err(e) => Err(e.into()),
    }
}

#[cfg(test)]
#[path = "tests/portal_tests.rs"]
mod tests;
