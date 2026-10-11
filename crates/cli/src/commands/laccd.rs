//! `admit laccd`: the stored LACCD sign-in and the transcript download.

use anyhow::{Context, Result};

use crate::ctx::today;
use crate::{LaccdCmd, session};

pub fn laccd_credentials() -> Result<admit_laccd::Credentials> {
    session::laccd().context("no LACCD sign-in stored: printf '%s\\n%s\\n' USER PASSWORD | admit laccd login")
}

pub async fn run(action: &LaccdCmd) -> Result<()> {
    if matches!(action, LaccdCmd::Login) {
        session::set_credentials_from_stdin("laccd")?;
    }
    execute(action, admit_laccd::Endpoints::default(), &laccd_credentials()?).await
}

/// The download `admit sync` attempts when `[sync] fetch = "laccd"`.
pub async fn download_transcript() -> Result<Vec<u8>> {
    download_with(admit_laccd::Endpoints::default(), &laccd_credentials()?).await
}

async fn download_with(endpoints: admit_laccd::Endpoints, credentials: &admit_laccd::Credentials) -> Result<Vec<u8>> {
    admit_laccd::fetch_unofficial_transcript(endpoints, credentials).await.map_err(Into::into)
}

async fn execute(
    action: &LaccdCmd,
    endpoints: admit_laccd::Endpoints,
    credentials: &admit_laccd::Credentials,
) -> Result<()> {
    match action {
        LaccdCmd::Login => {
            let p = admit_laccd::Portal::new(endpoints);
            p.sign_in(credentials).await?;
            println!("stored; LACCD sign-in works");
        }
        LaccdCmd::Fetch { out } => {
            let pdf = admit_laccd::fetch_unofficial_transcript(endpoints, credentials).await?;
            let out = destination(out.as_deref(), today());
            std::fs::write(&out, &pdf).with_context(|| format!("writing {}", out.display()))?;
            let courses = admit_sources::parse_file("laccd", &out, &admit_sources::laccd::COLLEGES)?;
            println!("saved {} ({} bytes, {} courses; totals reconcile)", out.display(), pdf.len(), courses.len());
        }
    }
    Ok(())
}

fn destination(out: Option<&std::path::Path>, date: time::Date) -> std::path::PathBuf {
    out.map(std::path::Path::to_path_buf).unwrap_or_else(|| format!("LAC_SR_TSRPT-{date}.pdf").into())
}

#[cfg(test)]
#[path = "tests/laccd_tests.rs"]
mod tests;
