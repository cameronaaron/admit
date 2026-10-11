//! A fresh clone has only what git tracks. A test input that `.gitignore`
//! drops (the `*.pdf` rule took the synthetic LACCD transcript, and every CI
//! run failed to compile `include_bytes!`) passes here and nowhere else.
use std::path::Path;
use std::process::Command;

#[test]
fn every_file_under_crates_and_fuzz_is_tracked_or_built() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let out = Command::new("git")
        .args(["ls-files", "--others", "--ignored", "--exclude-standard", "--", "crates", "fuzz"])
        .current_dir(&root)
        .output()
        .expect("git runs");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let ignored: Vec<_> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|f| !f.contains("/target/") && !f.contains("__pycache__") && !f.ends_with(".DS_Store"))
        .map(str::to_owned)
        .collect();
    assert!(ignored.is_empty(), "ignored by git, so missing from a fresh checkout: {ignored:?}");
}
