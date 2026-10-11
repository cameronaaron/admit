use std::path::Path;
use std::process::Command;

use super::{Cli, Ctx, ctx, dispatch_liaison};

use clap::Parser;

fn isolated(dir: &Path) -> Ctx {
    std::fs::create_dir_all(dir.join("profile")).expect("profile");
    Ctx {
        profile_dir: dir.join("profile"),
        base: dir.to_path_buf(),
        portal_name: None,
        config: ctx::Config { profile: "profile".into(), portals: vec![], sync: None, website: None },
    }
}

#[tokio::test]
async fn every_liaison_command_reaches_its_arm() {
    tokio::task::unconstrained(async move {
        if std::env::var_os("ADMIT_DISPATCH_SCHEDULE").is_some() {
            let dir = std::path::PathBuf::from(std::env::var_os("ADMIT_DISPATCH_DIR").expect("dir"));
            let ctx = isolated(&dir);
            let config = dir.join("admit.toml");
            std::fs::write(&config, "portal=[]\n").expect("config");
            let config = config.display().to_string();
            for action in ["install", "uninstall"] {
                let cli = Cli::try_parse_from(["admit", "-c", config.as_str(), "schedule", action]).expect("parse");
                let err = dispatch_liaison(&ctx, &cli).await.expect_err(action);
                assert!(err.to_string().contains("os error"), "{action}: {err}");
            }
            std::fs::write(dir.join("done"), "ok").expect("done");
            return;
        }
        let dir = std::env::temp_dir().join(format!("admit-dispatch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        let ctx = isolated(&dir);
        let config = dir.join("admit.toml");
        std::fs::write(&config, "portal=[]\n").expect("config");
        let config = config.display().to_string();
        let cli = |args: &[&str]| {
            let mut full = vec!["admit", "-c", config.as_str()];
            full.extend_from_slice(args);
            Cli::try_parse_from(full).expect("parse")
        };
        for (args, needle) in [
            (["questions"].as_slice(), "no [[portal]]"),
            (&["questions", "--open"], "no [[portal]]"),
            (&["documents"], "no [[portal]]"),
            (&["documents", "--open"], "no [[portal]]"),
            (["get", "aca"].as_slice(), "Slate"),
            (&["schools", "US:CA"], "Slate"),
            (&["schools", "US:CA", "Los Angeles"], "Slate"),
            (&["lookup", "o1", "Yale"], "Slate"),
            (&["status"], "Slate"),
            (&["website"], "[website]"),
            (&["plan"], "no [[portal]]"),
            (&["plan", "--only", "education"], "no [[portal]]"),
            (&["apply"], "no [[portal]]"),
            (&["apply", "--yes"], "no [[portal]]"),
            (&["apply", "--reveal"], "--reveal is for Slate"),
            (&["pull"], "no [[portal]]"),
            (&["pull", "--force"], "no [[portal]]"),
            (&["audit"], "no [[portal]]"),
            (&["courses"], "no [[portal]]"),
            (&["prereqs"], "no [[portal]]"),
            (&["session", "clear"], "no [[portal]]"),
            (&["sync"], "[sync]"),
            (&["sync", "--if-due"], "[sync]"),
        ] {
            let err = dispatch_liaison(&ctx, &cli(args)).await.expect_err(args[0]);
            assert!(err.to_string().contains(needle), "{needle}: {err}");
        }
        dispatch_liaison(&ctx, &cli(&["validate"])).await.expect("validate");
        let empty = dir.join("empty");
        std::fs::create_dir_all(&empty).expect("empty");
        let current = std::thread::current();
        let name = current.name().expect("test name");
        let out = Command::new(std::env::current_exe().expect("exe"))
            .args(["--exact", name, "--test-threads=1"])
            .env("ADMIT_DISPATCH_SCHEDULE", "1")
            .env("ADMIT_DISPATCH_DIR", &dir)
            .env("HOME", &dir)
            .env("PATH", &empty)
            .output()
            .expect("child");
        let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
        assert!(out.status.success() && dir.join("done").is_file(), "{text}");
        std::fs::remove_dir_all(&dir).expect("cleanup");
    })
    .await
}
