use std::os::unix::ffi::OsStringExt;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;

use super::*;
#[test]
fn config_defaults_and_invalid_schedules_are_explicit() {
    let cfg: SyncConfig = toml::from_str("inbox='.'\nprefix='x'\ntranscript='x.pdf'").expect("config");
    assert_eq!(cfg.reports, PathBuf::from("reports"));
    for bad in ["", "01", "x-01", "01-x", "00-01", "01-00", "01-32"] {
        assert!(month_day(bad).is_err(), "{bad}");
    }
    assert!(
        due(time::macros::date!(2026 - 03 - 01), Some(time::macros::date!(2026 - 01 - 01)), &["02-29".into()], None)
            .expect("due")
            .is_none()
    );
}
#[test]
fn duplicate_attempts_and_changed_units_are_reported() {
    let c = |title: &str| TranscriptCourse {
        year: "2026".into(),
        season: "Fall".into(),
        college: "LAVC".into(),
        code: "BIO 1".into(),
        title: title.into(),
        credits: "3".into(),
        grade: "A".into(),
        noncredit: false,
        term_months: None,
    };
    let old = [c("Biology"), c("Retake")];
    let mut new = c("Updated title");
    new.credits = "4".into();
    let changes = transcript_changes(&old, &[new]);
    assert_eq!(changes.len(), 2);
    assert!(changes[0].starts_with("changed:"));
    assert!(changes[1].contains("no longer listed:"));
    assert!(changes[1].contains("Retake"));
}
#[test]
fn calendar_dates_grade_direction_and_inbox_order() {
    assert!(calendar_day(2026, 0, 1).is_none());
    assert!(calendar_day(2026, 13, 1).is_none());
    assert!(calendar_day(2026, 2, 29).is_none());
    assert!(calendar_day(2024, 2, 29).is_some());
    assert_eq!(calendar_day(2026, 10, 6), Some(time::macros::date!(2026 - 10 - 06)));
    assert!(
        due(time::macros::date!(2026 - 10 - 06), Some(time::macros::date!(2026 - 10 - 01)), &["nope".into()], None)
            .is_err()
    );
    let c = |grade: &str, credits: &str, title: &str, noncredit: bool| TranscriptCourse {
        year: "2026".into(),
        season: "Fall".into(),
        college: "LAVC".into(),
        code: "BIO 1".into(),
        title: title.into(),
        credits: credits.into(),
        grade: grade.into(),
        noncredit,
        term_months: None,
    };
    let graded = c("A", "3", "Biology", false);
    let cleared = transcript_changes(std::slice::from_ref(&graded), &[c("", "3", "Biology", false)]);
    assert!(cleared[0].contains("A -> (in progress)"), "{cleared:?}");
    assert!(transcript_changes(std::slice::from_ref(&graded), &[c("B", "3", "Biology", false)])[0].contains("A -> B"));
    assert!(
        transcript_changes(std::slice::from_ref(&graded), &[c("A", "4", "Biology", false)])[0].starts_with("changed:")
    );
    assert!(
        transcript_changes(std::slice::from_ref(&graded), &[c("A", "3", "Other", false)])[0].starts_with("changed:")
    );
    assert!(
        transcript_changes(std::slice::from_ref(&graded), &[c("A", "3", "Biology", true)])[0].starts_with("changed:")
    );
    let t = |secs| SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(secs);
    let entry = |name: &str, modified: std::io::Result<SystemTime>| InboxEntry {
        name: name.into(),
        modified,
        path: PathBuf::from(name),
    };
    let picked = pick_newest(
        [
            entry("notes.txt", Ok(t(1))),
            entry("OTHER.pdf", Ok(t(9))),
            InboxEntry { name: std::ffi::OsString::from_vec(vec![0xff]), modified: Ok(t(9)), path: "bad".into() },
            entry("LAC.PDF", Err(std::io::Error::other("mtime"))),
            entry("LAC-new.pdf", Ok(t(300))),
            entry("LAC-old.pdf", Ok(t(100))),
            entry("LAC-mid.pdf", Ok(t(200))),
        ],
        "LAC",
        Some(t(150)),
    );
    assert_eq!(picked.as_deref(), Some(Path::new("LAC-new.pdf")));
    assert_eq!(
        pick_newest([entry("LAC-old.pdf", Ok(t(100))), entry("LAC-new.pdf", Ok(t(300)))], "LAC", None).as_deref(),
        Some(Path::new("LAC-new.pdf"))
    );
    assert!(pick_newest([entry("LAC-new.pdf", Ok(t(300)))], "LAC", Some(t(300))).is_none());
}

#[test]
fn an_absent_inbox_is_an_error() {
    assert!(
        newest_in_inbox(Path::new("/nonexistent/admit-inbox"), "x", None)
            .expect_err("missing")
            .to_string()
            .contains("reading")
    );
}

#[test]
fn quiet_notify() {
    if std::env::var_os("ADMIT_NOTIFY").is_some() {
        notify("a\"b", "c\\d");
        notify("", "");
        let dir = std::path::PathBuf::from(std::env::var_os("ADMIT_NOTIFY_DIR").expect("dir"));
        std::fs::write(dir.join("done"), "ok").expect("done");
        return;
    }
    let dir = std::env::temp_dir().join(format!("admit-notify-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let bin = dir.join("bin");
    std::fs::create_dir_all(&bin).expect("bin");
    std::fs::write(bin.join("osascript"), "#!/bin/sh\nexit 1\n").expect("script");
    std::fs::set_permissions(bin.join("osascript"), std::fs::Permissions::from_mode(0o755)).expect("mode");
    let current = std::thread::current();
    let name = current.name().expect("test name");
    let out = Command::new(std::env::current_exe().expect("exe"))
        .args(["--exact", name, "--test-threads=1"])
        .env("ADMIT_NOTIFY", "1")
        .env("ADMIT_NOTIFY_DIR", &dir)
        .env("PATH", &bin)
        .output()
        .expect("child");
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success() && dir.join("done").is_file(), "{text}");
    std::fs::remove_dir_all(&dir).expect("cleanup");
}
