use super::*;
#[test]
fn empty_bad_and_unreadable_captures_report_their_paths() {
    let dir = std::env::temp_dir().join(format!("admit-har-errors-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("directory");
    assert!(matches!(newest_cookie(&dir, "https://x/"), Err(HarError::NoHar(_))));
    let missing = dir.join("missing");
    assert!(hars_newest_first(&missing).expect_err("missing dir").to_string().contains("missing"));
    assert!(matches!(cookie_from_har(&missing, "https://x/"), Err(HarError::Io(..))));
    let p = dir.join("bad.har");
    std::fs::write(&p, "{").expect("write");
    assert!(cookie_from_har(&p, "https://x/").expect_err("invalid").to_string().contains("not a HAR"));
    std::fs::write(&p, "{}").expect("empty");
    assert_eq!(cookie_from_har(&p, "https://x/").expect("empty"), None);
    assert!(matches!(newest_cookie(&dir, "https://x/"), Err(HarError::NoCookie(..))));
    let missing_dir = dir.join("absent");
    assert!(newest_cookie(&missing_dir, "https://x/").expect_err("missing").to_string().contains("absent"));
    std::fs::write(dir.join("broken.har"), "{").expect("write");
    assert!(newest_cookie(&dir, "https://x/").expect_err("json").to_string().contains("not a HAR"));
    std::fs::remove_dir_all(dir).expect("cleanup");
}

#[test]
fn a_har_whose_metadata_cannot_be_read_is_skipped() {
    let dir = std::env::temp_dir().join(format!("admit-har-dangling-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("directory");
    std::os::unix::fs::symlink(dir.join("gone"), dir.join("dangling.har")).expect("symlink");
    std::fs::write(dir.join("real.har"), "{}").expect("write");
    assert_eq!(hars_newest_first(&dir).expect("listing"), vec![dir.join("real.har")]);
    std::fs::remove_dir_all(dir).expect("cleanup");
}
