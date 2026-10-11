//! `admit.toml`, the loaded profile, and the portal session.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use admit_domain::{Finding, Profile};
use admit_liaison::{ApiError, Client, Portal, Section};
use admit_sources::har;
use anyhow::{Context, Result, bail};
use serde::Deserialize;

use crate::{Cli, session, sync};

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default = "default_profile")]
    pub profile: PathBuf,
    #[serde(rename = "portal")]
    pub portals: Vec<PortalConfig>,
    pub sync: Option<sync::SyncConfig>,
    pub website: Option<WebsiteConfig>,
}

/// `[website]`: the applicant's own site, cross-checked by `admit website`
/// and `admit audit`.
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WebsiteConfig {
    pub url: String,
}

fn default_profile() -> PathBuf {
    "profile".into()
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PortalConfig {
    pub name: String,
    pub kind: String,
    pub base_url: String,
    /// Directory of browser HAR captures; the newest cookie for this portal wins.
    pub har_dir: Option<PathBuf>,
    /// Environment variable holding a Cookie header; checked first.
    pub cookie_env: Option<String>,
    /// Slate: the application to act on (an account can hold several).
    pub application: Option<String>,
    /// Slate: the application is submitted; only its status page is read
    /// (`admit status`, `admit audit`).
    #[serde(default)]
    pub submitted: bool,
    /// Slate: the application deadline, YYYY-MM-DD (Slate pages don't carry
    /// one; Liaison programs do). Shown by `admit todo`.
    pub deadline: Option<String>,
    /// Slate: another application on the same account, kept in step with
    /// portal `same_as`: its sign-in, session and answers file are that
    /// portal's (UCLA's 2026-2027 application mirrors the 2027-2028 one).
    pub same_as: Option<String>,
}

impl PortalConfig {
    /// The name the sign-in, session and answers file are kept under.
    pub fn account(&self) -> &str {
        self.same_as.as_deref().unwrap_or(&self.name)
    }
}

fn config_base(config: &Path) -> &Path {
    config.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."))
}

pub fn expand(p: &Path) -> PathBuf {
    match p.strip_prefix("~") {
        Ok(rest) => std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default().join(rest),
        Err(_) => p.to_path_buf(),
    }
}

/// The machine's UTC offset, read once by `main` before the async runtime
/// starts: `time` refuses to read it once other threads exist (the result
/// could race a `setenv`), so read later it always fails.
static LOCAL_OFFSET: std::sync::OnceLock<time::UtcOffset> = std::sync::OnceLock::new();

pub fn init_local_offset() {
    remember_offset(time::UtcOffset::current_local_offset());
}

fn remember_offset(offset: Result<time::UtcOffset, time::error::IndeterminateOffset>) {
    let offset = offset.unwrap_or_else(|e| {
        eprintln!("local time zone unknown ({e}); dates are UTC");
        time::UtcOffset::UTC
    });
    let _ = LOCAL_OFFSET.set(offset);
}

/// Today where the applicant is. In UTC, an evening in California is
/// already tomorrow: deadline countdowns ran a day short.
pub fn today() -> time::Date {
    date_at(time::OffsetDateTime::now_utc(), offset_or(LOCAL_OFFSET.get().copied()))
}

fn offset_or(stored: Option<time::UtcOffset>) -> time::UtcOffset {
    stored.unwrap_or(time::UtcOffset::UTC)
}

fn date_at(now: time::OffsetDateTime, offset: time::UtcOffset) -> time::Date {
    now.to_offset(offset).date()
}

pub fn sections(only: &[String]) -> Result<BTreeSet<Section>> {
    only.iter().map(|s| Section::parse(s).with_context(|| format!("unknown section {s:?}"))).collect()
}

pub struct Ctx {
    pub profile_dir: PathBuf,
    pub config: Config,
    pub portal_name: Option<String>,
    pub base: PathBuf,
}

impl Ctx {
    pub fn load(cli: &Cli) -> Result<Ctx> {
        let text = std::fs::read_to_string(&cli.config).with_context(|| format!("reading {}", cli.config.display()))?;
        let config: Config = toml::from_str(&text).with_context(|| format!("parsing {}", cli.config.display()))?;
        let base = config_base(&cli.config);
        Ok(Ctx {
            profile_dir: base.join(expand(&config.profile)),
            config,
            portal_name: cli.portal.clone(),
            base: base.to_path_buf(),
        })
    }

    /// The same context acting on another portal.
    pub fn for_portal(&self, name: &str) -> Ctx {
        Ctx {
            profile_dir: self.profile_dir.clone(),
            config: self.config.clone(),
            portal_name: Some(name.to_string()),
            base: self.base.clone(),
        }
    }

    pub fn profile(&self) -> Result<Profile> {
        Ok(Profile::load(&self.profile_dir)?)
    }

    /// A `kind = "slate"` portal with its application open. Cookie sources
    /// as for Liaison; with a stored sign-in (`admit session login`), a
    /// missing or expired cookie is replaced by signing in again.
    pub async fn slate(&self) -> Result<admit_slate::Portal> {
        let pc = self.portal_config()?;
        if pc.kind != "slate" {
            bail!("portal {:?} is kind {:?}, not slate", pc.name, pc.kind);
        }
        let from_env = pc.cookie_env.as_ref().and_then(|v| std::env::var(v).ok()).filter(|c| !c.is_empty());
        let stored = match (from_env.or_else(|| session::get(pc.account())), &pc.har_dir) {
            (Some(c), _) => Some(c),
            (None, Some(dir)) => {
                Some(har::newest_cookie(&expand(dir), &format!("{}/apply/", pc.base_url.trim_end_matches('/')))?)
            }
            (None, None) => None,
        };
        let creds = session::credentials(pc.account());
        if let Some(cookie) = stored {
            let mut client = admit_slate::Client::new(&pc.base_url, cookie).with_journal(self.journal(&pc.name));
            match open_application(pc, &mut client).await {
                Ok(()) => return Ok(admit_slate::Portal { name: pc.name.clone(), client }),
                Err(e) if creds.is_some() && slate_expired(&e) => {}
                Err(e) => return Err(e),
            }
        }
        let (user, password) = creds.with_context(|| {
            format!(
                "no session for {:?}: printf '%s\\n%s\\n' EMAIL PASSWORD | admit -p {} session login",
                pc.account(),
                pc.account()
            )
        })?;
        let cookie = admit_slate::Client::login(&pc.base_url, &user, &password).await?;
        if let Err(e) = session::put(pc.account(), &cookie) {
            eprintln!("session not stored ({e:#})");
        }
        eprintln!("signed in to {}", pc.name);
        let mut client = admit_slate::Client::new(&pc.base_url, cookie).with_journal(self.journal(&pc.name));
        open_application(pc, &mut client).await?;
        Ok(admit_slate::Portal { name: pc.name.clone(), client })
    }

    pub fn portal_config(&self) -> Result<&PortalConfig> {
        Ok(match &self.portal_name {
            Some(n) => self.config.portals.iter().find(|p| &p.name == n).with_context(|| format!("no portal {n:?}"))?,
            None => self.config.portals.first().context("admit.toml lists no [[portal]]")?,
        })
    }

    /// Cookie sources, first hit wins: `cookie_env`, the Keychain, `har_dir`.
    /// With a stored sign-in, a missing or expired cookie is replaced by
    /// signing in again (and the new cookie kept in the Keychain).
    /// Where this portal's writes are journalled (`admit_domain::journal`).
    pub fn journal_path(&self, portal: &str) -> PathBuf {
        self.base.join("reports").join(format!("journal-{portal}.log"))
    }

    /// The context an apply that failed midway carries: where its writes are.
    pub fn stopped(&self, portal: &str) -> String {
        format!(
            "apply stopped partway; each write it sent is in {} (a `started` line with no `done` after it was interrupted); `admit plan` shows what is left",
            self.journal_path(portal).display()
        )
    }

    fn journal(&self, portal: &str) -> admit_domain::journal::Journal {
        admit_domain::journal::Journal::to(self.journal_path(portal))
    }

    pub async fn portal(&self) -> Result<Portal> {
        let pc = self.portal_config()?;
        if pc.kind != "liaison" {
            bail!("portal {:?} is kind {:?}; this command supports liaison portals only", pc.name, pc.kind);
        }
        let from_env = pc.cookie_env.as_ref().and_then(|v| std::env::var(v).ok()).filter(|c| !c.is_empty());
        let stored = match (from_env.or_else(|| session::get(&pc.name)), &pc.har_dir) {
            (Some(c), _) => Some(c),
            (None, Some(dir)) => Some(har::newest_cookie(&expand(dir), &Client::api_prefix(&pc.base_url))?),
            (None, None) => None,
        };
        let creds = session::credentials(&pc.name);
        if let Some(cookie) = stored {
            let portal = Portal {
                name: pc.name.clone(),
                client: Client::new(&pc.base_url, cookie).with_journal(self.journal(&pc.name)),
            };
            match (&creds, portal.client.get("/navigation").await) {
                (_, Ok(_)) | (None, Err(ApiError::Expired { .. })) => return Ok(portal),
                (Some(_), Err(ApiError::Expired { .. })) => {}
                (_, Err(e)) => return Err(e.into()),
            }
        }
        let (user, password) = creds.with_context(|| {
            format!("no session for {:?}: printf '%s\\n%s\\n' EMAIL PASSWORD | admit session login", pc.name)
        })?;
        let cookie = Client::login(&pc.base_url, &user, &password).await?;
        // Keeping the cookie saves a sign-in next time; where there is no
        // Keychain (CI) it is simply not kept.
        if let Err(e) = session::put(&pc.name, &cookie) {
            eprintln!("session not stored ({e:#})");
        }
        eprintln!("signed in to {} again (session had expired)", pc.name);
        Ok(Portal {
            name: pc.name.clone(),
            client: Client::new(&pc.base_url, cookie).with_journal(self.journal(&pc.name)),
        })
    }
}

fn slate_expired(err: &anyhow::Error) -> bool {
    matches!(err.downcast_ref::<admit_slate::Error>(), Some(admit_slate::Error::Expired { .. }))
}

/// Opens the portal's `application`, or its only one. Slate pages act on
/// whichever application the session has open, and an account can hold
/// several (one per cycle or program), so this never guesses.
async fn open_application(pc: &PortalConfig, client: &mut admit_slate::Client) -> Result<()> {
    if pc.submitted {
        // Nothing to open: after submission `/apply/review` redirects to
        // `/apply/status`. Reading it proves the session.
        client.get("status").await?;
        return Ok(());
    }
    let id = match &pc.application {
        Some(id) => id.clone(),
        None => {
            let apps = admit_slate::page::applications(&client.get("").await?);
            match apps.as_slice() {
                [one] => one.id.clone(),
                _ => {
                    let list: Vec<String> =
                        apps.iter().map(|a| format!("  {}  {} ({})", a.id, a.name, a.status)).collect();
                    bail!(
                        "{} has {} applications; set application = \"<id>\" under its [[portal]] in admit.toml:\n{}",
                        pc.name,
                        apps.len(),
                        list.join("\n")
                    );
                }
            }
        }
    };
    client.open(&id).await?;
    Ok(())
}

pub fn print_findings(f: &[Finding]) {
    for x in f {
        println!("{x}");
    }
}

/// What an apply's last plan proves. No changes left means the profile is
/// on the portal; it does not mean the selected sections are right: a
/// violation in that plan (an answer naming no question, a choice that
/// would drop another program) is something the apply could not write, so
/// the run fails instead of calling itself clean. `applied` says whether
/// anything was written this run.
pub fn settle(applied: bool, plan_findings: &[Finding]) -> anyhow::Result<()> {
    let done = if applied { "applied" } else { "nothing to apply" };
    let open: Vec<&Finding> = plan_findings.iter().filter(|f| f.is_violation()).collect();
    if open.is_empty() {
        println!("{done}; no changes remain");
        return Ok(());
    }
    if applied {
        for f in &open {
            println!("{f}");
        }
    }
    anyhow::bail!("{done}; no changes remain, but {} violation(s) in the selected sections are unresolved", open.len())
}

#[cfg(test)]
#[path = "tests/ctx_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "tests/ctx_boundaries.rs"]
mod boundaries;
