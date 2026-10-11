//! Unattended transcript → portal sync and its launchd schedule.
//!
//! `admit sync` signs in to LACCD and downloads the unofficial transcript
//! (`fetch = "laccd"`; falls back to the newest `LAC_SR_TSRPT*.pdf` in the
//! inbox when that fails), installs it as the profile's transcript only if it
//! parses, reconciles, and differs course-for-course from the installed one,
//! then applies the additive coursework changes this tool has verified against
//! the live portal (new courses, new terms, new institutions). Everything
//! else (posted grades, conflicts, anything needing a person) goes into a
//! dated report and a notification.
//!
//! Scheduling: launchd runs `sync --if-due` daily and [`due`] decides. A
//! launchd `StartInterval` was used first and dropped: its countdown restarts
//! at every login/reboot and does not run while asleep, so on a laptop an
//! 8-week interval would essentially never fire.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use admit_sources::{TranscriptCourse, normalize};
use anyhow::{Context, Result};
use serde::Deserialize;
use time::Date;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SyncConfig {
    /// Where downloaded transcripts land.
    pub inbox: PathBuf,
    /// Filename prefix of the transcript to pick up (e.g. "LAC_SR_TSRPT").
    pub prefix: String,
    /// The profile's transcript path that coursework.toml reads (relative to
    /// the profile directory).
    pub transcript: PathBuf,
    /// Report directory (relative to the config file).
    #[serde(default = "default_reports")]
    pub reports: PathBuf,
    /// Calendar runs as "MM-DD" (a week after each term ends, when grades post).
    #[serde(default)]
    pub run_on: Vec<String>,
    /// Also run when this many weeks have passed since the last successful sync.
    #[serde(default)]
    pub every_weeks: Option<u32>,
    /// Download the transcript instead of waiting for one in the inbox:
    /// "laccd" (sign-in from the Keychain, `admit laccd login`).
    #[serde(default)]
    pub fetch: Option<String>,
}

/// Why a scheduled `sync --if-due` should run today, or `None`.
pub fn due(today: Date, last: Option<Date>, run_on: &[String], every_weeks: Option<u32>) -> Result<Option<String>> {
    let Some(last) = last else { return Ok(Some("no successful sync recorded yet".into())) };
    for d in run_on {
        let (m, day) = month_day(d)?;
        // This year's and last year's occurrence: a run missed on Dec 29
        // (machine off) is still owed on Jan 3.
        for y in [today.year() - 1, today.year()] {
            let Some(on) = calendar_day(y, m, day) else { continue };
            if last < on && on <= today {
                return Ok(Some(format!("term-end run {d}")));
            }
        }
    }
    if let Some(w) = every_weeks
        && (today - last).whole_days() >= i64::from(w) * 7
    {
        return Ok(Some(format!("{w} weeks since the last sync ({last})")));
    }
    Ok(None)
}

fn calendar_day(y: i32, m: u8, day: u8) -> Option<Date> {
    let month = time::Month::try_from(m).ok()?;
    Date::from_calendar_date(y, month, day).ok()
}

fn month_day(d: &str) -> Result<(u8, u8)> {
    let (m, day) = d.split_once('-').with_context(|| format!("run_on {d:?} is not MM-DD"))?;
    let (m, day): (u8, u8) = (m.parse()?, day.parse()?);
    // A day that exists in some year (Feb 29 in a leap one): "02-30" would
    // otherwise pass and then never fire.
    anyhow::ensure!(calendar_day(2024, m, day).is_some(), "run_on {d:?} is not a date");
    Ok((m, day))
}

