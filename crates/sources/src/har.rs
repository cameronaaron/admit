//! Session cookies from browser HAR captures.
//!
//! Portals behind SSO have no API tokens; the applicant's browser session is
//! the credential. Export a HAR from the logged-in portal (DevTools → Network
//! → Save all as HAR) and point the config at its directory. Sessions expire
//! within hours; a stale one shows up as a login page instead of JSON.

use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum HarError {
    #[error("reading {0}: {1}")]
    Io(String, std::io::Error),
    #[error("{0} is not a HAR: {1}")]
    Json(String, serde_json::Error),
    #[error("no .har file in {0}")]
    NoHar(String),
    #[error("no request to {0} carried a Cookie header in {1}")]
    NoCookie(String, String),
}

/// `.har` files in `dir`, newest first.
pub fn hars_newest_first(dir: &Path) -> Result<Vec<PathBuf>, HarError> {
    let rd = std::fs::read_dir(dir).map_err(|e| HarError::Io(dir.display().to_string(), e))?;
    let mut v: Vec<(std::time::SystemTime, PathBuf)> = rd
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "har"))
        .filter_map(|p| Some((std::fs::metadata(&p).and_then(|m| m.modified()).ok()?, p)))
        .collect();
    v.sort_by_key(|e| std::cmp::Reverse(e.0));
    Ok(v.into_iter().map(|(_, p)| p).collect())
}

/// The Cookie header of the last request whose URL starts with `url_prefix`.
pub fn cookie_from_har(path: &Path, url_prefix: &str) -> Result<Option<String>, HarError> {
    let text = std::fs::read_to_string(path).map_err(|e| HarError::Io(path.display().to_string(), e))?;
    let har: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| HarError::Json(path.display().to_string(), e))?;
    let entries = har["log"]["entries"].as_array().map(Vec::as_slice).unwrap_or_default();
    Ok(entries.iter().rev().find_map(|e| {
        let req = &e["request"];
        if !req["url"].as_str()?.starts_with(url_prefix) {
            return None;
        }
        req["headers"].as_array()?.iter().find_map(|h| {
            h["name"].as_str()?.eq_ignore_ascii_case("cookie").then(|| h["value"].as_str().map(str::to_owned))?
        })
    }))
}

/// The newest session cookie for `url_prefix` across the HARs in `dir`.
pub fn newest_cookie(dir: &Path, url_prefix: &str) -> Result<String, HarError> {
    let hars = hars_newest_first(dir)?;
    if hars.is_empty() {
        return Err(HarError::NoHar(dir.display().to_string()));
    }
    for p in &hars {
        if let Some(c) = cookie_from_har(p, url_prefix)? {
            return Ok(c);
        }
    }
    Err(HarError::NoCookie(url_prefix.into(), dir.display().to_string()))
}

#[cfg(test)]
#[path = "tests/har_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "tests/har_error_tests.rs"]
mod error_tests;
