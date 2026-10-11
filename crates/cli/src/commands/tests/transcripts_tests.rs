use std::path::{Path, PathBuf};
use std::process::Command;

use admit_domain::orders::{Order, Stage};
use admit_domain::{Finding, TranscriptOrdersFile, TranscriptSend};
use admit_parchment::Endpoints;
use serde_json::json;
use wiremock::matchers::{body_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::*;
use crate::ctx::{Config, Ctx};

fn fixture(krate: &str, name: &str) -> String {
    std::fs::read_to_string(format!("{}/../{krate}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR")))
        .expect("fixture")
}

fn send(via: &str, from: &str, to: &str, order: Option<&str>) -> TranscriptSend {
    TranscriptSend { from: from.into(), to: to.into(), via: via.into(), order: order.map(Into::into) }
}

fn sent(via: &'static str, id: &str, from: &str, to: &str) -> Order {
    Order {
        via,
        id: id.into(),
        from: from.into(),
        to: to.into(),
        status: "COMPLETE".into(),
        ordered: "2026-07-24".into(),
        changed: None,
        stage: Stage::Sent,
    }
}

/// A Clearinghouse that knows order 97852158 for a@b.c and refuses the rest.
async fn clearinghouse() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/tsorder/trackorder"))
        .and(body_json(json!({"email": "a@b.c", "toOrderId": "97852158"})))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("Authorization-Token", "Bearer t")
                .set_body_raw(fixture("clearinghouse", "trackorder.json"), "application/json"),
        )
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/tsorder/trackorder"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"status": "fail", "message": "not found"})))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/tsorder/trackorder/97852158/recipient"))
        .respond_with(
            ResponseTemplate::new(200).set_body_raw(fixture("clearinghouse", "recipient.json"), "application/json"),
        )
        .mount(&server)
        .await;
    server
}

#[test]
fn violations_print_first_then_pending_then_notes() {
    let out = render(&[Finding::Note("n".into()), Finding::Pending("p".into()), Finding::Violation("v".into())]);
    assert_eq!(out, "VIOLATION  v\nPENDING    p\nnote       n\n");
}

