use super::*;

#[tokio::test]
async fn download_candidates_are_saved_and_failures_fall_back_to_the_inbox() {
    tokio::task::unconstrained(async move {
        let dir = std::env::temp_dir().join(format!("admit-sync-candidate-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let cfg: sync::SyncConfig = toml::from_str(&format!(
            "inbox='{}'\nprefix='transcript'\ntranscript='installed.pdf'\nfetch='laccd'",
            dir.display()
        ))
        .expect("cfg");
        let mut report = String::new();
        let pdf = include_bytes!("../../../../laccd/tests/fixtures/transcript.pdf").to_vec();
        let candidate = candidate_with(&cfg, &dir, None, &mut report, std::future::ready(Ok(pdf.clone())))
            .await
            .expect("download")
            .expect("candidate");
        assert_eq!(std::fs::read(&candidate).expect("saved"), pdf);
        assert!(report.contains("downloaded"));
        let fallback = candidate_with(
            &cfg,
            &dir,
            None,
            &mut report,
            std::future::ready(Err(anyhow::anyhow!("network unavailable"))),
        )
        .await
        .expect("fallback");
        assert_eq!(fallback, Some(candidate));
        assert!(report.contains("network unavailable"));
        let fallback = candidate_with(&cfg, &dir.join("missing"), None, &mut report, std::future::ready(Ok(pdf)))
            .await
            .expect("a save failure also falls back");
        assert!(fallback.is_some());
        assert!(report.contains("No such file"));
        let mut empty_inbox = cfg.clone();
        empty_inbox.inbox = dir.join("empty");
        std::fs::create_dir_all(&empty_inbox.inbox).expect("empty inbox");
        let none = candidate_with(
            &empty_inbox,
            &dir.join("missing"),
            None,
            &mut report,
            std::future::ready(Err(anyhow::anyhow!("network unavailable"))),
        )
        .await
        .expect("fallback");
        assert!(none.is_none(), "{none:?}");
        assert!(report.contains("checking the inbox"), "{report}");
        std::fs::remove_dir_all(dir).expect("cleanup");
    })
    .await
}

