use super::*;
use std::os::unix::fs::PermissionsExt;
use wiremock::matchers::{method, path};
use wiremock::{Mock, ResponseTemplate};
#[path = "../../../../laccd/tests/support/mod.rs"]
mod support;
#[tokio::test]
async fn credentials_and_downloads_use_the_same_portal_flow() {
    tokio::task::unconstrained(async move {
        let pdf = include_bytes!("../../../../laccd/tests/fixtures/transcript.pdf").to_vec();
        let s = support::portal(pdf.clone()).await;
        let ep = admit_laccd::Endpoints { sso: s.uri(), campus: s.uri() };
        let creds = admit_laccd::Credentials { user: "escsso\\123".into(), password: "right pass".into() };
        execute(&LaccdCmd::Login, ep.clone(), &creds).await.expect("login");
        assert_eq!(download_with(ep.clone(), &creds).await.expect("download"), pdf);
        let dir = std::env::temp_dir().join(format!("admit-laccd-command-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let out = dir.join("transcript.pdf");
        execute(&LaccdCmd::Fetch { out: Some(out.clone()) }, ep.clone(), &creds).await.expect("fetch");
        assert_eq!(std::fs::read(&out).expect("pdf"), pdf);
        assert!(
            execute(&LaccdCmd::Fetch { out: Some(dir.join("absent/transcript.pdf")) }, ep.clone(), &creds)
                .await
                .expect_err("write fails")
                .to_string()
                .contains("writing")
        );
        assert_eq!(
            destination(None, time::macros::date!(2026 - 10 - 06)),
            std::path::PathBuf::from("LAC_SR_TSRPT-2026-10-06.pdf")
        );
        assert_eq!(destination(Some(&out), time::macros::date!(2026 - 10 - 06)), out);
        let bad = admit_laccd::Credentials { user: "escsso\\123".into(), password: "wrong".into() };
        assert!(
            execute(&LaccdCmd::Login, ep.clone(), &bad).await.expect_err("rejected").to_string().contains("rejected")
        );
        assert!(
            execute(&LaccdCmd::Fetch { out: Some(dir.join("nope.pdf")) }, ep.clone(), &bad)
                .await
                .expect_err("fetch rejected")
                .to_string()
                .contains("rejected")
        );
        assert!(download_with(ep.clone(), &bad).await.expect_err("download rejected").to_string().contains("rejected"));
        Mock::given(method("GET"))
            .and(path("/psc/csprd_5/view/%7bV2%7dabc/LAC_SR_TSRPT.pdf"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("Content-Type", "application/pdf")
                    .set_body_bytes(b"%PDF-not-a-transcript".to_vec()),
            )
            .with_priority(1)
            .mount(&s)
            .await;
        let unreadable = dir.join("unreadable.pdf");
        let err =
            execute(&LaccdCmd::Fetch { out: Some(unreadable.clone()) }, ep, &creds).await.expect_err("parse fails");
        let msg = format!("{err:#}");
        assert!(msg.contains("pdftotext") || msg.contains("no courses") || msg.contains("did not parse"), "{msg}");
        assert!(unreadable.exists(), "a transcript that does not parse is still written");
        std::fs::remove_dir_all(dir).expect("cleanup");
    })
    .await
}

/// `security` is a stub that fails, so the Keychain is never the user's.
/// Credentials fail before `execute` opens a connection.
#[test]
fn a_missing_sign_in_fails_before_the_portal_is_contacted() {
    if std::env::var_os("ADMIT_LACCD_CHILD").is_some() {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");
        rt.block_on(async {
            let err = laccd_credentials().err().expect("none stored");
            assert!(format!("{err:#}").contains("no LACCD sign-in"), "{err:#}");
            let err = run(&LaccdCmd::Fetch { out: None }).await.expect_err("fetch");
            assert!(format!("{err:#}").contains("no LACCD sign-in"), "{err:#}");
            let err = download_transcript().await.expect_err("download");
            assert!(format!("{err:#}").contains("no LACCD sign-in"), "{err:#}");
            let err = run(&LaccdCmd::Login).await.expect_err("login");
            assert!(err.to_string().contains("two lines"), "{err:#}");
        });
        let dir = std::env::var("ADMIT_LACCD_DIR").expect("dir");
        std::fs::write(format!("{dir}/done"), "ok").expect("done");
        return;
    }
    let dir = std::env::temp_dir().join(format!("admit-laccd-child-{}", std::process::id()));
    let bin = dir.join("bin");
    std::fs::create_dir_all(&bin).expect("bin");
    let security = bin.join("security");
    std::fs::write(&security, "#!/bin/sh\nexit 1\n").expect("script");
    std::fs::set_permissions(&security, std::fs::Permissions::from_mode(0o755)).expect("mode");
    let current = std::thread::current();
    let name = current.name().expect("test name");
    let out = std::process::Command::new(std::env::current_exe().expect("exe"))
        .args(["--exact", name, "--test-threads=1"])
        .env("ADMIT_LACCD_CHILD", "1")
        .env("ADMIT_LACCD_DIR", &dir)
        .env("PATH", &bin)
        .env_remove("LACCD_USER")
        .env_remove("LACCD_PASSWORD")
        .stdin(std::process::Stdio::null())
        .output()
        .expect("child");
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success() && dir.join("done").is_file(), "{text}");
    std::fs::remove_dir_all(dir).expect("cleanup");
}

/// With a stored sign-in, `run` and `download_transcript` go to LACCD's own
/// hosts. The child's `HTTPS_PROXY` is a local listener that records each
/// CONNECT and hangs up, so nothing leaves the machine.
#[test]
fn a_stored_sign_in_goes_to_laccd_hosts() {
    if std::env::var_os("ADMIT_LACCD_CHILD").is_some() {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");
        rt.block_on(async {
            let err = run(&LaccdCmd::Fetch { out: None }).await.expect_err("proxy hangs up");
            assert!(format!("{err:#}").contains("sign-in"), "{err:#}");
            download_transcript().await.expect_err("proxy hangs up");
        });
        return;
    }
    use std::io::BufRead;
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("listener");
    let addr = listener.local_addr().expect("address");
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let proxy = {
        let (seen, done) = (seen.clone(), done.clone());
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                if done.load(std::sync::atomic::Ordering::SeqCst) {
                    return;
                }
                let mut line = String::new();
                let _ = std::io::BufReader::new(stream.expect("connection")).read_line(&mut line);
                seen.lock().expect("lock").push(line.trim_end().to_string());
            }
        })
    };
    let current = std::thread::current();
    let name = current.name().expect("test name");
    let out = std::process::Command::new(std::env::current_exe().expect("exe"))
        .args(["--exact", name, "--test-threads=1"])
        .env("ADMIT_LACCD_CHILD", "1")
        .env("LACCD_USER", "escsso\\123")
        .env("LACCD_PASSWORD", "pass")
        .env("HTTPS_PROXY", format!("http://{addr}"))
        .env_remove("NO_PROXY")
        .env_remove("no_proxy")
        .output()
        .expect("child");
    done.store(true, std::sync::atomic::Ordering::SeqCst);
    let _ = std::net::TcpStream::connect(addr);
    proxy.join().expect("proxy thread");
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success(), "{text}");
    let seen = seen.lock().expect("lock").clone();
    assert_eq!(seen, vec!["CONNECT sso.laccd.edu:443 HTTP/1.1"; 2], "{text}");
}