#[test]
fn the_code_prompt_reads_one_line() {
    struct Tty(std::io::Cursor<Vec<u8>>, Vec<u8>);
    impl std::io::Read for Tty {
        fn read(&mut self, b: &mut [u8]) -> std::io::Result<usize> {
            self.0.read(b)
        }
    }
    impl std::io::Write for Tty {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            self.1.extend_from_slice(b);
            Ok(b.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let shown = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    struct Shared(Tty, std::rc::Rc<std::cell::RefCell<Vec<u8>>>);
    impl std::io::Read for Shared {
        fn read(&mut self, b: &mut [u8]) -> std::io::Result<usize> {
            self.0.read(b)
        }
    }
    impl std::io::Write for Shared {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            self.1.borrow_mut().extend_from_slice(b);
            self.0.write(b)
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let tty = Shared(Tty(std::io::Cursor::new(b" 123456\nrest".to_vec()), vec![]), shown.clone());
    assert_eq!(ask_code(Some(Box::new(tty))).as_deref(), Some("123456"));
    assert!(String::from_utf8_lossy(&shown.borrow()).contains("enter it"));
    assert_eq!(ask_code(Some(Box::new(Tty(std::io::Cursor::new(b"\n".to_vec()), vec![])))), None);
    assert_eq!(ask_code(None), None);
    struct Broken;
    impl std::io::Read for Broken {
        fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("gone"))
        }
    }
    impl std::io::Write for Broken {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("gone"))
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    assert_eq!(ask_code(Some(Box::new(Broken))), None);
    // A terminal is any file opened read-write; none at the path is nobody to ask.
    let file = std::env::temp_dir().join(format!("admit-tty-{}", std::process::id()));
    // A file shares one offset between the prompt and the answer, as a
    // terminal does not: the code sits after room for the prompt.
    std::fs::write(&file, format!("{}654321\n", " ".repeat(PROMPT.len()))).expect("tty");
    assert_eq!(ask_code(terminal(&file)).as_deref(), Some("654321"));
    std::fs::remove_file(&file).expect("cleanup");
    assert!(terminal(&file).is_none());
    // Under the test harness there is no terminal: nobody to ask.
    if std::fs::File::open("/dev/tty").is_err() {
        assert_eq!(tty_code(), None);
    }
}

#[tokio::test]
async fn nothing_declared_is_said_plainly() {
    tokio::task::unconstrained(async {
        let f = findings(&TranscriptOrdersFile::default(), "http://127.0.0.1:9", None).await;
        assert_eq!(f, vec![Finding::Note("transcript_orders.toml declares no transcripts to send".into())]);
    })
    .await
}

#[tokio::test]
async fn an_unreadable_service_is_unchecked_and_its_sends_are_not_called_unordered() {
    tokio::task::unconstrained(async {
        let ch = clearinghouse().await;
        let file = TranscriptOrdersFile {
            email: " a@b.c ".into(),
            sends: vec![
                send("clearinghouse", "Connecticut College", "Azusa Pacific University", Some("97852158")),
                send("clearinghouse", "Bridges", "Azusa Pacific University", Some(" 1 ")),
                send("clearinghouse", "Elmbridge", "UCLA", None),
                send("parchment", "LACC", "NursingCAS", None),
                send("fax", "A", "B", None),
            ],
        };
        assert!(uses_parchment(&file));
        let f = findings(&file, &ch.uri(), Some(Err(anyhow::anyhow!("signed out")))).await;
        assert_eq!(
            f,
            vec![
                Finding::Violation("UNCHECKED: Clearinghouse order 1: not found".into()),
                Finding::Violation("UNCHECKED: parchment: signed out".into()),
                Finding::Note(
                    "transcript Connecticut College -> Azusa Pacific University: Electronic Transcript Sent on 2026-10-10 (clearinghouse 97852158)"
                        .into()
                ),
                Finding::Pending(
                    "transcript Elmbridge -> UCLA: not ordered (or its Clearinghouse order number is not recorded: order = \"...\")"
                        .into()
                ),
                Finding::Violation("transcript A -> B: via \"fax\" is not one of [\"clearinghouse\", \"parchment\"]".into()),
            ]
        );
    })
    .await
}

#[tokio::test]
async fn clearinghouse_orders_need_the_order_email() {
    tokio::task::unconstrained(async {
        let file = TranscriptOrdersFile {
            email: " ".into(),
            sends: vec![send("clearinghouse", "A", "B", Some("1")), send("parchment", "LACC", "NursingCAS", None)],
        };
        let f =
            findings(&file, "http://127.0.0.1:9", Some(Ok(vec![sent("parchment", "TR1", "LACC", "NursingCAS")]))).await;
        assert!(matches!(&f[0], Finding::Violation(m) if m.contains("needs `email`")), "{f:?}");
        assert!(matches!(&f[1..], [Finding::Note(m)] if m.contains("parchment TR1")), "{f:?}");
    })
    .await
}

fn ctx(dir: &Path) -> Ctx {
    Ctx {
        profile_dir: dir.to_path_buf(),
        base: dir.to_path_buf(),
        portal_name: None,
        config: Config { profile: "profile".into(), portals: vec![], sync: None, website: None },
    }
}

#[tokio::test]
async fn the_command_fails_only_on_a_violation() {
    tokio::task::unconstrained(async {
        let ch = clearinghouse().await;
        let dir = std::env::temp_dir().join(format!("admit-transcripts-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        let sv = Services { clearinghouse: ch.uri(), parchment: Endpoints::default() };
        let write = |order: &str| {
            std::fs::write(
                dir.join("transcript_orders.toml"),
                format!(
                    "email = \"a@b.c\"\n[[send]]\nfrom = \"Connecticut College\"\nto = \"Azusa Pacific University\"\nvia = \"clearinghouse\"\norder = \"{order}\"\n"
                ),
            )
            .expect("write")
        };
        write("97852158");
        run_with(&ctx(&dir), &sv, &mut || None).await.expect("all sent");
        write("2");
        let err = run_with(&ctx(&dir), &sv, &mut || None).await.expect_err("unreadable order");
        assert_eq!(err.to_string(), "1 transcript order(s) need attention");
        std::fs::write(dir.join("transcript_orders.toml"), "nonsense = 1").expect("write");
        assert!(run_with(&ctx(&dir), &sv, &mut || None).await.is_err());
        std::fs::remove_dir_all(&dir).expect("cleanup");
        assert_eq!(Services::default().clearinghouse, admit_clearinghouse::BASE);
    })
    .await
}

// ------------------------------------------------- Parchment, with a Keychain

/// A Keychain in a directory: `add-generic-password … -a ACCOUNT -w VALUE`
/// writes the file ACCOUNT, `find-generic-password … -a ACCOUNT -w` prints
/// it; a file `deny` makes every write fail.
const SECURITY: &str = r#"#!/bin/sh
dir="${0%/*}/keychain"
cmd="$1"; shift
# A lookup ends with a bare -w: shift one at a time, or `shift 2` fails and loops.
while [ $# -gt 0 ]; do case "$1" in -a) shift; acc="$1";; -w) shift; val="${1-}";; esac; [ $# -gt 0 ] && shift; done
case "$cmd" in
  find-generic-password) [ -f "$dir/$acc" ] && /bin/cat "$dir/$acc" || exit 1 ;;
  add-generic-password) [ -f "${0%/*}/deny" ] && exit 1; printf '%s' "$val" > "$dir/$acc" ;;
  *) exit 1 ;;
esac"#;

fn keychain(dir: &Path) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let bin = dir.join("bin");
    std::fs::create_dir_all(bin.join("keychain")).expect("bin");
    std::fs::write(bin.join("security"), SECURITY).expect("script");
    std::fs::set_permissions(bin.join("security"), std::fs::Permissions::from_mode(0o755)).expect("mode");
    bin
}

