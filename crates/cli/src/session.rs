//! Portal session cookies in the macOS login Keychain (service `admit`,
//! account = portal name), so nothing on disk holds them.
//!
//! The cookie is the browser's: DevTools → Network → any request to the
//! portal's `/applicant/api/` → copy the `Cookie` request header, then
//! `pbpaste | admit session set`. Liaison sessions last hours, not days, so
//! with a stored sign-in (`admit session login`) an expired cookie is
//! replaced by signing in again.

use std::io::Read;
use std::process::Command;

use anyhow::{Context, Result, bail};

const SERVICE: &str = "admit";

pub fn get(portal: &str) -> Option<String> {
    let out =
        Command::new("security").args(["find-generic-password", "-s", SERVICE, "-a", portal, "-w"]).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string()).filter(|c| !c.is_empty())
}

/// Reads a Cookie header from stdin and stores it.
pub fn set_from_stdin(portal: &str) -> Result<String> {
    cookie_from_reader(portal, std::io::stdin())
}

fn cookie_from_reader(portal: &str, mut reader: impl Read) -> Result<String> {
    let mut raw = String::new();
    reader.read_to_string(&mut raw).context("reading the cookie from stdin")?;
    let cookie = raw.trim().trim_start_matches("Cookie:").trim_start_matches("cookie:").trim().to_string();
    if !cookie.contains('=') {
        bail!("that doesn't look like a Cookie header (expected name=value; name=value)");
    }
    put(portal, &cookie)?;
    Ok(cookie)
}

pub fn clear(portal: &str) -> Result<()> {
    let st = Command::new("security").args(["delete-generic-password", "-s", SERVICE, "-a", portal]).status()?;
    if !st.success() {
        bail!("no stored session for {portal}");
    }
    Ok(())
}

/// Stores `value` under `account`.
pub fn put(account: &str, value: &str) -> Result<()> {
    // -U updates an existing item. The value passes as an argument: briefly
    // visible to `ps` for this user, which is acceptable on a personal Mac.
    let st = Command::new("security")
        .args(["add-generic-password", "-U", "-s", SERVICE, "-a", account, "-w", value])
        .status()
        .context("running `security`")?;
    if !st.success() {
        bail!("`security add-generic-password` failed for {account}");
    }
    Ok(())
}

/// A sign-in kept as two Keychain items, `<name>-user` and `<name>-password`
/// (`nursingcas-…` for a portal, `laccd-…` for the LACCD student portal), so
/// unattended runs can sign in when a session has expired.
///
/// Environment variables `<NAME>_USER` / `<NAME>_PASSWORD` (e.g.
/// `LACCD_USER`) take precedence: that is how CI, which has no Keychain,
/// passes them.
pub fn credentials(name: &str) -> Option<(String, String)> {
    let (user_var, password_var) = env_names(name);
    let var = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
    if let (Some(u), Some(p)) = (var(&user_var), var(&password_var)) {
        return Some((u, p));
    }
    Some((get(&format!("{name}-user"))?, get(&format!("{name}-password"))?))
}

/// `nursingcas` → (`NURSINGCAS_USER`, `NURSINGCAS_PASSWORD`).
pub fn env_names(name: &str) -> (String, String) {
    let base = name.to_ascii_uppercase().replace('-', "_");
    (format!("{base}_USER"), format!("{base}_PASSWORD"))
}

/// Reads the user (first line) and password (second line) from stdin and
/// stores them as `name`'s sign-in; returns them.
pub fn set_credentials_from_stdin(name: &str) -> Result<(String, String)> {
    credentials_from_reader(name, std::io::stdin())
}

fn credentials_from_reader(name: &str, mut reader: impl Read) -> Result<(String, String)> {
    let mut raw = String::new();
    reader.read_to_string(&mut raw).context("reading credentials from stdin")?;
    let mut lines = raw.lines();
    let (Some(user), Some(password)) = (lines.next().map(str::trim).filter(|l| !l.is_empty()), lines.next()) else {
        bail!("expected two lines on stdin: user, then password");
    };
    put(&format!("{name}-user"), user)?;
    put(&format!("{name}-password"), password)?;
    Ok((user.to_string(), password.to_string()))
}

pub fn laccd() -> Option<admit_laccd::Credentials> {
    credentials("laccd").map(|(user, password)| admit_laccd::Credentials { user, password })
}

#[cfg(test)]
#[path = "tests/session_tests.rs"]
mod tests;
