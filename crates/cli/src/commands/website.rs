//! `admit website`: the profile against the applicant's own site.

use admit_domain::{Finding, Profile};
use anyhow::{Context, Result};

use crate::ctx::{Ctx, print_findings, today};

/// The site's findings; also used by `admit audit`.
pub async fn findings(ctx: &Ctx, profile: &Profile) -> Result<Option<Vec<Finding>>> {
    let Some(w) = &ctx.config.website else { return Ok(None) };
    let html = admit_website::fetch(&w.url).await?;
    let site = admit_website::published(&w.url, &html)?;
    Ok(Some(admit_website::compare(profile, &site, today())))
}

pub async fn run(ctx: &Ctx) -> Result<()> {
    let f = findings(ctx, &ctx.profile()?).await?.context("admit.toml has no [website] url")?;
    print_findings(&f);
    let v = f.iter().filter(|x| x.is_violation()).count();
    println!("{} finding(s), {v} violation(s)", f.len());
    if v > 0 {
        std::process::exit(1);
    }
    Ok(())
}

#[cfg(test)]
#[path = "tests/website_tests.rs"]
mod tests;