fn stored(bin: &Path, account: &str) -> Option<String> {
    std::fs::read_to_string(bin.join("keychain").join(account)).ok()
}

/// Runs this test again in a child whose PATH holds only the fake Keychain.
fn child(bin: &Path, flag: &str, stdin: &str) {
    use std::io::Write;
    let name = std::thread::current().name().expect("test name").to_string();
    let mut cmd = Command::new(std::env::current_exe().expect("exe"));
    cmd.args(["--exact", &name, "--test-threads=1"])
        .env(flag, bin)
        .env("PATH", format!("{}:/bin:/usr/bin", bin.display()))
        .env_remove("PARCHMENT_USER")
        .env_remove("PARCHMENT_PASSWORD")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let mut proc = cmd.spawn().expect("child");
    proc.stdin.take().expect("stdin").write_all(stdin.as_bytes()).expect("stdin");
    let out = proc.wait_with_output().expect("child");
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success() && bin.join("done").is_file(), "{text}");
    std::fs::remove_file(bin.join("done")).expect("done");
}

#[derive(Clone, Copy, PartialEq)]
enum Portal {
    Working,
    /// The csrf endpoint fails: no sign-in.
    SignInFails,
    /// Signs in, but the orders page still sends it to sign in.
    OrdersFail,
}

