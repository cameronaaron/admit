use std::os::unix::fs::PermissionsExt;
use std::process::Command;

use clap::Parser;

use super::*;
#[test]
fn tilde_paths_expand_and_dates_initialize_before_use() {
    assert_eq!(expand(Path::new("~/profile")), PathBuf::from(std::env::var_os("HOME").expect("HOME")).join("profile"));
    init_local_offset();
    assert_eq!(today(), date_at(time::OffsetDateTime::now_utc(), *LOCAL_OFFSET.get().expect("initialized")));
}
#[tokio::test]
async fn a_liaison_config_cannot_be_opened_as_slate() {
    tokio::task::unconstrained(async move {
        let config: Config =
            toml::from_str("[[portal]]\nname='test'\nkind='liaison'\nbase_url='invalid'\n").expect("config");
        let ctx = Ctx { config, base: ".".into(), profile_dir: "missing-profile".into(), portal_name: None };
        match ctx.slate().await {
            Err(e) => assert!(e.to_string().contains("not slate")),
            Ok(_) => panic!("wrong adapter"),
        }
    })
    .await
}

#[test]
fn offsets_bases_and_portal_selection_have_fallbacks() {
    assert_eq!(offset_or(None), time::UtcOffset::UTC);
    assert_eq!(offset_or(Some(time::UtcOffset::UTC)), time::UtcOffset::UTC);
    remember_offset(Err(time::error::IndeterminateOffset));
    assert_eq!(config_base(Path::new("/")), Path::new("."));
    assert_eq!(config_base(Path::new("admit.toml")), Path::new("."));
    assert_eq!(config_base(Path::new("sub/admit.toml")), Path::new("sub"));
    assert_eq!(expand(Path::new("profile")), PathBuf::from("profile"));
    assert!(sections(&["nope".into()]).is_err());
    assert!(sections(&["education".into()]).expect("section").contains(&admit_liaison::Section::Education));
    let dir = std::env::temp_dir().join(format!("admit-ctx-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("sub")).expect("sub");
    std::fs::write(
        dir.join("sub/admit.toml"),
        "[[portal]]\nname='a'\nkind='liaison'\nbase_url='http://example.test'\n",
    )
    .expect("config");
    let config_arg = dir.join("sub/admit.toml");
    let config_arg = config_arg.display().to_string();
    let cli = Cli::try_parse_from(["admit", "-c", config_arg.as_str(), "validate"]).expect("parse");
    let ctx = Ctx::load(&cli).expect("load");
    assert_eq!(ctx.base, dir.join("sub"));
    assert_eq!(ctx.for_portal("a").portal_config().expect("portal").name, "a");
    let missing = Ctx { portal_name: Some("nope".into()), ..ctx.clone_shallow() };
    assert!(missing.portal_config().is_err());
    let empty: Config = toml::from_str("portal=[]").expect("config");
    let bare = Ctx { config: empty, portal_name: None, ..missing };
    assert!(bare.portal_config().err().expect("empty").to_string().contains("no [[portal]]"));
    std::fs::create_dir(dir.join("sub/profile")).expect("profile");
    std::fs::create_dir(dir.join("sub/profile/personal.toml")).expect("not a file");
    let bad = Ctx { profile_dir: dir.join("sub/profile"), ..bare };
    assert!(bad.profile().expect_err("profile").to_string().contains("personal.toml"));
    print_findings(&[admit_domain::Finding::Note("n".into())]);
    std::fs::remove_dir_all(&dir).expect("cleanup");
}

trait Shallow {
    fn clone_shallow(&self) -> Self;
}
impl Shallow for Ctx {
    fn clone_shallow(&self) -> Self {
        Ctx {
            profile_dir: self.profile_dir.clone(),
            config: self.config.clone(),
            portal_name: self.portal_name.clone(),
            base: self.base.clone(),
        }
    }
}

#[test]
fn no_home() {
    if std::env::var_os("ADMIT_NO_HOME").is_some() {
        assert_eq!(expand(Path::new("~/profile")), PathBuf::from("profile"));
        let dir = PathBuf::from(std::env::var_os("ADMIT_CTX_DIR").expect("dir"));
        std::env::set_current_dir(&dir).expect("chdir");
        let cli = Cli::try_parse_from(["admit", "-c", "admit.toml", "validate"]).expect("parse");
        let ctx = Ctx::load(&cli).expect("load");
        assert_eq!(ctx.base, PathBuf::from("."));
        std::fs::write(dir.join("done"), "ok").expect("done");
        return;
    }
    let dir = std::env::temp_dir().join(format!("admit-ctx-home-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("dir");
    std::fs::write(dir.join("admit.toml"), "portal=[]\n").expect("config");
    let current = std::thread::current();
    let name = current.name().expect("test name");
    let out = Command::new(std::env::current_exe().expect("exe"))
        .args(["--exact", name, "--test-threads=1"])
        .env("ADMIT_NO_HOME", "1")
        .env("ADMIT_CTX_DIR", &dir)
        .env_remove("HOME")
        .output()
        .expect("child");
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success() && dir.join("done").is_file(), "{text}");
    std::fs::remove_dir_all(&dir).expect("cleanup");
}

fn portal(
    kind: &str,
    base: &str,
    cookie_env: Option<&str>,
    application: Option<&str>,
    submitted: bool,
) -> PortalConfig {
    PortalConfig {
        name: "s".into(),
        kind: kind.into(),
        base_url: base.into(),
        har_dir: None,
        cookie_env: cookie_env.map(str::to_string),
        application: application.map(str::to_string),
        submitted,
        deadline: None,
        same_as: None,
    }
}

fn host(pc: PortalConfig) -> Ctx {
    Ctx {
        profile_dir: "profile".into(),
        base: ".".into(),
        portal_name: None,
        config: Config { profile: "profile".into(), portals: vec![pc], sync: None, website: None },
    }
}

#[tokio::test]
async fn a_missing_portal_and_an_unread_slate_page_are_errors() {
    tokio::task::unconstrained(async move {
        let bare = Ctx {
            profile_dir: "profile".into(),
            base: ".".into(),
            portal_name: None,
            config: Config { profile: "profile".into(), portals: vec![], sync: None, website: None },
        };
        match bare.slate().await {
            Err(e) => assert!(e.to_string().contains("no [[portal]]"), "{e}"),
            Ok(_) => panic!("slate opened with no portal"),
        }
        match bare.portal().await {
            Err(e) => assert!(e.to_string().contains("no [[portal]]"), "{e}"),
            Ok(_) => panic!("portal opened with no portal"),
        }
        let dead = "http://127.0.0.1:1";
        let mut client = admit_slate::Client::new(dead, "_uid=x".into());
        let err = open_application(&portal("slate", dead, None, None, true), &mut client).await.expect_err("status");
        assert!(err.to_string().to_lowercase().contains("connect") || err.to_string().contains("127.0.0.1"), "{err}");
        let err = open_application(&portal("slate", dead, None, None, false), &mut client).await.expect_err("list");
        assert!(err.to_string().to_lowercase().contains("connect") || err.to_string().contains("127.0.0.1"), "{err}");
    })
    .await
}

async fn sign_in_mode(mode: &str) {
    let server = if mode == "liaison" { None } else { Some(wiremock::MockServer::start().await) };
    if let Some(server) = &server {
        if mode == "expired" {
            wiremock::Mock::given(wiremock::matchers::method("POST"))
                .and(wiremock::matchers::path("/apply/"))
                .respond_with(
                    wiremock::ResponseTemplate::new(302)
                        .insert_header("location", format!("{}/account/login", server.uri())),
                )
                .mount(server)
                .await;
        } else {
            wiremock::Mock::given(wiremock::matchers::method("GET"))
                .and(wiremock::matchers::path("/account/login"))
                .respond_with(wiremock::ResponseTemplate::new(200))
                .mount(server)
                .await;
            wiremock::Mock::given(wiremock::matchers::method("POST"))
                .and(wiremock::matchers::path("/account/login"))
                .respond_with(
                    wiremock::ResponseTemplate::new(302)
                        .insert_header("set-cookie", "_uid=abc; Path=/")
                        .insert_header("location", "/apply/"),
                )
                .mount(server)
                .await;
            wiremock::Mock::given(wiremock::matchers::method("POST"))
                .and(wiremock::matchers::path("/apply/"))
                .respond_with(wiremock::ResponseTemplate::new(500).set_body_string("no"))
                .mount(server)
                .await;
        }
    }
    let base = server.as_ref().map(|s| s.uri()).unwrap_or_else(|| "http://127.0.0.1:1".into());
    let ctx = host(match mode {
        "liaison" => portal("liaison", &base, None, None, false),
        "expired" => portal("slate", &base, Some("ADMIT_CTX_COOKIE"), Some("app"), false),
        _ => portal("slate", &base, None, Some("app"), false),
    });
    let err = match mode {
        "liaison" => match ctx.portal().await {
            Err(e) => e,
            Ok(_) => panic!("liaison login succeeded"),
        },
        _ => match ctx.slate().await {
            Err(e) => e,
            Ok(_) => panic!("slate login succeeded"),
        },
    };
    let text = format!("{err:#}");
    let expected = match mode {
        "expired" => "sign-in rejected",
        "reopen" => "HTTP 500",
        _ => "transport",
    };
    assert!(text.contains(expected), "{mode}: {text}");
    if mode == "expired" {
        let reqs = server.as_ref().expect("server").received_requests().await.expect("requests");
        let paths: Vec<String> = reqs.iter().map(|r| format!("{} {}", r.method, r.url.path())).collect();
        assert!(paths.iter().any(|p| p.contains("POST") && p.contains("apply")), "{paths:?}\n{text}");
    }
}

#[tokio::test]
async fn sign_in_errors() {
    tokio::task::unconstrained(async move {
        if let Ok(mode) = std::env::var("ADMIT_CTX_MODE") {
            sign_in_mode(&mode).await;
            let dir = PathBuf::from(std::env::var_os("ADMIT_CTX_DIR").expect("dir"));
            std::fs::write(dir.join("done"), &mode).expect("done");
            return;
        }
        let dir = std::env::temp_dir().join(format!("admit-ctx-signin-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let empty = dir.join("empty");
        std::fs::create_dir_all(&empty).expect("empty");
        let current = std::thread::current();
        let name = current.name().expect("test name");
        for mode in ["expired", "reopen", "liaison"] {
            let _ = std::fs::remove_file(dir.join("done"));
            let mut cmd = Command::new(std::env::current_exe().expect("exe"));
            cmd.args(["--exact", name, "--test-threads=1"])
                .env("ADMIT_CTX_MODE", mode)
                .env("ADMIT_CTX_DIR", &dir)
                .env("PATH", &empty)
                .env("S_USER", "user")
                .env("S_PASSWORD", "secret")
                .env_remove("ADMIT_CTX_COOKIE");
            if mode == "expired" {
                cmd.env("ADMIT_CTX_COOKIE", "_uid=abc");
            }
            let out = cmd.output().expect("child");
            let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
            assert!(out.status.success() && dir.join("done").is_file(), "{mode}: {text}");
        }
        std::fs::remove_dir_all(&dir).expect("cleanup");
    })
    .await
}

#[test]
fn only_an_expired_slate_session_is_replaced() {
    let expired = anyhow::Error::from(admit_slate::Error::Expired { path: "/apply/".into() });
    assert!(slate_expired(&expired));
    assert!(!slate_expired(&anyhow::Error::msg("other")));
}

fn har_file(dir: &Path, url: &str) {
    let har = dir.join("har");
    std::fs::create_dir_all(&har).expect("har");
    let body = format!(
        r#"{{"log":{{"entries":[{{"request":{{"url":"{url}","headers":[{{"name":"Cookie","value":"_uid=har"}}]}}}}]}}}}"#
    );
    std::fs::write(har.join("session.har"), body).expect("write");
}

async fn mount_slate_login(server: &wiremock::MockServer) {
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path("/account/login"))
        .respond_with(wiremock::ResponseTemplate::new(200))
        .mount(server)
        .await;
    wiremock::Mock::given(wiremock::matchers::method("POST"))
        .and(wiremock::matchers::path("/account/login"))
        .respond_with(
            wiremock::ResponseTemplate::new(302)
                .insert_header("set-cookie", "_uid=abc; Path=/")
                .insert_header("location", "/apply/"),
        )
        .mount(server)
        .await;
}

async fn mount_liaison_login(server: &wiremock::MockServer) {
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path("/applicant/initialize"))
        .respond_with(wiremock::ResponseTemplate::new(200))
        .mount(server)
        .await;
    wiremock::Mock::given(wiremock::matchers::method("POST"))
        .and(wiremock::matchers::path("/applicant/authenticate"))
        .respond_with(
            wiremock::ResponseTemplate::new(200)
                .insert_header("set-cookie", "SESSION=fresh; Path=/")
                .set_body_string("{}"),
        )
        .mount(server)
        .await;
}

fn open_ok(id: &str) -> wiremock::ResponseTemplate {
    wiremock::ResponseTemplate::new(200).insert_header("set-cookie", format!(".application={id}; Path=/"))
}

async fn session_shapes(mode: &str) {
    let offline = matches!(mode, "not-slate" | "liaison-kind" | "no-creds" | "nav-none" | "har-bad" | "nav-har-bad");
    let server = if offline { None } else { Some(wiremock::MockServer::start().await) };
    let base = server.as_ref().map(|s| s.uri()).unwrap_or_else(|| "http://127.0.0.1:1".into());
    let dir = PathBuf::from(std::env::var_os("ADMIT_CTX_DIR").expect("dir"));
    if mode == "har-ok" {
        har_file(&dir, &format!("{base}/apply/page"));
    } else if mode == "nav-har" {
        har_file(&dir, &format!("{base}/applicant/api/navigation"));
    } else if mode == "har-bad" || mode == "nav-har-bad" {
        std::fs::create_dir_all(dir.join("har")).expect("har");
    }
    if let Some(server) = &server {
        match mode {
            "har-ok" | "open-ok" => {
                wiremock::Mock::given(wiremock::matchers::method("POST"))
                    .and(wiremock::matchers::path("/apply/"))
                    .respond_with(open_ok("app"))
                    .mount(server)
                    .await;
            }
            "status-ok" => {
                wiremock::Mock::given(wiremock::matchers::method("GET"))
                    .and(wiremock::matchers::path("/apply/status"))
                    .respond_with(wiremock::ResponseTemplate::new(200).set_body_string("ok"))
                    .mount(server)
                    .await;
            }
            "one-app" | "two-apps" | "zero-apps" => {
                let html = match mode {
                    "one-app" => "<table><tr data-href='?id=only&cmd=detail'><td>Only</td><td>Open</td></tr></table>",
                    "two-apps" => {
                        "<table><tr data-href='?id=a&cmd=detail'><td>A</td><td>Open</td></tr><tr data-href='?id=b&cmd=detail'><td>B</td><td>Open</td></tr></table>"
                    }
                    _ => "<table></table>",
                };
                wiremock::Mock::given(wiremock::matchers::method("GET"))
                    .and(wiremock::matchers::path("/apply/"))
                    .respond_with(wiremock::ResponseTemplate::new(200).set_body_string(html))
                    .mount(server)
                    .await;
                if mode == "one-app" {
                    wiremock::Mock::given(wiremock::matchers::method("POST"))
                        .and(wiremock::matchers::path("/apply/"))
                        .respond_with(open_ok("only"))
                        .mount(server)
                        .await;
                }
            }
            "http-err" => {
                wiremock::Mock::given(wiremock::matchers::method("POST"))
                    .and(wiremock::matchers::path("/apply/"))
                    .respond_with(wiremock::ResponseTemplate::new(500).set_body_string("no"))
                    .mount(server)
                    .await;
            }
            "resign" => {
                mount_slate_login(server).await;
                wiremock::Mock::given(wiremock::matchers::method("POST"))
                    .and(wiremock::matchers::path("/apply/"))
                    .respond_with(open_ok("app"))
                    .mount(server)
                    .await;
            }
            "nav-ok" | "nav-har" => {
                wiremock::Mock::given(wiremock::matchers::method("GET"))
                    .and(wiremock::matchers::path("/applicant/api/navigation"))
                    .respond_with(wiremock::ResponseTemplate::new(200).set_body_string("{}"))
                    .mount(server)
                    .await;
            }
            "nav-expired" | "nav-reauth" | "nav-kept" => {
                wiremock::Mock::given(wiremock::matchers::method("GET"))
                    .and(wiremock::matchers::path("/applicant/api/navigation"))
                    .respond_with(wiremock::ResponseTemplate::new(401).set_body_string("no"))
                    .mount(server)
                    .await;
                if mode == "nav-reauth" || mode == "nav-kept" {
                    mount_liaison_login(server).await;
                }
            }
            "nav-http" => {
                wiremock::Mock::given(wiremock::matchers::method("GET"))
                    .and(wiremock::matchers::path("/applicant/api/navigation"))
                    .respond_with(wiremock::ResponseTemplate::new(500).set_body_string("no"))
                    .mount(server)
                    .await;
            }
            _ => {}
        }
    }
    if mode == "liaison-kind" {
        let ctx = host(portal("slate", &base, None, None, false));
        match ctx.portal().await {
            Err(e) => assert!(e.to_string().contains("liaison"), "{e}"),
            Ok(_) => panic!("slate opened as liaison"),
        }
        std::fs::write(dir.join("done"), mode).expect("done");
        return;
    }
    let kind = if mode == "not-slate" || mode.starts_with("nav") { "liaison" } else { "slate" };
    let mut pc = portal(
        kind,
        &base,
        Some("ADMIT_CTX_COOKIE"),
        matches!(mode, "har-ok" | "open-ok" | "http-err" | "resign").then_some("app"),
        mode == "status-ok",
    );
    if matches!(mode, "har-ok" | "nav-har" | "har-bad" | "nav-har-bad") {
        pc.har_dir = Some(dir.join("har"));
    }
    let ctx = host(pc);
    let needle = match mode {
        "har-ok" | "open-ok" | "status-ok" | "one-app" | "resign" | "nav-ok" | "nav-expired" | "nav-reauth"
        | "nav-kept" | "nav-har" => None,
        "not-slate" => Some("not slate"),
        "no-creds" | "nav-none" => Some("no session"),
        "har-bad" | "nav-har-bad" => Some("no .har"),
        "two-apps" | "zero-apps" => Some("applications"),
        "http-err" | "nav-http" => Some("HTTP 500"),
        other => panic!("unknown mode {other}"),
    };
    let failed = if mode.starts_with("nav") {
        match ctx.portal().await {
            Ok(_) => None,
            Err(e) => Some(format!("{e:#}")),
        }
    } else {
        match ctx.slate().await {
            Ok(_) => None,
            Err(e) => Some(format!("{e:#}")),
        }
    };
    match (needle, failed) {
        (None, None) => {}
        (Some(n), Some(text)) => assert!(text.contains(n), "{mode}: {text}"),
        (None, Some(text)) => panic!("{mode}: {text}"),
        (Some(n), None) => panic!("{mode}: opened, wanted {n}"),
    }
    std::fs::write(dir.join("done"), mode).expect("done");
}

#[tokio::test]
async fn session_shapes_cover_sign_in_and_har() {
    tokio::task::unconstrained(async move {
        if let Ok(mode) = std::env::var("ADMIT_CTX_MODE") {
            session_shapes(&mode).await;
            return;
        }
        let dir = std::env::temp_dir().join(format!("admit-ctx-shapes-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let empty = dir.join("empty");
        let bin = dir.join("bin");
        std::fs::create_dir_all(&empty).expect("empty");
        std::fs::create_dir_all(&bin).expect("bin");
        std::fs::write(bin.join("security"), "#!/bin/sh\nexit 0\n").expect("security");
        std::fs::set_permissions(bin.join("security"), std::fs::Permissions::from_mode(0o755)).expect("mode");
        let current = std::thread::current();
        let name = current.name().expect("test name");
        let creds = ["http-err", "resign", "nav-reauth", "nav-kept", "two-apps"];
        let cookie = [
            "open-ok",
            "status-ok",
            "one-app",
            "two-apps",
            "zero-apps",
            "http-err",
            "nav-ok",
            "nav-expired",
            "nav-reauth",
            "nav-kept",
            "nav-http",
        ];
        for mode in [
            "not-slate",
            "liaison-kind",
            "no-creds",
            "nav-none",
            "har-bad",
            "nav-har-bad",
            "har-ok",
            "nav-har",
            "open-ok",
            "status-ok",
            "one-app",
            "two-apps",
            "zero-apps",
            "http-err",
            "resign",
            "nav-ok",
            "nav-expired",
            "nav-reauth",
            "nav-kept",
            "nav-http",
        ] {
            let _ = std::fs::remove_file(dir.join("done"));
            let mut cmd = Command::new(std::env::current_exe().expect("exe"));
            cmd.args(["--exact", name, "--test-threads=1"])
                .env("ADMIT_CTX_MODE", mode)
                .env("ADMIT_CTX_DIR", &dir)
                .env("PATH", if mode == "resign" || mode == "nav-kept" { &bin } else { &empty })
                .env_remove("ADMIT_CTX_COOKIE")
                .env_remove("S_USER")
                .env_remove("S_PASSWORD");
            if creds.contains(&mode) {
                cmd.env("S_USER", "user").env("S_PASSWORD", "secret");
            }
            if cookie.contains(&mode) {
                cmd.env("ADMIT_CTX_COOKIE", "_uid=abc");
            }
            let out = cmd.output().expect("child");
            let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
            assert!(out.status.success() && dir.join("done").is_file(), "{mode}: {text}");
        }
        std::fs::remove_dir_all(&dir).expect("cleanup");
    })
    .await
}