/// Course-level differences between two parses of the same transcript:
/// what a person would want to hear about (a grade posted, a class added or
/// dropped). Empty means the transcripts say the same thing.
pub fn transcript_changes(old: &[TranscriptCourse], new: &[TranscriptCourse]) -> Vec<String> {
    type Key = (String, String, String, String);
    let key = |c: &TranscriptCourse| (c.year.clone(), c.season.clone(), c.college.clone(), normalize(&c.code));
    let index = |v: &[TranscriptCourse]| {
        let mut m: BTreeMap<Key, Vec<TranscriptCourse>> = BTreeMap::new();
        for c in v {
            m.entry(key(c)).or_default().push(c.clone());
        }
        m
    };
    let (old, new) = (index(old), index(new));
    let label = |c: &TranscriptCourse| format!("{} {} {} {} {}", c.season, c.year, c.college, c.code, c.title);
    let mut out = Vec::new();
    for (k, n) in &new {
        let o = old.get(k).map(Vec::as_slice).unwrap_or_default();
        for (i, c) in n.iter().enumerate() {
            match o.get(i) {
                None => out.push(format!("added: {}", label(c))),
                Some(p) if p.grade != c.grade => out.push(format!(
                    "grade: {} {} -> {}",
                    label(c),
                    if p.grade.is_empty() { "(in progress)" } else { &p.grade },
                    if c.grade.is_empty() { "(in progress)" } else { &c.grade }
                )),
                Some(p) if p.credits != c.credits || p.title != c.title || p.noncredit != c.noncredit => {
                    out.push(format!("changed: {} (was {} units, {})", label(c), p.credits, p.title))
                }
                Some(_) => {}
            }
        }
        for p in o.iter().skip(n.len()) {
            out.push(format!("no longer listed: {}", label(p)));
        }
    }
    for (k, o) in &old {
        if !new.contains_key(k) {
            out.extend(o.iter().map(|p| format!("no longer listed: {}", label(p))));
        }
    }
    out
}

fn default_reports() -> PathBuf {
    "reports".into()
}

/// Newest `prefix*.pdf` in `inbox` modified after `since`.
struct InboxEntry {
    name: std::ffi::OsString,
    modified: std::io::Result<SystemTime>,
    path: PathBuf,
}

fn pick_newest(
    entries: impl IntoIterator<Item = InboxEntry>,
    prefix: &str,
    since: Option<SystemTime>,
) -> Option<PathBuf> {
    let mut best: Option<(SystemTime, PathBuf)> = None;
    for e in entries {
        let name = e.name.to_str().unwrap_or_default();
        if !name.starts_with(prefix) || !name.to_ascii_lowercase().ends_with(".pdf") {
            continue;
        }
        let Ok(m) = e.modified else { continue };
        if since.is_some_and(|s| m <= s) {
            continue;
        }
        if best.as_ref().is_none_or(|(b, _)| m > *b) {
            best = Some((m, e.path));
        }
    }
    best.map(|(_, p)| p)
}

pub fn newest_in_inbox(inbox: &Path, prefix: &str, since: Option<SystemTime>) -> Result<Option<PathBuf>> {
    let entries = std::fs::read_dir(inbox).with_context(|| format!("reading {}", inbox.display()))?;
    Ok(pick_newest(
        entries.flatten().map(|e| InboxEntry {
            name: e.file_name(),
            modified: e.metadata().and_then(|m| m.modified()),
            path: e.path(),
        }),
        prefix,
        since,
    ))
}

pub fn notify(title: &str, message: &str) {
    let esc = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
    let script = format!("display notification \"{}\" with title \"{}\"", esc(message), esc(title));
    let _ = std::process::Command::new("osascript").args(["-e", &script]).status();
}

pub const LABEL: &str = "com.admit.sync";

/// The launchd job: a daily `sync --if-due` (09:00; launchd runs a missed
/// calendar slot on wake). [`due`] turns that into term-end and N-week runs.
pub fn plist(exe: &Path, config: &Path, workdir: &Path, log: &Path, s: &SyncConfig) -> Result<String> {
    let esc = |p: &Path| p.display().to_string().replace('&', "&amp;").replace('<', "&lt;");
    for d in &s.run_on {
        month_day(d)?;
    }
    Ok(format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>{LABEL}</string>
  <key>ProgramArguments</key>
  <array><string>{}</string><string>-c</string><string>{}</string><string>sync</string><string>--if-due</string></array>
  <key>WorkingDirectory</key><string>{}</string>
  <key>StandardOutPath</key><string>{}</string>
  <key>StandardErrorPath</key><string>{}</string>
  <key>StartCalendarInterval</key>
  <dict><key>Hour</key><integer>9</integer><key>Minute</key><integer>0</integer></dict>
</dict>
</plist>
"#,
        esc(exe),
        esc(config),
        esc(workdir),
        esc(log),
        esc(log)
    ))
}

#[cfg(test)]
#[path = "tests/sync_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "tests/sync_boundaries.rs"]
mod boundaries;