/// The portal: orders behind cookie `session-token=S`, a sign-in that
/// Keycloak finishes without a form (its session is alive).
async fn parchment_as(mode: Portal) -> (MockServer, MockServer) {
    let (www, auth) = (MockServer::start().await, MockServer::start().await);
    if mode == Portal::SignInFails {
        Mock::given(method("GET"))
            .and(path("/lp/api/auth/csrf"))
            .respond_with(ResponseTemplate::new(503))
            .mount(&www)
            .await;
    }
    if mode != Portal::OrdersFail {
        Mock::given(method("GET"))
            .and(path("/lp/orders"))
            .and(wiremock::matchers::header_regex("cookie", "session-token=S"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(fixture("parchment", "orders.html"), "text/html"))
            .mount(&www)
            .await;
    }
    Mock::given(method("GET"))
        .and(path("/lp/orders"))
        .and(wiremock::matchers::header_regex("cookie", "broken=1"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&www)
        .await;
    Mock::given(method("GET"))
        .and(path("/lp/orders"))
        .respond_with(ResponseTemplate::new(307).insert_header("Location", "/lp/signIn"))
        .mount(&www)
        .await;
    Mock::given(method("GET"))
        .and(path("/lp/signIn"))
        .respond_with(ResponseTemplate::new(200).set_body_string("sign in"))
        .mount(&www)
        .await;
    Mock::given(method("GET"))
        .and(path("/lp/api/auth/csrf"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"csrfToken": "T"})))
        .mount(&www)
        .await;
    Mock::given(method("POST"))
        .and(path("/lp/api/auth/signin/keycloak"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"url": format!("{}/auth/x", auth.uri())})))
        .mount(&www)
        .await;
    Mock::given(method("GET"))
        .and(path("/lp/api/auth/callback/keycloak"))
        .respond_with(
            ResponseTemplate::new(302)
                .insert_header("Set-Cookie", "session-token=S; Path=/lp")
                .insert_header("Location", "/lp/orders"),
        )
        .mount(&www)
        .await;
    Mock::given(method("GET"))
        .and(path("/auth/x"))
        .respond_with(
            ResponseTemplate::new(302)
                .insert_header("Set-Cookie", "KEYCLOAK_TRUSTED_DEVICE=d; Path=/auth/realms/parchment/")
                .insert_header("Location", format!("{}/lp/api/auth/callback/keycloak", www.uri())),
        )
        .mount(&auth)
        .await;
    (www, auth)
}

async fn parchment() -> (MockServer, MockServer) {
    parchment_as(Portal::Working).await
}

fn endpoints(hosts: &(MockServer, MockServer)) -> Endpoints {
    Endpoints { www: hosts.0.uri(), auth: hosts.1.uri() }
}

const BAD: fn() -> Endpoints = || Endpoints { www: "not a url".into(), auth: "x".into() };

