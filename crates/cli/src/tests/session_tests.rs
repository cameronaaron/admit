use std::io::Read;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

#[test]
fn env_names_are_upper_snake() {
    assert_eq!(super::env_names("nursingcas"), ("NURSINGCAS_USER".into(), "NURSINGCAS_PASSWORD".into()));
    assert_eq!(super::env_names("pharm-cas"), ("PHARM_CAS_USER".into(), "PHARM_CAS_PASSWORD".into()));
}

struct Boom;
impl Read for Boom {
    fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
        Err(std::io::Error::other("closed"))
    }
}

#[test]
fn stdin_that_is_not_a_secret_is_rejected_before_the_keychain() {
    let err = super::cookie_from_reader("portal", Boom).expect_err("read");
    assert!(err.to_string().contains("reading the cookie"), "{err}");
    let err = super::cookie_from_reader("portal", std::io::Cursor::new("not a cookie")).expect_err("shape");
    assert!(err.to_string().contains("Cookie header"), "{err}");
    let err = super::credentials_from_reader("portal", Boom).expect_err("read");
    assert!(err.to_string().contains("reading credentials"), "{err}");
    for raw in ["", "only-user\n", "\npassword\n"] {
        let err = super::credentials_from_reader("portal", std::io::Cursor::new(raw)).expect_err("lines");
        assert!(err.to_string().contains("two lines"), "{raw}: {err}");
    }
}

fn mode(dir: &Path, name: &str) {
    std::fs::write(dir.join("bin/mode"), name).expect("mode");
}

fn spawn(label: &str, path: &Path, env: &[(&str, Option<&str>)], flag: &str) {
    let current = std::thread::current();
    let name = current.name().expect("test name");
    let mut cmd = Command::new(std::env::current_exe().expect("exe"));
    cmd.args(["--exact", name, "--test-threads=1"])
        .env(flag, "1")
        .env("ADMIT_SESSION_DIR", path.parent().unwrap_or(path))
        .env("PATH", path)
        .env_remove("LACCD_USER")
        .env_remove("LACCD_PASSWORD")
        .env_remove("PORTAL_USER")
        .env_remove("PORTAL_PASSWORD");
    for (k, v) in env {
        match v {
            Some(v) => {
                cmd.env(k, v);
            }
            None => {
                cmd.env_remove(k);
            }
        }
    }
    let out = cmd.output().expect("child");
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    let dir = path.parent().unwrap_or(path);
    assert!(out.status.success() && dir.join("done").is_file(), "{label}: {text}");
}

