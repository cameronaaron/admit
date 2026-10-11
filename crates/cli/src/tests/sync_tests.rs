use super::*;

fn cfg() -> SyncConfig {
    SyncConfig {
        inbox: "/tmp".into(),
        prefix: "LAC_SR_TSRPT".into(),
        transcript: "transcripts/laccd.pdf".into(),
        reports: "reports".into(),
        run_on: vec!["12-29".into(), "06-15".into()],
        every_weeks: Some(8),
        fetch: Some("laccd".into()),
    }
}

#[test]
fn plist_runs_a_daily_if_due_check_and_no_start_interval() {
    let p = plist(Path::new("/bin/admit"), Path::new("/a/admit.toml"), Path::new("/a"), Path::new("/a/r.log"), &cfg())
        .unwrap_or_default();
    assert!(p.contains("<string>sync</string><string>--if-due</string>"));
    assert!(p.contains("<key>StartCalendarInterval</key>\n  <dict><key>Hour</key><integer>9</integer>"));
    assert!(!p.contains("<key>Month</key>"), "calendar dates are decided by `due`, not launchd");
    assert!(!p.contains("StartInterval</key><integer>"), "StartInterval restarts at every login");
}

fn d(y: i32, m: u8, day: u8) -> Date {
    Date::from_calendar_date(y, time::Month::try_from(m).unwrap_or(time::Month::January), day).unwrap_or(Date::MIN)
}

#[test]
fn due_on_term_ends_after_eight_weeks_and_never_twice() {
    let run_on = cfg().run_on;
    let due = |today, last| due(today, last, &run_on, Some(8)).ok().flatten();
    assert!(due(d(2026, 10, 6), None).is_some(), "first run");
    assert!(due(d(2026, 12, 29), Some(d(2026, 12, 1))).is_some(), "term end day");
    assert!(due(d(2027, 1, 3), Some(d(2026, 12, 1))).is_some(), "term end missed while the Mac was off");
    assert!(due(d(2026, 12, 30), Some(d(2026, 12, 29))).is_none(), "already ran for that term end");
    assert!(due(d(2026, 11, 30), Some(d(2026, 10, 6))).is_none(), "55 days, no term end between");
    assert!(due(d(2026, 12, 1), Some(d(2026, 10, 6))).is_some(), "56 days = 8 weeks");
}

fn course(code: &str, grade: &str) -> TranscriptCourse {
    TranscriptCourse {
        year: "2026".into(),
        season: "Fall".into(),
        code: code.into(),
        college: "LAVC".into(),
        title: format!("{code} title"),
        credits: "3.00".into(),
        grade: grade.into(),
        noncredit: false,
        term_months: None,
    }
}

#[test]
fn transcript_changes_report_grades_adds_and_drops() {
    let old = [course("BIOLOGY 003", ""), course("CHEM 101", ""), course("MATH 227", "A")];
    let new = [course("BIOLOGY 003", "A"), course("MATH 227", "A"), course("PSYCH 001", "")];
    let ch = transcript_changes(&old, &new);
    assert_eq!(ch.len(), 3, "{ch:?}");
    assert!(
        ch.iter().any(|c| c.starts_with("grade:") && c.contains("BIOLOGY 003") && c.ends_with("(in progress) -> A"))
    );
    assert!(ch.iter().any(|c| c.starts_with("added:") && c.contains("PSYCH 001")));
    assert!(ch.iter().any(|c| c.starts_with("no longer listed:") && c.contains("CHEM 101")));
    assert!(transcript_changes(&new, &new).is_empty(), "same courses, nothing to report");
}

#[test]
fn bad_run_on_dates_are_rejected() {
    let mut c = cfg();
    for bad in ["13-01", "02-30", "04-31", "00-10", "06-00"] {
        c.run_on = vec![bad.into()];
        assert!(plist(Path::new("a"), Path::new("b"), Path::new("c"), Path::new("d"), &c).is_err(), "{bad}");
    }
    c.run_on = vec!["02-29".into(), "12-31".into()];
    assert!(plist(Path::new("a"), Path::new("b"), Path::new("c"), Path::new("d"), &c).is_ok());
}

#[test]
fn inbox_picks_the_newest_matching_pdf_after_since() {
    let dir = std::env::temp_dir().join(format!("admit-inbox-{}", std::process::id()));
    std::fs::create_dir_all(&dir).ok();
    for (name, secs) in
        [("LAC_SR_TSRPT.pdf", 100), ("LAC_SR_TSRPT (1).pdf", 200), ("other.pdf", 300), ("LAC_SR_TSRPT.txt", 400)]
    {
        let p = dir.join(name);
        std::fs::write(&p, b"x").ok();
        let t = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000 + secs);
        std::fs::File::options().write(true).open(&p).and_then(|f| f.set_modified(t)).ok();
    }
    let base = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000);
    let pick = newest_in_inbox(&dir, "LAC_SR_TSRPT", None).ok().flatten();
    assert_eq!(
        pick.and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned())),
        Some("LAC_SR_TSRPT (1).pdf".into())
    );
    let none = newest_in_inbox(&dir, "LAC_SR_TSRPT", Some(base + std::time::Duration::from_secs(200))).ok().flatten();
    assert!(none.is_none(), "nothing newer than the installed transcript");
    std::fs::remove_dir_all(&dir).ok();
}
