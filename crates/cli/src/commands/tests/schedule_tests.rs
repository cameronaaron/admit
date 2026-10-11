use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::ctx::{Config, Ctx};
use crate::sync::{LABEL, SyncConfig};

fn script(dir: &Path, name: &str, body: &str) {
    let p = dir.join(name);
    std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).expect("script");
    std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o755)).expect("mode");
}

fn ctx(base: &Path, sync: Option<SyncConfig>) -> Ctx {
    Ctx {
        profile_dir: base.join("profile"),
        base: base.to_path_buf(),
        portal_name: None,
        config: Config { profile: "profile".into(), portals: vec![], sync, website: None },
    }
}

fn sync_cfg(dir: &Path) -> SyncConfig {
    SyncConfig {
        inbox: dir.join("inbox"),
        prefix: "t".into(),
        transcript: "t.pdf".into(),
        reports: "reports".into(),
        run_on: vec![],
        every_weeks: None,
        fetch: None,
    }
}

fn child(dir: &Path, mode: &str) {
    let base = dir.join("base");
    let config = dir.join("admit.toml");
    let c = ctx(&base, Some(sync_cfg(dir)));
    match mode {
        "no-home" => {
            let err = super::run(&c, &config, true).expect_err("home");
            assert!(err.to_string().contains("HOME is not set"), "{err}");
        }
        "no-id" => {
            let err = super::run(&c, &config, true).expect_err("id");
            assert!(err.to_string().contains("os error"), "{err}");
        }
        "bad-id" => {
            let err = super::run(&c, &config, true).expect_err("utf8");
            assert!(err.to_string().to_lowercase().contains("utf-8"), "{err}");
        }
        "no-sync" => {
            let bare = ctx(&base, None);
            let err = super::run(&bare, &config, true).expect_err("sync");
            assert!(err.to_string().contains("no [sync]"), "{err}");
        }
        "bad-base" => {
            let missing = ctx(&dir.join("missing"), Some(sync_cfg(dir)));
            let err = super::run(&missing, &config, true).expect_err("base");
            assert!(err.to_string().contains("os error"), "{err}");
        }
        "reports-file" => {
            let err = super::run(&c, &config, true).expect_err("reports");
            assert!(err.to_string().contains("File exists") || err.to_string().contains("Not a directory"), "{err}");
        }
        "no-exe" => {
            let err = super::run_with(&c, &config, true, || Err(std::io::Error::other("no exe"))).expect_err("exe");
            assert!(err.to_string().contains("no exe"), "{err}");
        }
        "missing-config" => {
            let err = super::run(&c, &dir.join("nope.toml"), true).expect_err("config");
            assert!(err.to_string().contains("os error"), "{err}");
        }
        "bad-date" => {
            let mut cfg = sync_cfg(dir);
            cfg.run_on = vec!["nope".into()];
            let dated = ctx(&base, Some(cfg));
            let err = super::run(&dated, &config, true).expect_err("date");
            assert!(err.to_string().contains("run_on"), "{err}");
        }
        "agents-file" => {
            let err = super::run(&c, &config, true).expect_err("agents");
            assert!(err.to_string().contains("File exists") || err.to_string().contains("Not a directory"), "{err}");
        }
        "plist-dir" => {
            let err = super::run(&c, &config, true).expect_err("write");
            assert!(err.to_string().contains("directory") || err.to_string().contains("Directory"), "{err}");
        }
        "no-launchctl" => {
            let home = PathBuf::from(std::env::var_os("HOME").expect("home"));
            let err = super::run(&c, &config, true).expect_err("spawn");
            assert!(!err.to_string().contains("bootstrap failed"), "{err}");
            assert!(home.join(format!("Library/LaunchAgents/{LABEL}.plist")).is_file(), "plist written");
        }
        "launchctl-fail" => {
            let err = super::run(&c, &config, true).expect_err("bootstrap");
            assert!(err.to_string().contains("launchctl bootstrap failed"), "{err}");
        }
        "installed" => {
            super::run(&c, &config, true).expect("install");
        }
        "uninstall" => {
            super::run_with(&c, &config, false, || panic!("uninstall must not resolve the executable"))
                .expect("uninstall");
            super::run(&c, &config, false).expect("already gone");
        }
        other => panic!("unknown mode {other}"),
    }
    std::fs::write(dir.join("done"), mode).expect("done");
}

fn prepare(dir: &Path, mode: &str) {
    let bin = dir.join("bin");
    let base = dir.join("base");
    let home = dir.join("home");
    std::fs::create_dir_all(&bin).expect("bin");
    std::fs::create_dir_all(&base).expect("base");
    std::fs::create_dir_all(&home).expect("home");
    std::fs::write(dir.join("admit.toml"), "x").expect("config");
    let id = if mode == "bad-id" { "printf '\\377'" } else { "printf '%s' 501" };
    if mode != "no-id" {
        script(&bin, "id", id);
    }
    if mode == "launchctl-fail" {
        script(&bin, "launchctl", "exit 1");
    } else if mode == "installed" {
        script(&bin, "launchctl", "exit 0");
    }
    if mode == "reports-file" {
        std::fs::write(base.join("reports"), b"x").expect("reports file");
    }
    if mode == "agents-file" {
        std::fs::create_dir_all(home.join("Library")).expect("library");
        std::fs::write(home.join("Library/LaunchAgents"), b"x").expect("agents file");
    }
    if mode == "plist-dir" {
        std::fs::create_dir_all(home.join(format!("Library/LaunchAgents/{LABEL}.plist"))).expect("plist dir");
    }
}

#[test]
fn shims() {
    let mode = std::env::var("ADMIT_SCHEDULE_MODE").ok();
    let dir = std::env::var_os("ADMIT_SCHEDULE_DIR").map(PathBuf::from);
    if let (Some(mode), Some(dir)) = (mode, dir) {
        child(&dir, &mode);
        return;
    }
    let current = std::thread::current();
    let name = current.name().expect("test name");
    let mut failed = String::new();
    for mode in [
        "no-home",
        "no-id",
        "bad-id",
        "no-sync",
        "bad-base",
        "reports-file",
        "no-exe",
        "missing-config",
        "bad-date",
        "agents-file",
        "plist-dir",
        "no-launchctl",
        "launchctl-fail",
        "installed",
        "uninstall",
    ] {
        let dir = std::env::temp_dir().join(format!("admit-schedule-{mode}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        prepare(&dir, mode);
        let mut cmd = Command::new(std::env::current_exe().expect("exe"));
        cmd.args(["--exact", name, "--test-threads=1"])
            .env("ADMIT_SCHEDULE_MODE", mode)
            .env("ADMIT_SCHEDULE_DIR", &dir)
            .env("PATH", dir.join("bin"))
            .current_dir(&dir);
        if mode == "no-home" {
            cmd.env_remove("HOME");
        } else {
            cmd.env("HOME", dir.join("home"));
        }
        let out = cmd.output().expect("child");
        let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
        if !(out.status.success() && dir.join("done").is_file()) {
            failed.push_str(&format!("\n{mode}: {text}"));
        }
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }
    assert!(failed.is_empty(), "{failed}");
}