#[test]
fn no_security_binary() {
    if std::env::var_os("ADMIT_SESSION_ABSENT").is_some() {
        assert!(super::get("portal").is_none());
        let err = super::clear("portal").expect_err("clear");
        assert!(err.to_string().contains("os error"), "{err}");
        let err = super::put("portal", "SESSION=abc").expect_err("put");
        assert!(err.to_string().contains("running `security`"), "{err}");
        let dir = PathBuf::from(std::env::var_os("ADMIT_SESSION_DIR").expect("dir"));
        std::fs::write(dir.join("done"), "ok").expect("done");
        return;
    }
    let dir = std::env::temp_dir().join(format!("admit-session-absent-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let empty = dir.join("empty");
    std::fs::create_dir_all(&empty).expect("empty");
    spawn("absent", &empty, &[], "ADMIT_SESSION_ABSENT");
    std::fs::remove_dir_all(&dir).expect("cleanup");
}

#[test]
fn fake_keychain() {
    if std::env::var_os("ADMIT_SESSION_SCRIPTS").is_some() {
        let dir = PathBuf::from(std::env::var_os("ADMIT_SESSION_DIR").expect("dir"));
        if std::env::var("LACCD_USER").ok().as_deref() == Some("env-user") {
            let creds = super::laccd().expect("env creds");
            assert_eq!((creds.user.as_str(), creds.password.as_str()), ("env-user", "env-pass"));
            std::fs::write(dir.join("done"), "ok").expect("done");
            return;
        }
        if std::env::var("PORTAL_USER").ok().as_deref() == Some("env-user") {
            mode(&dir, "deny");
            assert!(super::credentials("portal").is_none(), "an empty password is not a sign-in");
            std::fs::write(dir.join("done"), "ok").expect("done");
            return;
        }
        mode(&dir, "deny");
        assert!(super::get("portal").is_none());
        let err = super::clear("portal").expect_err("clear");
        assert!(err.to_string().contains("no stored session"), "{err}");
        let err = super::put("portal", "SESSION=abc").expect_err("put");
        assert!(err.to_string().contains("add-generic-password"), "{err}");
        mode(&dir, "empty");
        assert!(super::get("portal").is_none(), "empty stdout is not a cookie");
        mode(&dir, "cookie");
        assert_eq!(super::get("portal").as_deref(), Some("SESSION=abc"));
        super::clear("portal").expect("clear");
        super::put("portal", "SESSION=abc").expect("put");
        let cookie =
            super::cookie_from_reader("portal", std::io::Cursor::new("Cookie: SESSION=from-stdin\n")).expect("set");
        assert!(cookie.contains("SESSION=from-stdin"), "{cookie}");
        let cookie = super::cookie_from_reader("portal", std::io::Cursor::new("cookie: a=b")).expect("prefix");
        assert_eq!(cookie, "a=b");
        mode(&dir, "fail");
        let err = super::cookie_from_reader("portal", std::io::Cursor::new("a=b")).expect_err("store");
        assert!(err.to_string().contains("add-generic-password"), "{err}");
        let err = super::credentials_from_reader("portal", std::io::Cursor::new("user\nsecret\n")).expect_err("creds");
        assert!(err.to_string().contains("add-generic-password"), "{err}");
        mode(&dir, "half");
        let err = super::credentials_from_reader("portal", std::io::Cursor::new("user\nsecret\n")).expect_err("half");
        assert!(err.to_string().contains("portal-password"), "the password store failing is reported: {err}");
        mode(&dir, "cookie");
        let stored = super::credentials_from_reader("portal", std::io::Cursor::new("user\nsecret\n")).expect("stored");
        assert_eq!(stored, ("user".to_string(), "secret".to_string()));
        mode(&dir, "user-only");
        assert!(super::credentials("portal").is_none(), "password item missing");
        mode(&dir, "both");
        assert_eq!(
            super::credentials("portal").as_ref().map(|(u, p)| (u.as_str(), p.as_str())),
            Some(("user", "secret"))
        );
        std::fs::write(dir.join("done"), "ok").expect("done");
        return;
    }
    let dir = std::env::temp_dir().join(format!("admit-session-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let bin = dir.join("bin");
    std::fs::create_dir_all(&bin).expect("bin");
    let script = r#"mode=$(/bin/cat "${0%/*}/mode")
case "$1" in
  find-generic-password)
    case "$mode" in
      empty) exit 0 ;;
      cookie) printf '%s' 'SESSION=abc' ;;
      user-only) case "$5" in *-user) printf '%s' user ;; *) exit 1 ;; esac ;;
      both) case "$5" in *-user) printf '%s' user ;; *-password) printf '%s' secret ;; *) exit 1 ;; esac ;;
      *) exit 1 ;;
    esac ;;
  *) case "$mode" in deny|fail) exit 1 ;; half) case "$6" in *-password) exit 1 ;; esac ;; *) exit 0 ;; esac ;;
esac"#;
    std::fs::write(bin.join("security"), format!("#!/bin/sh\n{script}\n")).expect("script");
    std::fs::set_permissions(bin.join("security"), std::fs::Permissions::from_mode(0o755)).expect("mode");
    mode(&dir, "deny");
    spawn("scripts", &bin, &[], "ADMIT_SESSION_SCRIPTS");
    spawn(
        "env",
        &bin,
        &[("LACCD_USER", Some("env-user")), ("LACCD_PASSWORD", Some("env-pass"))],
        "ADMIT_SESSION_SCRIPTS",
    );
    spawn(
        "empty-password",
        &bin,
        &[("PORTAL_USER", Some("env-user")), ("PORTAL_PASSWORD", Some(""))],
        "ADMIT_SESSION_SCRIPTS",
    );
    std::fs::remove_dir_all(&dir).expect("cleanup");
}
