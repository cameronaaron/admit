use super::*;

#[test]
fn takes_the_last_matching_cookie() {
    let dir = std::env::temp_dir().join(format!("admit-har-{}", std::process::id()));
    std::fs::create_dir_all(&dir).ok();
    let har = serde_json::json!({"log": {"entries": [
        {"request": {"url": "https://x.example/api/a", "headers": [{"name": "Cookie", "value": "old"}]}},
        {"request": {"url": "https://other.example/", "headers": [{"name": "cookie", "value": "nope"}]}},
        {"request": {"url": "https://x.example/api/b", "headers": [{"name": "cookie", "value": "new"}]}},
        {"request": {"url": "https://other.example/2", "headers": [{"name": "cookie", "value": "nope2"}]}}
    ]}});
    std::fs::write(dir.join("a.har"), har.to_string()).ok();
    assert_eq!(newest_cookie(&dir, "https://x.example/api/").ok(), Some("new".into()));
    assert!(matches!(newest_cookie(&dir, "https://none.example/"), Err(HarError::NoCookie(..))));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn newest_file_wins_and_unreadable_or_odd_entries_are_skipped() {
    let dir = std::env::temp_dir().join(format!("admit-har-order-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("directory");
    let older = dir.join("old.har");
    let newer = dir.join("new.har");
    // Scanned newest-entry first, so the malformed rows sit after the cookie and are visited.
    let body = |cookie: &str| {
        serde_json::json!({"log": {"entries": [
            {"request": {"url": "https://x.example/api/a", "headers": [
                {"name": 1, "value": "nope"},
                {"value": "x"},
                {"name": "Accept", "value": "text"},
                {"name": "cookie"},
                {"name": "cookie", "value": 5},
                {"name": "Cookie", "value": cookie}
            ]}},
            {"request": {"url": 1}},
            {"request": {}},
            {"request": {"url": "https://x.example/api/a", "headers": {"name": "cookie"}}},
            {"request": {"url": "https://x.example/api/a"}}
        ]}})
        .to_string()
    };
    std::fs::write(&older, body("old")).expect("old");
    std::fs::write(&newer, body("new")).expect("new");
    let stamp = |path: &std::path::Path, secs: u64| {
        let file = std::fs::File::options().write(true).open(path).expect("open");
        let when = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(secs);
        file.set_times(std::fs::FileTimes::new().set_modified(when)).expect("mtime");
    };
    stamp(&older, 1_000);
    stamp(&newer, 2_000);
    std::fs::write(dir.join("notes.txt"), "not a capture").expect("txt");
    std::os::unix::fs::symlink("missing-target", dir.join("gone.har")).expect("symlink");
    assert_eq!(newest_cookie(&dir, "https://x.example/").expect("cookie"), "new");
    std::fs::remove_dir_all(&dir).expect("cleanup");
}
