//! A line per portal write, appended before it is sent and again when it is
//! answered, so a run that stops midway (a crash, a dropped connection, a
//! killed process) leaves a record of which writes ran, which failed and
//! which one was in flight. Bodies are not recorded: they carry personal
//! details, and the profile re-derives them on the next plan.
//!
//! Format, tab-separated: `<UTC time>  started|done|failed  <METHOD> <path>  [detail]`.
//! A `started` line with no later `done`/`failed` for the same request is
//! the write that was interrupted.

use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Journal {
    path: Option<PathBuf>,
}

impl Journal {
    /// Records to `path` (its directory is created on the first write).
    pub fn to(path: impl Into<PathBuf>) -> Journal {
        Journal { path: Some(path.into()) }
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// Before a write: when this can't be recorded, the write must not run.
    pub fn started(&self, method: &str, path: &str) -> Result<(), String> {
        self.line("started", method, path, "")
    }

    pub fn done(&self, method: &str, path: &str) -> Result<(), String> {
        self.line("done", method, path, "")
    }

    pub fn failed(&self, method: &str, path: &str, why: &str) -> Result<(), String> {
        self.line("failed", method, path, why)
    }

    fn line(&self, step: &str, method: &str, path: &str, detail: &str) -> Result<(), String> {
        let Some(file) = &self.path else { return Ok(()) };
        let err = |e: std::io::Error| format!("journal {}: {e}", file.display());
        // `create_dir_all("")` (a bare file name) is a no-op.
        std::fs::create_dir_all(file.parent().unwrap_or(file)).map_err(err)?;
        let mut f = std::fs::OpenOptions::new().create(true).append(true).open(file).map_err(err)?;
        let detail: String = detail.chars().map(|c| if c.is_control() { ' ' } else { c }).take(300).collect();
        writeln!(f, "{}\t{step}\t{method} {path}\t{detail}", now()).map_err(err)
    }
}

fn now() -> String {
    let t = time::OffsetDateTime::now_utc();
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        t.year(),
        u8::from(t.month()),
        t.day(),
        t.hour(),
        t.minute(),
        t.second()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_step_is_one_line_and_the_interrupted_write_has_no_end() {
        let dir = std::env::temp_dir().join(format!("admit-journal-{}", std::process::id()));
        let j = Journal::to(dir.join("nested/journal.log"));
        j.started("DELETE", "/a/1").expect("start");
        j.done("DELETE", "/a/1").expect("done");
        j.started("POST", "/a").expect("start");
        j.failed("POST", "/a", "HTTP 500:\nline two").expect("failed");
        j.started("POST", "/b").expect("start");
        let text = std::fs::read_to_string(dir.join("nested/journal.log")).expect("read");
        let steps: Vec<(&str, &str, &str)> = text
            .lines()
            .map(|l| {
                let f: Vec<&str> = l.split('\t').collect();
                assert!(f[0].ends_with('Z') && f[0].len() == 20, "{l}");
                (f[1], f[2], f[3])
            })
            .collect();
        assert_eq!(
            steps,
            [
                ("started", "DELETE /a/1", ""),
                ("done", "DELETE /a/1", ""),
                ("started", "POST /a", ""),
                ("failed", "POST /a", "HTTP 500: line two"),
                ("started", "POST /b", ""),
            ]
        );
        // Nowhere to write: the caller is told, so the write never runs unrecorded.
        std::fs::write(dir.join("file"), "").expect("file");
        assert!(
            Journal::to(dir.join("file/journal.log")).started("PUT", "/x").expect_err("blocked").contains("journal")
        );
        assert!(Journal::to(&dir).started("PUT", "/x").expect_err("a directory").contains("journal"));
        assert_eq!(Journal::default().started("PUT", "/x"), Ok(()), "no journal: nothing recorded");
        assert_eq!(Journal::default().path(), None);
        let _ = std::fs::remove_dir_all(dir);
    }
}