#[tokio::test]
async fn parchment_keeps_its_session_and_signs_in_again_when_it_lapses() {
    tokio::task::unconstrained(async {
        if let Some(bin) = std::env::var_os("ADMIT_TRANSCRIPTS_KEYCHAIN").map(PathBuf::from) {
            let (www, auth) = parchment().await;
            let ep = Endpoints { www: www.uri(), auth: auth.uri() };
            let k = |account: &str, value: &str| std::fs::write(bin.join("keychain").join(account), value);
            // No session, no sign-in: say how to store one.
            let err = parchment_orders(&ep, &mut || None).await.expect_err("nothing stored");
            assert!(err.to_string().contains("admit parchment login"), "{err}");
            // A stored session is used as it is, and kept.
            k("parchment", "session-token=S").expect("cookie");
            assert_eq!(parchment_orders(&ep, &mut || None).await.expect("orders").len(), 5);
            assert_eq!(stored(&bin, "parchment").as_deref(), Some("session-token=S"));
            // An error other than expiry is not a reason to sign in.
            k("parchment", "broken=1").expect("cookie");
            assert!(parchment_orders(&ep, &mut || None).await.is_err());
            // Expired, with a sign-in stored: signs in and keeps both cookies.
            k("parchment", "old=1").expect("cookie");
            k("parchment-user", "u").expect("user");
            k("parchment-password", "p").expect("password");
            assert_eq!(parchment_orders(&ep, &mut || None).await.expect("signed in").len(), 5);
            assert!(stored(&bin, "parchment").is_some_and(|c| c.contains("session-token=S")));
            assert_eq!(stored(&bin, "parchment-device").as_deref(), Some("KEYCLOAK_TRUSTED_DEVICE=d"));
            // Endpoints that are not URLs, a sign-in that fails, orders still
            // unreadable after signing in: each is an error, not an empty list.
            assert!(parchment_orders(&BAD(), &mut || None).await.is_err());
            k("parchment", "old=1").expect("cookie");
            let fails = parchment_as(Portal::SignInFails).await;
            let err = parchment_orders(&endpoints(&fails), &mut || None).await.expect_err("csrf down");
            assert!(err.to_string().contains("csrf: HTTP 503"), "{err}");
            let stuck = parchment_as(Portal::OrdersFail).await;
            let err = parchment_orders(&endpoints(&stuck), &mut || None).await.expect_err("still signed out");
            assert!(err.to_string().contains("session expired"), "{err}");
            // The command reads Parchment when a send goes through it.
            k("parchment", "session-token=S").expect("cookie");
            let profile = bin.join("profile");
            std::fs::create_dir_all(&profile).expect("profile");
            std::fs::write(
                profile.join("transcript_orders.toml"),
                "[[send]]\nfrom = \"Los Angeles City College\"\nto = \"NursingCAS - American Association of Colleges of Nursing\"\nvia = \"parchment\"\n",
            )
            .expect("orders file");
            let sv = Services { clearinghouse: "http://127.0.0.1:9".into(), parchment: ep.clone() };
            run_with(&ctx(&profile), &sv, &mut || None).await.expect("LACC was sent");
            // Where nothing can be stored, the orders are still read.
            std::fs::write(bin.join("deny"), "").expect("deny");
            k("parchment", "old=1").expect("cookie");
            assert_eq!(parchment_orders(&ep, &mut || None).await.expect("unstored").len(), 5);
            std::fs::remove_file(bin.join("deny")).expect("allow");
            std::fs::write(bin.join("done"), "ok").expect("done");
            return;
        }
        let dir = std::env::temp_dir().join(format!("admit-transcripts-kc-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let bin = keychain(&dir);
        child(&bin, "ADMIT_TRANSCRIPTS_KEYCHAIN", "");
        std::fs::remove_dir_all(&dir).expect("cleanup");
    })
    .await
}

#[tokio::test]
async fn login_stores_the_sign_in_from_stdin_and_checks_it() {
    tokio::task::unconstrained(async {
        if let Some(bin) = std::env::var_os("ADMIT_TRANSCRIPTS_LOGIN").map(PathBuf::from) {
            // stdin is read once per process: one case per child.
            let case = std::fs::read_to_string(bin.join("case")).expect("case");
            let result = match case.as_str() {
                "bad endpoints" => login(&BAD(), &mut || None).await,
                "sign-in fails" => login(&endpoints(&parchment_as(Portal::SignInFails).await), &mut || None).await,
                "orders fail" => login(&endpoints(&parchment_as(Portal::OrdersFail).await), &mut || None).await,
                _ => login(&endpoints(&parchment().await), &mut || None).await,
            };
            match case.as_str() {
                "works" => {
                    result.expect("login");
                    assert_eq!(stored(&bin, "parchment-user").as_deref(), Some("me@school.example"));
                    assert_eq!(stored(&bin, "parchment-password").as_deref(), Some("pw"));
                    assert!(stored(&bin, "parchment").is_some_and(|c| c.contains("session-token=S")));
                }
                "no stdin" => assert!(result.expect_err(&case).to_string().contains("two lines")),
                _ => assert!(result.is_err(), "{case}"),
            }
            std::fs::write(bin.join("done"), "ok").expect("done");
            return;
        }
        let dir = std::env::temp_dir().join(format!("admit-transcripts-login-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let bin = keychain(&dir);
        for (case, stdin) in [
            ("works", "me@school.example\npw\n"),
            ("no stdin", ""),
            ("bad endpoints", "u\np\n"),
            ("sign-in fails", "u\np\n"),
            ("orders fail", "u\np\n"),
        ] {
            std::fs::write(bin.join("case"), case).expect("case");
            child(&bin, "ADMIT_TRANSCRIPTS_LOGIN", stdin);
        }
        std::fs::remove_dir_all(&dir).expect("cleanup");
    })
    .await
}
