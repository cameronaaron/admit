//! `admit transcripts`: every official transcript `transcript_orders.toml`
//! declares, with the status of the order that sends it, from the
//! Clearinghouse (tracked by order number + email) and Parchment (the
//! account's order list). `admit parchment login` stores the Parchment
//! sign-in; Keycloak then emails a code, asked for on the terminal.
//!
//! Only reads: ordering costs money and signs a release, and is done in
//! the browser (SUPPORT.md).

use std::collections::BTreeSet;
use std::path::Path;

use admit_domain::orders::{self, Order};
use admit_domain::{Finding, TranscriptOrdersFile};
use admit_parchment::{Endpoints, Saved};
use anyhow::{Context, Result, bail};

use crate::ctx::Ctx;
use crate::session;

/// The hosts read; tests point them at mocks.
pub struct Services {
    pub clearinghouse: String,
    pub parchment: Endpoints,
}

impl Default for Services {
    fn default() -> Self {
        Services { clearinghouse: admit_clearinghouse::BASE.into(), parchment: Endpoints::default() }
    }
}

pub async fn run(ctx: &Ctx) -> Result<()> {
    run_with(ctx, &Services::default(), &mut tty_code).await
}

pub async fn run_with(ctx: &Ctx, sv: &Services, code: &mut dyn FnMut() -> Option<String>) -> Result<()> {
    let file = ctx.profile()?.transcript_orders;
    let parchment = match uses_parchment(&file) {
        true => Some(parchment_orders(&sv.parchment, code).await),
        false => None,
    };
    let found = findings(&file, &sv.clearinghouse, parchment).await;
    print!("{}", render(&found));
    let violations = found.iter().filter(|f| f.is_violation()).count();
    if violations > 0 {
        bail!("{violations} transcript order(s) need attention");
    }
    Ok(())
}

/// Violations first, then what is pending, then the rest.
pub fn render(found: &[Finding]) -> String {
    let rank = |f: &Finding| match f {
        Finding::Violation(_) => 0,
        Finding::Pending(_) => 1,
        Finding::Note(_) => 2,
    };
    let mut sorted: Vec<&Finding> = found.iter().collect();
    sorted.sort_by_key(|f| rank(f));
    sorted.iter().map(|f| format!("{f}\n")).collect()
}

/// Reads every service a send uses. A service (or one Clearinghouse
/// order) that can't be read is an UNCHECKED violation, and the sends it
/// would have answered are left out of the check rather than reported as
/// not ordered.
/// Whether any send goes through Parchment (only then is it read: it may
/// ask for an emailed code).
pub fn uses_parchment(file: &TranscriptOrdersFile) -> bool {
    file.sends.iter().any(|s| s.via == "parchment")
}

/// `parchment` is the account's orders, read when [`uses_parchment`].
pub async fn findings(
    file: &TranscriptOrdersFile,
    clearinghouse: &str,
    parchment: Option<Result<Vec<Order>>>,
) -> Vec<Finding> {
    if file.sends.is_empty() {
        return vec![Finding::Note("transcript_orders.toml declares no transcripts to send".into())];
    }
    let mut out = Vec::new();
    let mut read = Vec::<Order>::new();
    let mut unread_ids = BTreeSet::new();
    let ids: BTreeSet<&str> = file
        .sends
        .iter()
        .filter(|s| s.via == "clearinghouse")
        .filter_map(|s| s.order.as_deref().map(str::trim))
        .collect();
    if !ids.is_empty() && file.email.trim().is_empty() {
        out.push(Finding::Violation(
            "UNCHECKED: clearinghouse: transcript_orders.toml needs `email`, the address the orders were placed with"
                .into(),
        ));
        unread_ids.extend(ids.iter().copied());
    } else {
        let ch = admit_clearinghouse::Client::new(clearinghouse);
        for id in &ids {
            match ch.track(file.email.trim(), id).await {
                Ok(o) => read.extend(o),
                Err(e) => {
                    out.push(Finding::Violation(format!("UNCHECKED: {e}")));
                    unread_ids.insert(*id);
                }
            }
        }
    }
    let mut parchment_read = true;
    match parchment {
        Some(Ok(o)) => read.extend(o),
        Some(Err(e)) => {
            out.push(Finding::Violation(format!("UNCHECKED: parchment: {e:#}")));
            parchment_read = false;
        }
        None => {}
    }
    let checked = TranscriptOrdersFile {
        email: file.email.clone(),
        sends: file
            .sends
            .iter()
            .filter(|s| match s.via.as_str() {
                "parchment" => parchment_read,
                "clearinghouse" => !s.order.as_deref().is_some_and(|id| unread_ids.contains(id.trim())),
                _ => true,
            })
            .cloned()
            .collect(),
    };
    out.extend(orders::check(&checked, &read));
    out
}