#[tokio::test]
async fn an_unknown_fetch_and_an_empty_inbox_are_reported() {
    tokio::task::unconstrained(async move {
        let dir = std::env::temp_dir().join(format!("admit-sync-inbox-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("inbox")).expect("inbox");
        let inbox = dir.join("inbox");
        let plain: sync::SyncConfig =
            toml::from_str(&format!("inbox='{}'\nprefix='transcript'\ntranscript='installed.pdf'", inbox.display()))
                .expect("cfg");
        let mut report = String::new();
        let found =
            candidate_with(&plain, &dir, None, &mut report, std::future::ready(Ok(Vec::new()))).await.expect("inbox");
        assert!(found.is_none(), "{found:?}");
        assert!(report.contains("no transcript newer"), "{report}");
        let other: sync::SyncConfig = toml::from_str(&format!(
            "inbox='{}'\nprefix='transcript'\ntranscript='installed.pdf'\nfetch='other'",
            inbox.display()
        ))
        .expect("cfg");
        let err = candidate_with(&other, &dir, None, &mut report, std::future::ready(Ok(Vec::new())))
            .await
            .expect_err("fetch");
        assert!(err.to_string().contains("not supported"), "{err:#}");
        std::fs::remove_dir_all(dir).expect("cleanup");
    })
    .await
}

fn workspace(kind: &str, sync_toml: &str) -> (std::path::PathBuf, crate::ctx::Ctx) {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "admit-sync-run-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    std::fs::create_dir_all(dir.join("profile")).expect("profile");
    std::fs::create_dir_all(dir.join("inbox")).expect("inbox");
    std::fs::create_dir_all(dir.join("reports")).expect("reports");
    let mut ctx = crate::ctx::Ctx {
        profile_dir: dir.join("profile"),
        base: dir.clone(),
        portal_name: None,
        config: crate::ctx::Config {
            profile: "profile".into(),
            portals: vec![crate::ctx::PortalConfig {
                name: "test".into(),
                kind: kind.into(),
                base_url: "http://127.0.0.1:9".into(),
                har_dir: None,
                cookie_env: None,
                application: None,
                submitted: false,
                deadline: None,
                same_as: None,
            }],
            sync: None,
            website: None,
        },
    };
    ctx.config.sync = Some(toml::from_str(&sync_toml.replace("$DIR", &dir.display().to_string())).expect("sync"));
    (dir, ctx)
}

#[test]
fn a_path_with_no_parent_needs_no_directory() {
    ensure_parent(std::path::Path::new("/")).expect("root");
    ensure_parent(std::path::Path::new("/.")).expect("dot");
}

#[tokio::test]
async fn sync_stops_on_the_schedule_the_report_file_and_the_transcript_itself() {
    tokio::task::unconstrained(async move {
        let (dir, ctx) = workspace(
            "liaison",
            "inbox='$DIR/missing'\nprefix='transcript'\ntranscript='laccd.pdf'\nreports='reports'\n",
        );
        let mut report = String::new();
        let err = sync_inner(&ctx, ctx.config.sync.as_ref().expect("sync"), &dir.join("reports"), &mut report)
            .await
            .expect_err("inbox");
        assert!(err.to_string().contains("reading"), "{err:#}");
        std::fs::remove_dir_all(&dir).expect("cleanup");

        let (dir, ctx) = workspace(
            "liaison",
            "inbox='$DIR/inbox'\nprefix='transcript'\ntranscript='laccd.pdf'\nreports='reports'\nrun_on=['nope']\n",
        );
        std::fs::write(dir.join("reports/last-sync"), "2026-01-01\n").expect("last");
        assert!(run(&ctx, false).await.expect_err("date").to_string().contains("not MM-DD"));
        std::fs::remove_dir_all(&dir).expect("cleanup");

        let (dir, mut ctx) = workspace(
            "liaison",
            "inbox='$DIR/inbox'\nprefix='transcript'\ntranscript='laccd.pdf'\nreports='reports'\n",
        );
        std::fs::remove_dir_all(dir.join("reports")).expect("reports dir");
        std::fs::write(dir.join("reports"), "not a directory").expect("file");
        assert!(run(&ctx, false).await.is_err());
        std::fs::remove_file(dir.join("reports")).expect("undo");
        std::fs::create_dir_all(dir.join("reports")).expect("reports");
        let stamp = crate::ctx::today();
        std::fs::create_dir(dir.join("reports").join(format!("sync-{stamp}.txt"))).expect("blocks the report");
        ctx.config.sync.as_mut().expect("sync").fetch = Some("nope".into());
        let err = run(&ctx, false).await.expect_err("report");
        assert!(err.to_string().contains("directory") || err.to_string().contains("Directory"), "{err:#}");
        std::fs::remove_dir_all(&dir).expect("cleanup");

        let pdf = include_bytes!("../../../../laccd/tests/fixtures/transcript.pdf");
        let (dir, ctx) = workspace(
            "liaison",
            "inbox='$DIR/inbox'\nprefix='transcript'\ntranscript='laccd.pdf'\nreports='reports'\n",
        );
        let installed = dir.join("profile/laccd.pdf");
        std::fs::write(&installed, "not a transcript").expect("installed");
        std::fs::File::options()
            .write(true)
            .open(&installed)
            .expect("open")
            .set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(120))
            .expect("mtime");
        std::fs::write(dir.join("inbox/transcript-new.pdf"), pdf).expect("new");
        let mut report = String::new();
        let err = sync_inner(&ctx, ctx.config.sync.as_ref().expect("sync"), &dir.join("reports"), &mut report)
            .await
            .expect_err("installed does not parse");
        assert!(err.to_string().contains("pdftotext") || err.to_string().contains("no courses"), "{err:#}");
        std::fs::remove_dir_all(&dir).expect("cleanup");

        let (dir, ctx) = workspace(
            "liaison",
            "inbox='$DIR/inbox'\nprefix='transcript'\ntranscript='blocked/laccd.pdf'\nreports='reports'\n",
        );
        std::fs::write(dir.join("profile/blocked"), "not a directory").expect("blocked");
        std::fs::write(dir.join("inbox/transcript-new.pdf"), pdf).expect("new");
        let mut report = String::new();
        let err = sync_inner(&ctx, ctx.config.sync.as_ref().expect("sync"), &dir.join("reports"), &mut report)
            .await
            .expect_err("parent");
        assert!(err.to_string().contains("File exists") || err.to_string().contains("Not a directory"), "{err:#}");
        std::fs::remove_dir_all(&dir).expect("cleanup");

        let (dir, ctx) = workspace(
            "liaison",
            "inbox='$DIR/inbox'\nprefix='transcript'\ntranscript='laccd.pdf'\nreports='reports'\n",
        );
        std::fs::write(dir.join("profile/achievements.toml"), "not = [").expect("toml");
        let mut report = String::new();
        let err = sync_inner(&ctx, ctx.config.sync.as_ref().expect("sync"), &dir.join("reports"), &mut report)
            .await
            .expect_err("profile");
        let shown = format!("{err:#}");
        assert!(shown.contains("parsing") && shown.contains("NursingCAS not updated"), "{shown}");
        std::fs::remove_dir_all(&dir).expect("cleanup");

        let (dir, ctx) =
            workspace("slate", "inbox='$DIR/inbox'\nprefix='transcript'\ntranscript='laccd.pdf'\nreports='reports'\n");
        let mut report = String::new();
        let err = sync_inner(&ctx, ctx.config.sync.as_ref().expect("sync"), &dir.join("reports"), &mut report)
            .await
            .expect_err("kind");
        assert!(format!("{err:#}").contains("liaison portals only"), "{err:#}");
        std::fs::remove_dir_all(&dir).expect("cleanup");
    })
    .await
}