/// The account's orders on the stored session; signs in again (asking for
/// the emailed code) when it has expired and a sign-in is stored.
pub async fn parchment_orders(ep: &Endpoints, code: &mut dyn FnMut() -> Option<String>) -> Result<Vec<Order>> {
    let saved = Saved { session: session::get("parchment"), device: session::get("parchment-device") };
    let client = admit_parchment::Client::new(ep, &saved)?;
    match client.orders().await {
        Ok(o) => {
            keep(&client);
            return Ok(o);
        }
        Err(admit_parchment::Error::Expired) => {}
        Err(e) => return Err(e.into()),
    }
    let (user, password) = session::credentials("parchment")
        .context("no Parchment sign-in stored: printf '%s\\n%s\\n' EMAIL PASSWORD | admit parchment login")?;
    client.sign_in(&user, &password, code).await?;
    keep(&client);
    eprintln!("signed in to Parchment");
    Ok(client.orders().await?)
}

/// Keeps the session and Keycloak's cookies (the trusted device spares the
/// emailed code for a day); where there is no Keychain they are not kept.
fn keep(client: &admit_parchment::Client) {
    let saved = client.saved();
    for (account, value) in [("parchment", saved.session), ("parchment-device", saved.device)] {
        if let Some(v) = value
            && let Err(e) = session::put(account, &v)
        {
            eprintln!("{account} not stored ({e:#})");
        }
    }
}

/// `admit parchment login`: store the sign-in from stdin, sign in, keep
/// the session.
pub async fn login(ep: &Endpoints, code: &mut dyn FnMut() -> Option<String>) -> Result<()> {
    let (user, password) = session::set_credentials_from_stdin("parchment")?;
    let client = admit_parchment::Client::new(ep, &Saved { session: None, device: session::get("parchment-device") })?;
    client.sign_in(&user, &password, code).await?;
    keep(&client);
    println!("stored; Parchment sign-in works ({} transcript order(s))", client.orders().await?.len());
    Ok(())
}

/// The emailed code, typed on the terminal; `None` without one (CI).
pub fn tty_code() -> Option<String> {
    ask_code(terminal(Path::new("/dev/tty")))
}

fn terminal(path: &Path) -> Option<Box<dyn Terminal>> {
    let file = std::fs::OpenOptions::new().read(true).write(true).open(path).ok()?;
    Some(Box::new(file))
}

/// A terminal to prompt on (one non-generic `ask_code`, so coverage counts
/// one instantiation).
pub trait Terminal: std::io::Read + std::io::Write {}
impl<T: std::io::Read + std::io::Write> Terminal for T {}

const PROMPT: &str = "Parchment emailed a sign-in code to the account's address; enter it: ";

fn ask_code(tty: Option<Box<dyn Terminal>>) -> Option<String> {
    use std::io::BufRead;
    let mut tty = std::io::BufReader::new(tty?);
    let _ = tty.get_mut().write_all(PROMPT.as_bytes());
    let mut line = String::new();
    tty.read_line(&mut line).ok()?;
    Some(line.trim().to_string()).filter(|c| !c.is_empty())
}

#[cfg(test)]
#[path = "tests/transcripts_tests.rs"]
mod tests;
