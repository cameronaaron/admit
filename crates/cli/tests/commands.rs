//! The real binary, isolated HOME and command stubs: never the user's Keychain,
//! launchd jobs, transcript inbox, or application portals.
#[path = "../../liaison/tests/support/mod.rs"]
mod support;
use serde_json::json;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

struct Workspace {
    dir: PathBuf,
}
impl Workspace {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "admit-cli-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir_all(dir.join("bin")).expect("bin");
        std::fs::create_dir_all(dir.join("profile")).expect("profile");
        let w = Self { dir };
        w.script("security", "case \"$1\" in\n find-generic-password) case \"$5\" in *-user) if [ -n \"$TEST_KEYCHAIN_CREDS\" ]; then echo user; else exit 1; fi;; *-password) if [ -n \"$TEST_KEYCHAIN_CREDS\" ]; then echo password; else exit 1; fi;; *) if [ -n \"$TEST_STORED_COOKIE\" ]; then echo \"$TEST_STORED_COOKIE\"; else exit 1; fi;; esac;;\n *) exit \"${TEST_SECURITY_STATUS:-0}\";;\nesac");
        w.script("launchctl", "exit \"${TEST_LAUNCH_STATUS:-0}\"");
        w.script("osascript", "exit 0");
        w.script("pdftotext", "case \"$2\" in *elmbridge*) cat \"$TEST_FIXTURES/elmbridge_layout.txt\";; *conncoll*) cat \"$TEST_FIXTURES/conncoll_raw.txt\";; *) cat \"$TEST_FIXTURES/laccd_raw.txt\";; esac");
        w
    }
    fn script(&self, name: &str, body: &str) {
        let p = self.dir.join("bin").join(name);
        std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).expect("script");
        std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o755)).expect("permissions");
    }
    fn write(&self, p: &str, body: &str) {
        std::fs::write(self.dir.join(p), body).expect("write");
    }
    fn config(&self, kind: &str, url: &str, extra: &str) {
        self.write(
            "admit.toml",
            &format!("{extra}\n[[portal]]\nname='test'\nkind='{kind}'\nbase_url='{url}'\ncookie_env='TEST_COOKIE'\n"),
        );
    }
    /// Records the fake portal's response shapes: apply writes nothing
    /// without a recording to check them against.
    fn record(&self) {
        self.run(&["drift", "--record"], true, "recorded");
    }
    fn command(&self, args: &[&str]) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_admit"));
        c.current_dir(&self.dir)
            .args(args)
            .env("HOME", &self.dir)
            .env("PATH", format!("{}:/usr/bin:/bin", self.dir.join("bin").display()))
            .env("TEST_FIXTURES", Path::new(env!("CARGO_MANIFEST_DIR")).join("../sources/tests/fixtures"))
            .env("TEST_COOKIE", "SESSION=test")
            .env_remove("LACCD_USER")
            .env_remove("LACCD_PASSWORD")
            .env_remove("TEST_USER")
            .env_remove("TEST_PASSWORD");
        c
    }
    fn run(&self, args: &[&str], success: bool, expected: &str) -> Output {
        let out = self.command(args).output().expect("run");
        check(&out, success, expected);
        out
    }
    fn run_env(&self, args: &[&str], env: &[(&str, &str)], remove: &[&str], success: bool, expected: &str) {
        let mut c = self.command(args);
        for (k, v) in env {
            c.env(k, v);
        }
        for k in remove {
            c.env_remove(k);
        }
        check(&c.output().expect("run"), success, expected);
    }
    fn stdin(&self, args: &[&str], input: &str, success: bool, expected: &str) {
        let mut child = self
            .command(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("run");
        child.stdin.take().expect("stdin").write_all(input.as_bytes()).expect("input");
        check(&child.wait_with_output().expect("output"), success, expected);
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.dir).expect("cleanup");
    }
}
fn check(out: &Output, success: bool, expected: &str) {
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert_eq!(out.status.success(), success, "{text}");
    assert!(text.contains(expected), "missing {expected:?}: {text}");
}

#[tokio::test]
async fn liaison_commands_use_the_real_dispatch_and_profile_files() {
    tokio::task::unconstrained(async move {
        let s = support::portal().await;
        let w = Workspace::new();
        w.config("liaison", &s.uri(), "");
        w.record();
        w.run(&["validate"], true, "0 finding(s)");
        w.run(&["questions"], true, "no program questions on test");
        for cmd in ["status", "get", "schools", "lookup"] {
            let args = match cmd {
                "get" => vec![cmd, "aca"],
                "schools" => vec![cmd, "US:CA"],
                "lookup" => vec![cmd, "o1", "Yale"],
                _ => vec![cmd],
            };
            w.run(&args, false, "Slate portal");
        }
        w.run(&["plan", "--only", "bad"], false, "unknown section");
        w.run(&["plan"], true, "0 change(s)");
        w.run(&["apply"], true, "nothing to apply; no changes remain");
        // No change left is not the same as nothing wrong: an answer for a
        // program the portal doesn't have can't be written, so apply fails.
        w.write("profile/questions.toml", "[[program]]\nname='Nowhere'\nportal='test'\n[program.answers]\n1='Yes'\n");
        w.run(&["apply", "--yes"], false, "nothing to apply; no changes remain, but 1 violation(s)");
        std::fs::remove_file(w.dir.join("profile/questions.toml")).expect("removed");
        w.run(&["pull"], true, "wrote");
        w.run(&["pull"], true, "kept");
        w.run(&["pull", "--force"], true, "wrote");
        w.run(&["plan"], true, "0 change(s)");
        w.run(&["-p", "test", "audit"], true, "finding(s)");
        w.run(&["audit"], true, "across 2 group(s)");
        // personal.toml against Biographic Information ("Pat Doe" on the fake portal).
        w.write("profile/personal.toml", "legal_name = 'Pat Q Doe'\n");
        w.run(&["-p", "test", "audit"], false, "legal_name is \"Pat Doe\"; personal.toml says \"Pat Q Doe\"");
        w.write("profile/personal.toml", "legal_name = 'Pat Doe'\n");
        w.run(&["-p", "test", "audit"], true, "finding(s)");
        w.run(&["courses"], true, "");
        w.run(&["session", "check"], true, "session works");
        w.run(&["session", "clear"], true, "forgot");
        w.stdin(&["session", "set"], "Cookie: SESSION=test\n", true, "session works");
        w.stdin(&["session", "set"], "not a cookie", false, "Cookie header");
        w.stdin(&["session", "login"], "", false, "two lines");
        w.write("profile/achievements.toml", "[[achievement]]\nkind='award'\nname=''\nstate='California'\n");
        w.run(&["validate"], false, "no name");
        w.run(&["apply", "--yes"], false, "fix the profile violations");
        w.write("profile/achievements.toml", "[[achievement]]\nkind='award'\nname='New award'\nstate='California'\n");
        w.run(&["apply"], false, "re-run with --yes");
        w.run(&["apply", "--yes"], false, "did not converge");
        w.run(&["-p", "test", "audit"], true, "not yet applied");
        w.run(&["-p", "missing", "plan"], false, "no portal");

        // Unreadable personal details are a violation, and the rest of the audit still runs.
        let s = support::portal().await;
        http_500(&s, "GET", "/applicant/api/authenticatedUser").await;
        let w = Workspace::new();
        w.config("liaison", &s.uri(), "");
        let out = w.run(&["-p", "test", "audit"], false, "personal details UNCHECKED");
        assert!(String::from_utf8_lossy(&out.stdout).contains("finding(s)"), "the audit finished");
    })
    .await
}

#[tokio::test]
async fn website_and_configuration_failures_are_visible() {
    tokio::task::unconstrained(async move {
        let s = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_string(format!(
                "<script type=\"application/ld+json\">{}</script>",
                json!({"@type":"Person","hasCredential":[{"name":"License","recognizedBy":{"name":"Board"}}]})
            )))
            .mount(&s)
            .await;
        let w = Workspace::new();
        w.config("liaison", &s.uri(), &format!("[website]\nurl='{}'", s.uri()));
        w.run(&["website"], true, "finding(s)");
        w.config("liaison", &s.uri(), "");
        w.run(&["website"], false, "no [website]");
        w.write("admit.toml", "portal=[]");
        w.run(&["plan"], false, "lists no [[portal]]");
        w.write("admit.toml", "invalid = true");
        w.run(&["plan"], false, "parsing admit.toml");
        w.run(&["-c", "missing.toml", "plan"], false, "reading missing.toml");
        w.config("unknown", &s.uri(), "");
        w.run(&["audit"], false, "unknown portal kind");
        w.run(&["plan"], false, "supports liaison portals only");
        w.run(&["transcript", "x.pdf", "--parser", "bad"], false, "unknown transcript parser");
        for parser in ["laccd", "elmbridge", "conncoll"] {
            w.run(&["transcript", &format!("{parser}.pdf"), "--parser", parser], true, "totals reconcile");
        }
        w.run(&["laccd", "fetch"], false, "no LACCD sign-in");
        w.stdin(&["laccd", "login"], "", false, "two lines");
    })
    .await
}

#[tokio::test]
async fn scheduling_and_sync_keep_reports_and_due_state() {
    tokio::task::unconstrained(async move {
    let s = support::portal().await;
    let w = Workspace::new();
    w.config("liaison", &s.uri(), "");
    w.run(&["schedule", "install"], false, "no [sync]");
    w.run(&["sync"], false, "no [sync]");
    std::fs::create_dir(w.dir.join("inbox")).expect("inbox");
    let sync = format!(
        "[sync]\ninbox='{}'\nprefix='transcript'\ntranscript='laccd.pdf'\nreports='reports'\nevery_weeks=8\nrun_on=[]",
        w.dir.join("inbox").display()
    );
    w.config("liaison", &s.uri(), &sync);
    w.run(&["schedule", "install"], true, "installed");
    let plist = std::fs::read_to_string(w.dir.join("Library/LaunchAgents/com.admit.sync.plist")).expect("plist");
    assert!(plist.contains("--if-due"));
    w.run(&["schedule", "uninstall"], true, "removed");
    w.run(&["sync"], true, "no transcript newer");
    assert!(w.dir.join("reports/last-sync").exists());
    w.run(&["sync", "--if-due"], true, "not due");
    w.write("inbox/transcript-new.pdf", "mock pdf");
    w.run(&["sync"], true, "installed");
    assert_eq!(std::fs::read_to_string(w.dir.join("profile/laccd.pdf")).expect("installed"), "mock pdf");
    w.write("inbox/transcript-newer.pdf", "same courses");
    w.run(&["sync"], true, "transcript unchanged");
    w.config("liaison", &s.uri(), &format!("{sync}\nfetch='unknown'"));
    w.run(&["sync"], false, "not supported");
    w.config("liaison", &s.uri(), &format!("{sync}\nfetch='laccd'"));
    w.run(&["sync"], true, "LACCD download failed");
})
    .await
}

async fn slate_server() -> MockServer {
    let s = MockServer::start().await;
    let review = "<div id=\"menu\"><a href='frm?basic' onclick='return apply_navigate(this);'>Basic</a><a href='aca' onclick='return apply_navigate(this);'>Academic History</a><a href='job' onclick='return apply_navigate(this);'>Professional Experience</a><a href='review' onclick='return apply_navigate(this);'>Review</a></div><h1>Review</h1>";
    let page="<h1>Basic</h1><div class='form_question' data-id='name' data-type='text' data-export='sys:first' data-required='1'><label>Name</label><input name='form_name' value='Pat'></div><div class='form_question' data-id='choice' data-type='select' data-export='choice'><label>Choice</label><select name='form_choice'><option value=''></option><option value='1'>Yes</option></select></div>".replace('\'', "\"");
    for (p, body) in [
        ("/apply/review", review),
        ("/apply/frm", page.as_str()),
        ("/apply/", include_str!("../../slate/tests/fixtures/applications.html")),
        ("/apply/aca", include_str!("../../slate/tests/fixtures/aca.html")),
        ("/apply/job", include_str!("../../slate/tests/fixtures/job.html")),
        ("/manage/service/lookup", "{\"item\":[]}"),
    ] {
        Mock::given(method("GET"))
            .and(path(p))
            .respond_with(ResponseTemplate::new(200).set_body_string(body))
            .mount(&s)
            .await;
    }
    Mock::given(method("POST"))
        .and(path("/apply/"))
        .respond_with(ResponseTemplate::new(200).insert_header("set-cookie", ".application=app; path=/"))
        .mount(&s)
        .await;
    Mock::given(method("POST"))
        .and(path("/apply/frm"))
        .respond_with(ResponseTemplate::new(200).set_body_string("saved"))
        .mount(&s)
        .await;
    s
}

#[tokio::test]
async fn slate_commands_read_plan_save_and_report_open_sections() {
    tokio::task::unconstrained(async move {
        let s = slate_server().await;
        let w = Workspace::new();
        w.config("slate", &s.uri(), "");
        let config = std::fs::read_to_string(w.dir.join("admit.toml")).expect("config");
        w.write("admit.toml", &format!("{config}\napplication='app'"));
        w.run(&["plan", "--only", "education"], false, "--only picks Liaison");
        w.run(&["plan"], false, "create it with");
        w.run(&["pull"], true, "wrote");
        w.run(&["pull"], true, "kept");
        w.run(&["pull", "--force"], true, "wrote");
        w.run(&["questions"], true, "sys:first");
        w.run(&["questions", "--open"], true, "(blank)");
        w.run(&["get", "aca"], true, "Academic History");
        w.run(&["schools", "US:CA"], true, "cities:");
        w.run(&["schools", "US:CA", "Los Angeles"], true, "Academic History");
        w.run(&["lookup", "o1", "Yale"], true, "item");
        w.run(&["plan"], true, "0 change(s)");
        w.run(&["apply"], true, "section Academic History: TODO");
        let answers = std::fs::read_to_string(w.dir.join("profile/slate/test.toml")).expect("answers");
        w.write(
            "profile/slate/test.toml",
            &format!("{answers}\n[excluded]\n'Academic History'='No colleges in this test profile'\n"),
        );
        w.run(&["apply"], true, "every section accounted for");
        w.run(&["session", "check"], true, "session works");
        w.stdin(&["session", "set"], "cookie: _uid=test", true, "session works");
        w.run(&["session", "clear"], true, "forgot");
        w.run(&["-p", "test", "audit"], true, "finding(s)");
        w.run(&["audit"], true, "no deadline declared");
        w.write("profile/slate/test.toml", "[answers]\n'sys:first'='Changed'\n");
        w.run(&["apply"], false, "re-run with --yes");
        w.run(&["apply", "--yes"], false, "did not converge");
        w.run(&["-p", "test", "audit"], true, "not yet applied");
        w.write("profile/slate/test.toml", "[answers]\nchoice='invalid'\n");
        w.run(&["apply", "--yes"], false, "fix the answers");
        w.write("profile/slate/test.toml", "bad = true");
        w.run(&["plan"], false, "parsing");
        w.write("admit.toml", &format!("{config}\nsubmitted=true"));
        for cmd in ["plan", "apply", "pull", "questions"] {
            w.run(&[cmd], false, "is submitted");
        }
    })
    .await
}

#[tokio::test]
async fn submitted_slate_downloads_its_update_and_audits_the_checklist() {
    tokio::task::unconstrained(async move {
        let s = MockServer::start().await;
        for (p, body) in [
            ("/apply/status", include_str!("../../slate/tests/fixtures/apu_status.html")),
            ("/apply/update", include_str!("../../slate/tests/fixtures/apu_update.html")),
        ] {
            Mock::given(method("GET"))
                .and(path(p))
                .respond_with(ResponseTemplate::new(200).set_body_string(body))
                .mount(&s)
                .await;
        }
        Mock::given(method("GET"))
            .and(path("/apply/update"))
            .and(wiremock::matchers::query_param("cmd", "stream"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"%PDF-test".to_vec()))
            .with_priority(1)
            .mount(&s)
            .await;
        let w = Workspace::new();
        w.config("slate", &s.uri(), "");
        let config = std::fs::read_to_string(w.dir.join("admit.toml")).expect("config");
        w.write("admit.toml", &format!("{config}\nsubmitted=true"));
        w.run(&["status"], true, "saved");
        let pdfs: Vec<_> = std::fs::read_dir(w.dir.join("reports")).expect("reports").collect();
        assert_eq!(pdfs.len(), 1);
        assert_eq!(std::fs::read(pdfs[0].as_ref().expect("entry").path()).expect("pdf"), b"%PDF-test");
        w.run(&["session", "check"], true, "checklist item(s)");
        w.run(&["-p", "test", "audit"], true, "awaited");
        w.run(&["audit"], true, "across 2 group(s)");
        // Submitted: its awaited items are listed, but nothing is left to make ready.
        w.run(&["ready"], true, "test: SUBMITTED, 2 item(s) awaited");
        w.run(&["remind"], true, "no unready application is due");
    })
    .await
}

#[tokio::test]
async fn expired_sessions_reauthenticate_and_keychain_failures_are_visible() {
    tokio::task::unconstrained(async move {
    let s = support::portal().await;
    Mock::given(method("GET"))
        .and(path("/applicant/initialize"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&s)
        .await;
    Mock::given(method("POST"))
        .and(path("/applicant/authenticate"))
        .respond_with(
            ResponseTemplate::new(200).insert_header("set-cookie", "SESSION=fresh; path=/").set_body_json(json!({})),
        )
        .with_priority(1)
        .mount(&s)
        .await;
    Mock::given(method("GET"))
        .and(path("/applicant/api/navigation"))
        .and(wiremock::matchers::header("cookie", "SESSION=expired"))
        .respond_with(ResponseTemplate::new(401))
        .with_priority(1)
        .mount(&s)
        .await;
    let w = Workspace::new();
    w.config("liaison", &s.uri(), "");
    let out = w
        .command(&["plan"])
        .env("TEST_COOKIE", "SESSION=expired")
        .env("TEST_USER", "user")
        .env("TEST_PASSWORD", "password")
        .env("TEST_SECURITY_STATUS", "1")
        .output()
        .expect("reauth");
    check(&out, true, "session not stored");
    let out = w.command(&["session", "check"]).env("TEST_COOKIE", "SESSION=expired").output().expect("expired");
    check(&out, false, "session has expired");
    let out = w.command(&["plan"]).env_remove("TEST_COOKIE").output().expect("no session");
    check(&out, false, "no session");
    let out = w
        .command(&["plan"])
        .env_remove("TEST_COOKIE")
        .env("TEST_STORED_COOKIE", "SESSION=test")
        .output()
        .expect("stored cookie");
    check(&out, true, "0 change(s)");
    let out = w
        .command(&["plan"])
        .env_remove("TEST_COOKIE")
        .env("TEST_USER", "user")
        .env("TEST_PASSWORD", "password")
        .output()
        .expect("no cookie login");
    check(&out, true, "signed in");
    let out = w.command(&["session", "clear"]).env("TEST_SECURITY_STATUS", "1").output().expect("clear");
    check(&out, false, "no stored session");
    let mut child = w
        .command(&["session", "set"])
        .env("TEST_SECURITY_STATUS", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("set");
    child.stdin.take().expect("stdin").write_all(b"SESSION=test").expect("write");
    check(&child.wait_with_output().expect("output"), false, "add-generic-password");
    let dir = w.dir.join("har");
    std::fs::create_dir(&dir).expect("har");
    std::fs::write(dir.join("test.har"),json!({"log":{"entries":[{"startedDateTime":"2026-10-06T00:00:00Z","request":{"url":format!("{}/applicant/api/navigation",s.uri()),"headers":[{"name":"Cookie","value":"SESSION=test"}]}}]}}).to_string()).expect("har");
    let config = std::fs::read_to_string(w.dir.join("admit.toml")).expect("config");
    w.write("admit.toml", &format!("{config}\nhar_dir='{}'", dir.display()));
    let out = w.command(&["plan"]).env_remove("TEST_COOKIE").output().expect("har cookie");
    check(&out, true, "0 change(s)");
    Mock::given(method("GET"))
        .and(path("/applicant/api/navigation"))
        .respond_with(ResponseTemplate::new(500).set_body_string("failed"))
        .with_priority(1)
        .mount(&s)
        .await;
    w.run(&["plan"], false, "HTTP 500");
    w.run(&["audit"], false, "UNCHECKED");
})
    .await
}

#[tokio::test]
async fn successful_apply_replans_and_courses_print_science_subjects() {
    tokio::task::unconstrained(async move {
    let s = support::portal().await;
    let changed = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let read = changed.clone();
    Mock::given(method("GET")).and(path("/applicant/api/supportingInfo/achievements"))
        .respond_with(move |_:&wiremock::Request| ResponseTemplate::new(200).set_body_json(json!({"achievements":[{"id":1,"achievement":"Awards","name":"Award","stateId":339,"countryId":222,"description":if read.load(std::sync::atomic::Ordering::SeqCst){"Changed"}else{"Old"}}]}))).with_priority(1).mount(&s).await;
    Mock::given(method("PUT"))
        .and(path("/applicant/api/supportingInfo/achievements/1"))
        .respond_with(move |_: &wiremock::Request| {
            changed.store(true, std::sync::atomic::Ordering::SeqCst);
            ResponseTemplate::new(200)
        })
        .with_priority(1)
        .mount(&s)
        .await;
    let w = Workspace::new();
    w.config("liaison", &s.uri(), "");
    w.record();
    w.write(
        "profile/achievements.toml",
        "[[achievement]]\nkind='award'\nname='Award'\nstate='California'\ndescription='Changed'\n",
    );
    // A violation in a selected section fails the run, after the writes
    // that could go in (the achievement converged)...
    w.write("profile/questions.toml", "[[program]]\nname='Nowhere'\nportal='test'\n[program.answers]\n1='Yes'\n");
    w.run(&["apply", "--yes"], false, "applied; no changes remain, but 1 violation(s)");
    // ...and one in a section the run doesn't select doesn't fail it.
    w.run(&["apply", "--only", "achievements", "--yes"], true, "nothing to apply; no changes remain");
    std::fs::remove_file(w.dir.join("profile/questions.toml")).expect("removed");
    Mock::given(method("GET"))
        .and(path("/applicant/api/academicHistory/courseWork"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([{"id":1,"college":"School"}])))
        .with_priority(1)
        .mount(&s)
        .await;
    Mock::given(method("GET")).and(path("/applicant/api/academicHistory/courseWork/summary/1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"terms":[{"term":"Fall","year":2026,"courses":[{"code":"SCI 1","title":"Science","credits":3,"subject":"Other Science","isScience":true},{"code":"ART 1","title":"Art","credits":"3","subject":"Art","isScience":false}]}]}))).mount(&s).await;
    w.run(&["courses"], true, "Other Science (science)");
    w.write("profile/prerequisites.toml","[[program]]\nname='Test'\nstart='2027-01'\n[[program.requirement]]\nname='Science'\n[[program.requirement.courses]]\ncollege='Missing'\ncode='SCI 1'\n");
    w.run(&["-p", "test", "audit"], false, "no transcript in coursework.toml");
    w.config("liaison", &s.uri(), &format!("[website]\nurl='{}/missing'", s.uri()));
    w.run(&["-p", "test", "audit"], false, "website not checked");
    w.script("pdftotext", "echo invalid PDF >&2; exit 1");
    w.run(&["transcript", "bad.pdf"], false, "pdftotext failed");
})
    .await
}

#[tokio::test]
async fn slate_reauthentication_opens_the_configured_application() {
    tokio::task::unconstrained(async move {
        let s = slate_server().await;
        Mock::given(method("GET")).and(path("/account/login")).respond_with(ResponseTemplate::new(200)).mount(&s).await;
        Mock::given(method("POST"))
            .and(path("/account/login"))
            .respond_with(ResponseTemplate::new(302).insert_header("set-cookie", "_uid=fresh; path=/"))
            .mount(&s)
            .await;
        Mock::given(method("POST"))
            .and(path("/apply/"))
            .and(wiremock::matchers::header("cookie", "SESSION=expired"))
            .respond_with(ResponseTemplate::new(302).insert_header("location", "/account/login"))
            .with_priority(1)
            .mount(&s)
            .await;
        let w = Workspace::new();
        w.config("slate", &s.uri(), "");
        w.run(&["get", "aca"], false, "has 2 applications");
        let config = std::fs::read_to_string(w.dir.join("admit.toml")).expect("config");
        w.write("admit.toml", &format!("{config}\napplication='app'\ndeadline='2027-01-01'"));
        let out = w
            .command(&["get", "aca"])
            .env("TEST_COOKIE", "SESSION=expired")
            .env("TEST_USER", "user")
            .env("TEST_PASSWORD", "password")
            .env("TEST_SECURITY_STATUS", "1")
            .output()
            .expect("reauth");
        check(&out, true, "session not stored");
        let out = w.command(&["get", "aca"]).env("TEST_COOKIE", "SESSION=expired").output().expect("expired");
        check(&out, false, "session has expired");
        let out = w.command(&["get", "aca"]).env_remove("TEST_COOKIE").output().expect("missing session");
        check(&out, false, "no session");
        let out = w
            .command(&["get", "aca"])
            .env_remove("TEST_COOKIE")
            .env("TEST_USER", "user")
            .env("TEST_PASSWORD", "password")
            .output()
            .expect("login");
        check(&out, true, "signed in");
        w.run(&["pull"], true, "wrote");
        w.run(&["audit"], true, "2027-01-01");
        w.stdin(&["session", "login"], "user\npassword\n", false, "credentials were not stored");
        let mut child = w
            .command(&["session", "login"])
            .env("TEST_USER", "user")
            .env("TEST_PASSWORD", "password")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("login");
        child.stdin.take().expect("stdin").write_all(b"user\npassword\n").expect("write");
        check(&child.wait_with_output().expect("output"), true, "session works");
        w.config("slate", &s.uri(), "");
        let single = include_str!("../../slate/tests/fixtures/applications.html")
            .replace("c7c162d5-c56f-4433-9065-aaeeb1334c14", "app");
        // A server with exactly one application must open it automatically.
        let marker = single.find("data-href").expect("application row");
        let start = single[..marker].rfind("<tr").expect("row");
        let end = single[start..].find("</tr>").expect("end") + start + 5;
        let one = format!("<table>{}</table>", &single[start..end]);
        Mock::given(method("GET"))
            .and(path("/apply/"))
            .respond_with(ResponseTemplate::new(200).set_body_string(one))
            .with_priority(1)
            .mount(&s)
            .await;
        w.run(&["get", "aca"], true, "Academic History");
    })
    .await
}

#[tokio::test]
async fn transcript_driven_apply_and_sync_converge_in_two_passes() {
    tokio::task::unconstrained(async move {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    let s = support::portal().await;
    let phase = Arc::new(AtomicUsize::new(0));
    let read = phase.clone();
    Mock::given(method("GET")).and(path("/applicant/api/academicHistory/courseWork")).respond_with(move |_: &wiremock::Request| {
        ResponseTemplate::new(200).set_body_json(if read.load(Ordering::SeqCst)==0 {json!([])}else{json!([{"id":8,"college":"School","collegeCode":"code","stillAttending":false,"collegeEndTerm":"Fall","collegeEndYear":2026}])})
    }).with_priority(1).mount(&s).await;
    let read = phase.clone();
    Mock::given(method("GET")).and(path("/applicant/api/academicHistory/courseWork/summary/8")).respond_with(move |_: &wiremock::Request| {
        let n=read.load(Ordering::SeqCst);
        ResponseTemplate::new(200).set_body_json(json!({"terms":[{"id":20,"term":"Fall","year":2026,"termInProgress":false,"courses":if n<2 {json!([])}else{json!([{"id":30,"code":if n==3 {"BIO 2"}else{"BIO 1"},"title":"Biology","grade":if n==3 {"B"}else{"A"},"credits":"3.00","subject":"Art","subjectId":11}])}}]}))
    }).mount(&s).await;
    Mock::given(method("GET"))
        .and(path("/applicant/api/lookup/colleges"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([{"text":"School","code":"code"}])))
        .mount(&s)
        .await;
    let write = phase.clone();
    Mock::given(method("POST"))
        .and(path("/applicant/api/academicHistory/colleges"))
        .respond_with(move |_: &wiremock::Request| {
            write.store(1, Ordering::SeqCst);
            ResponseTemplate::new(200).set_body_json(json!({"id":8}))
        })
        .with_priority(1)
        .mount(&s)
        .await;
    let write = phase.clone();
    Mock::given(method("POST"))
        .and(path("/applicant/api/academicHistory/courses/collegeId/8/termId/20"))
        .respond_with(move |_: &wiremock::Request| {
            write.store(2, Ordering::SeqCst);
            ResponseTemplate::new(200).set_body_json(json!({"id":30}))
        })
        .with_priority(1)
        .mount(&s)
        .await;
    let w = Workspace::new();
    w.script("pdftotext", "cat \"$2\"");
    w.config("liaison", &s.uri(), "");
    w.record();
    let text = "2026 Fall (Sep 1, 2026 - Dec 21, 2026)\nBIO 1 LAVC Biology 3.00 3.00 A 12 CSU/UC\nTerm GPA 4.00 Term Totals 3.00\n";
    w.write("profile/laccd.pdf", text);
    w.write("profile/coursework.toml","[[transcript]]\npath='laccd.pdf'\nparser='laccd'\nsummer_term='Summer'\ncreate_missing_colleges=true\nacademic_status='Freshman'\n[transcript.colleges]\nLAVC='School'\nLACC='Other school'\n[transcript.subjects]\n'BIO 1'='Art'\n[[transcript]]\npath='absent.pdf'\nparser='laccd'\nsummer_term='Summer'\nacademic_status='Freshman'\n[transcript.colleges]\nLAVC='School'\n");
    w.run(&["apply", "--only", "coursework", "--yes"], false, "absent.pdf");
    assert_eq!(phase.load(Ordering::SeqCst), 0, "unreadable sources must prevent writes");
    // Reads only: GETs, and the deadline search, a POST that reads.
    let read = |r: &wiremock::Request| {
        r.method.as_str() == "GET" || (r.method.as_str() == "POST" && r.url.path() == "/applicant/api/search/filteredQueries")
    };
    assert!(s.received_requests().await.expect("requests").iter().all(read));
    w.write("profile/coursework.toml","[[transcript]]\npath='laccd.pdf'\nparser='laccd'\nsummer_term='Summer'\ncreate_missing_colleges=true\nacademic_status='Freshman'\n[transcript.colleges]\nLAVC='School'\nLACC='Other school'\n[transcript.subjects]\n'BIO 1'='Art'\n");
    w.run(&["apply", "--only", "coursework", "--yes"], true, "second pass for the new institutions");
    assert_eq!(phase.load(Ordering::SeqCst), 2);
    w.run(&["plan", "--only", "coursework"], true, "0 change(s)");
    w.write("profile/prerequisites.toml","[[program]]\nname='Test'\nstart='2027-01'\n[[program.requirement]]\nname='Biology'\n[[program.requirement.courses]]\ncollege='LAVC'\ncode='BIO 1'\n");
    w.run(&["-p", "test", "audit"], true, "Biology");
    std::fs::create_dir(w.dir.join("inbox")).expect("inbox");
    let sync = format!(
        "[sync]\ninbox='{}'\nprefix='transcript'\ntranscript='laccd.pdf'\nreports='reports'",
        w.dir.join("inbox").display()
    );
    w.config("liaison", &s.uri(), &sync);
    w.script("osascript", "printf '%s' \"$2\" > notification.txt");
    phase.store(1, Ordering::SeqCst);
    w.run(&["sync"], true, "after apply:");
    assert_eq!(phase.load(Ordering::SeqCst), 2);
    assert!(
        std::fs::read_to_string(w.dir.join("notification.txt")).expect("notification").contains("coursework updated")
    );
    phase.store(3, Ordering::SeqCst);
    w.run(&["sync"], true, "BIO 2");
    let notification = std::fs::read_to_string(w.dir.join("notification.txt")).expect("notification");
    assert!(notification.contains("correction(s) held"));
    assert!(notification.contains("item(s) need you"));
    let coursework = std::fs::read_to_string(w.dir.join("profile/coursework.toml")).expect("coursework");
    w.write(
        "profile/coursework.toml",
        &format!(
            "{coursework}\n[[transcript]]\npath='absent.pdf'\nparser='laccd'\nsummer_term='Summer'\nacademic_status='Freshman'\n[transcript.colleges]\nLAVC='School'\n"
        ),
    );
    w.run(&["sync"], true, "skipped absent.pdf");
    w.run(&["-p", "test", "audit"], false, "absent.pdf");
    // The prior PDF is backed up when a genuinely different transcript arrives.
    w.write("inbox/transcript-new.pdf", &text.replace("3.00 3.00 A 12", "3.00 3.00 B 9"));
    phase.store(2, Ordering::SeqCst);
    w.run(&["sync"], true, "grade:");
    let files: Vec<_> =
        std::fs::read_dir(w.dir.join("profile")).expect("files").map(|e| e.expect("entry").file_name()).collect();
    assert!(files.iter().any(|n| n.to_string_lossy().starts_with("laccd-")));
    w.write("inbox/transcript-bad.pdf", "not a transcript");
    w.run(&["sync"], false, "did not parse; left the installed transcript alone");
})
    .await
}

#[tokio::test]
async fn slate_list_additions_upload_and_replan_to_a_verified_state() {
    tokio::task::unconstrained(async move {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    let s = slate_server().await;
    let school = Arc::new(AtomicBool::new(false));
    let role = Arc::new(AtomicBool::new(false));
    let read = school.clone();
    Mock::given(method("GET")).and(path("/apply/aca")).respond_with(move |_: &wiremock::Request| {
        ResponseTemplate::new(200).set_body_string(if read.load(Ordering::SeqCst){"<h1>Academic History</h1><div id=\"part_table\"><table><tbody><tr><td>Connecticut College</td></tr></tbody></table></div>"}else{include_str!("../../slate/tests/fixtures/aca.html")})
    }).with_priority(2).mount(&s).await;
    Mock::given(method("GET"))
        .and(path("/apply/aca"))
        .and(wiremock::matchers::query_param("id", ""))
        .respond_with(
            ResponseTemplate::new(200).set_body_string(include_str!("../../slate/tests/fixtures/aca_add.html")),
        )
        .with_priority(1)
        .mount(&s)
        .await;
    let write = school.clone();
    Mock::given(method("POST"))
        .and(path("/apply/aca"))
        .respond_with(move |r: &wiremock::Request| {
            assert!(String::from_utf8_lossy(&r.body).contains("%PDF-test"));
            write.store(true, Ordering::SeqCst);
            ResponseTemplate::new(200)
        })
        .mount(&s)
        .await;
    let read = role.clone();
    Mock::given(method("GET")).and(path("/apply/job")).respond_with(move |_: &wiremock::Request| {
        ResponseTemplate::new(200).set_body_string(if read.load(Ordering::SeqCst){"<h1>Professional Experience</h1><div id=\"part_table\"><table><tbody><tr><td>Clinic</td><td>01/2020–Present</td></tr></tbody></table></div>"}else{include_str!("../../slate/tests/fixtures/job.html")})
    }).with_priority(1).mount(&s).await;
    let write = role.clone();
    Mock::given(method("POST"))
        .and(path("/apply/job"))
        .respond_with(move |_: &wiremock::Request| {
            write.store(true, Ordering::SeqCst);
            ResponseTemplate::new(200)
        })
        .mount(&s)
        .await;
    let w = Workspace::new();
    w.config("slate", &s.uri(), "");
    let cfg = std::fs::read_to_string(w.dir.join("admit.toml")).expect("config");
    w.write("admit.toml", &format!("{cfg}\napplication='app'"));
    std::fs::create_dir(w.dir.join("profile/slate")).expect("answers dir");
    w.write("profile/slate/test.toml","[[institutions]]\nname='Connecticut College'\nkey='003284'\ncity='New London'\nregion='CT'\ncountry='US'\nfrom='2017-08'\nto='2021-05'\nlevel='U'\ndegree='Bachelor of Arts'\nconferred='2021-05'\ntranscript='transcript.pdf'\n[[jobs]]\nemployer='Clinic'\ntitle='Care'\nfrom='2020-01'\nhours=10\ndescription='Care'\n");
    w.run(&["apply", "--yes"], false, "reading transcript");
    assert!(!school.load(Ordering::SeqCst));
    assert!(!role.load(Ordering::SeqCst));
    w.write("profile/transcript.pdf", "%PDF-test");
    w.run(&["apply", "--yes"], true, "applied; no changes remain");
    assert!(school.load(Ordering::SeqCst));
    assert!(role.load(Ordering::SeqCst));
    w.run(&["plan"], true, "0 change(s)");
    w.run(&["validate"], true, "0 finding(s)");
})
    .await
}

#[tokio::test]
async fn session_and_audit_failures_never_report_success() {
    tokio::task::unconstrained(async move {
    let s = support::portal().await;
    let w = Workspace::new();
    w.config("liaison", &s.uri(), "");
    Mock::given(method("GET"))
        .and(path("/applicant/initialize"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&s)
        .await;
    Mock::given(method("POST"))
        .and(path("/applicant/authenticate"))
        .respond_with(
            ResponseTemplate::new(200).insert_header("set-cookie", "SESSION=fresh; path=/").set_body_json(json!({})),
        )
        .with_priority(1)
        .mount(&s)
        .await;
    let mut child = w
        .command(&["session", "login"])
        .env("TEST_KEYCHAIN_CREDS", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("login");
    child.stdin.take().expect("stdin").write_all(b"user\npassword\n").expect("input");
    check(&child.wait_with_output().expect("output"), true, "session works");
    let out = w
        .command(&["laccd", "fetch"])
        .env("LACCD_USER", "user")
        .env("LACCD_PASSWORD", "")
        .output()
        .expect("credentials");
    check(&out, false, "no LACCD sign-in");
    // portal() proves the cookie with one navigation read; the check's own read then fails.
    let nav_reads = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let seen = nav_reads.clone();
    Mock::given(method("GET"))
        .and(path("/applicant/api/navigation"))
        .respond_with(move |_: &wiremock::Request| {
            if seen.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
                ResponseTemplate::new(200).set_body_json(json!({"sections": []}))
            } else {
                ResponseTemplate::new(500).set_body_string("failed")
            }
        })
        .with_priority(1)
        .mount(&s)
        .await;
    w.run(&["session", "check"], false, "HTTP 500");
    let s = slate_server().await;
    w.config("slate", &s.uri(), "");
    let config = std::fs::read_to_string(w.dir.join("admit.toml")).expect("config");
    w.write("admit.toml", &format!("{config}\napplication='app'"));
    w.run(&["pull"], true, "wrote");
    // HAR authentication takes precedence when no environment/Keychain cookie exists.
    let har = w.dir.join("har");
    std::fs::create_dir(&har).expect("har");
    std::fs::write(har.join("slate.har"),json!({"log":{"entries":[{"startedDateTime":"2026-10-06","request":{"url":format!("{}/apply/review",s.uri()),"headers":[{"name":"cookie","value":"_uid=test"}]}}]}}).to_string()).expect("har");
    w.write("admit.toml", &format!("{config}\napplication='app'\nhar_dir='{}'", har.display()));
    let out = w.command(&["get", "aca"]).env_remove("TEST_COOKIE").output().expect("har");
    check(&out, true, "Academic History");
    Mock::given(method("GET"))
        .and(path("/apply/review"))
        .respond_with(ResponseTemplate::new(302).insert_header("location", "/account/login"))
        .with_priority(2)
        .mount(&s)
        .await;
    w.run(&["session", "check"], false, "session has expired");
    Mock::given(method("GET"))
        .and(path("/apply/review"))
        .respond_with(ResponseTemplate::new(500))
        .with_priority(1)
        .mount(&s)
        .await;
    w.run(&["session", "check"], false, "HTTP 500");
})
    .await
}

#[tokio::test]
async fn website_violations_and_status_without_an_update_are_reported() {
    tokio::task::unconstrained(async move {
    let s = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            "<script type=\"application/ld+json\">{\"@type\":\"Person\",\"hasCredential\":[{\"name\":\"BLS\",\"recognizedBy\":{\"name\":\"Board\"},\"credentialCategory\":\"active (expires sep 2020)\"}]}</script>",
        ))
        .mount(&s)
        .await;
    let w = Workspace::new();
    w.config("liaison", &s.uri(), &format!("[website]\nurl='{}'", s.uri()));
    w.write("profile/licenses.toml", "[[license]]\nkind='certification'\ntitle='BLS'\norganization='Board'\n");
    w.run(&["website"], false, "VIOLATION");
    w.config("liaison", &s.uri(), "[website]\nurl='invalid url'");
    w.run(&["website"], false, "fetching invalid url");
    let s = slate_server().await;
    w.config("slate", &s.uri(), "");
    let config = std::fs::read_to_string(w.dir.join("admit.toml")).expect("config");
    w.write("admit.toml", &format!("{config}\napplication='app'\nsubmitted=true"));
    Mock::given(method("GET"))
        .and(path("/apply/status"))
        .respond_with(
            ResponseTemplate::new(200).set_body_string("<div id=\"part_checklist_Application\"><table></table></div>"),
        )
        .mount(&s)
        .await;
    w.run(&["status"], true, "no update posted");
    Mock::given(method("GET")).and(path("/apply/status")).respond_with(ResponseTemplate::new(200).set_body_string("<div id=\"part_checklist_Application\"><table></table></div>An update to your application was last posted June 2, 2026.")).with_priority(1).mount(&s).await;
    Mock::given(method("GET"))
        .and(path("/apply/update"))
        .respond_with(ResponseTemplate::new(200).set_body_string("No PDF"))
        .mount(&s)
        .await;
    w.run(&["status"], true, "update posted June 2, 2026");
})
    .await
}

#[tokio::test]
async fn overview_and_slate_failures_stay_on_the_error_paths() {
    tokio::task::unconstrained(async move {
        let s = support::portal().await;
        let w = Workspace::new();
        w.config("liaison", &s.uri(), "");
        std::fs::create_dir(w.dir.join("profile/personal.toml")).expect("not a file");
        w.run(&["audit"], false, "personal.toml");
        std::fs::remove_dir(w.dir.join("profile/personal.toml")).expect("undo");

        Mock::given(method("GET"))
            .and(path("/applicant/api/programsSelected"))
            .respond_with(ResponseTemplate::new(500).set_body_string("no"))
            .with_priority(1)
            .mount(&s)
            .await;
        w.run(&["audit"], false, "UNCHECKED");

        let s = support::portal().await;
        w.config("liaison", &s.uri(), "");
        Mock::given(method("GET"))
            .and(path("/applicant/api/getProgramNavigation"))
            .respond_with(ResponseTemplate::new(500).set_body_string("no"))
            .with_priority(1)
            .mount(&s)
            .await;
        w.run(&["audit"], false, "UNCHECKED");
        // `plan` reads program navigation too (prerequisites).
        w.run(&["plan"], false, "GET /getProgramNavigation: HTTP 500");

        let s = support::portal().await;
        w.config("liaison", &s.uri(), "");
        let out = w
            .command(&["plan"])
            .env("TEST_COOKIE", "")
            .env("TEST_STORED_COOKIE", "SESSION=test")
            .output()
            .expect("empty cookie");
        check(&out, true, "0 change(s)");
        let har = w.dir.join("empty-har");
        std::fs::create_dir(&har).expect("har");
        let config = std::fs::read_to_string(w.dir.join("admit.toml")).expect("config");
        w.write("admit.toml", &format!("{config}\nhar_dir='{}'", har.display()));
        let out = w.command(&["plan"]).env_remove("TEST_COOKIE").output().expect("har");
        check(&out, false, "no .har file");

        let s = slate_server().await;
        w.config("slate", &s.uri(), "");
        let out = w.command(&["audit"]).env_remove("TEST_COOKIE").output().expect("no session");
        check(&out, false, "UNCHECKED");
        let config = std::fs::read_to_string(w.dir.join("admit.toml")).expect("config");
        w.write("admit.toml", &format!("{config}\napplication='app'"));
        w.run(&["pull"], true, "wrote");
        let with_app = std::fs::read_to_string(w.dir.join("admit.toml")).expect("config");
        w.write("admit.toml", &format!("{with_app}\ndeadline='not-a-date'"));
        w.run(&["audit"], false, "not YYYY-MM-DD");
        w.run(&["validate"], true, "0 finding(s)");
        let out = w
            .command(&["get", "aca"])
            .env("TEST_COOKIE", "")
            .env("TEST_STORED_COOKIE", "_uid=test")
            .output()
            .expect("empty slate cookie");
        check(&out, true, "Academic History");
        Mock::given(method("GET"))
            .and(path("/apply/aca"))
            .and(wiremock::matchers::query_param("cmd", "fetch_cities"))
            .respond_with(ResponseTemplate::new(500).set_body_string("no"))
            .with_priority(1)
            .mount(&s)
            .await;
        w.run(&["schools", "US:CA"], false, "HTTP 500");
        Mock::given(method("GET"))
            .and(path("/apply/aca"))
            .and(wiremock::matchers::query_param("cmd", "fetch_schools"))
            .respond_with(ResponseTemplate::new(500).set_body_string("no"))
            .with_priority(1)
            .mount(&s)
            .await;
        w.run(&["schools", "US:CA", "Los Angeles"], false, "HTTP 500");
        Mock::given(method("GET"))
            .and(path("/apply/aca"))
            .and(wiremock::matchers::query_param_is_missing("cmd"))
            .respond_with(ResponseTemplate::new(500).set_body_string("no"))
            .with_priority(1)
            .mount(&s)
            .await;
        w.run(&["get", "aca"], false, "HTTP 500");
        Mock::given(method("GET"))
            .and(path("/manage/service/lookup"))
            .respond_with(ResponseTemplate::new(500).set_body_string("no"))
            .with_priority(1)
            .mount(&s)
            .await;
        w.run(&["lookup", "o1", "Yale"], false, "HTTP 500");
        let out = w.command(&["lookup", "o1", "Yale"]).env_remove("TEST_COOKIE").output().expect("lookup session");
        check(&out, false, "no session");
        let out = w.command(&["schools", "US:CA"]).env_remove("TEST_COOKIE").output().expect("schools session");
        check(&out, false, "no session");
        w.write("admit.toml", &format!("{config}\napplication='app'\nhar_dir='{}'", har.display()));
        let out = w.command(&["get", "aca"]).env_remove("TEST_COOKIE").output().expect("slate har");
        check(&out, false, "no .har file");
    })
    .await
}

async fn http_500(server: &MockServer, verb: &str, endpoint: &str) {
    Mock::given(method(verb))
        .and(path(endpoint))
        .respond_with(ResponseTemplate::new(500).set_body_string("no"))
        .with_priority(1)
        .mount(server)
        .await;
}

fn coursework_text() -> &'static str {
    "2026 Fall (Sep 1, 2026 - Dec 21, 2026)\nBIO 1 LAVC Biology 3.00 3.00 A 12 CSU/UC\nTerm GPA 4.00 Term Totals 3.00\n"
}

fn prepare_coursework(w: &Workspace, url: &str, syncing: bool) {
    w.script("pdftotext", "cat \"$2\"");
    w.write("profile/laccd.pdf", coursework_text());
    w.write(
        "profile/coursework.toml",
        "[[transcript]]\npath='laccd.pdf'\nparser='laccd'\nsummer_term='Summer'\ncreate_missing_colleges=true\nacademic_status='Freshman'\n[transcript.colleges]\nLAVC='School'\n[transcript.subjects]\n'BIO 1'='Art'\n",
    );
    if syncing {
        std::fs::create_dir(w.dir.join("inbox")).expect("inbox");
        w.config(
            "liaison",
            url,
            &format!(
                "[sync]\ninbox='{}'\nprefix='transcript'\ntranscript='laccd.pdf'\nreports='reports'",
                w.dir.join("inbox").display()
            ),
        );
    } else {
        w.config("liaison", url, "");
    }
    w.record();
}

async fn mount_coursework(s: &MockServer, fail: &'static str) {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    let phase = Arc::new(AtomicUsize::new(0));
    let reads = Arc::new(AtomicUsize::new(0));
    let college = json!([{
        "id": 8, "college": "School", "collegeCode": "code", "stillAttending": false,
        "collegeEndTerm": "Fall", "collegeEndYear": 2026
    }]);
    let phase_r = phase.clone();
    let reads_r = reads.clone();
    let listed = college.clone();
    let deny_replan = fail == "replan";
    let deny_second_plan = fail == "second-plan";
    let stick = fail == "stick";
    Mock::given(method("GET"))
        .and(path("/applicant/api/academicHistory/courseWork"))
        .respond_with(move |_: &wiremock::Request| {
            let n = reads_r.fetch_add(1, Ordering::SeqCst);
            // Reads: the recording, the plan, the pre-write shape check, the
            // re-plan, then the second pass's plan (the one to fail).
            if deny_second_plan && n >= 4 {
                return ResponseTemplate::new(500).set_body_string("no");
            }
            if deny_replan && phase_r.load(Ordering::SeqCst) > 0 {
                return ResponseTemplate::new(500).set_body_string("no");
            }
            let body = if stick || phase_r.load(Ordering::SeqCst) > 0 { listed.clone() } else { json!([]) };
            ResponseTemplate::new(200).set_body_json(body)
        })
        .with_priority(1)
        .mount(s)
        .await;
    Mock::given(method("GET"))
        .and(path("/applicant/api/academicHistory/courseWork/summary/8"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(
                json!({"terms":[{"id":20,"term":"Fall","year":2026,"termInProgress":false,"courses":[]}]}),
            ),
        )
        .with_priority(1)
        .mount(s)
        .await;
    Mock::given(method("GET"))
        .and(path("/applicant/api/lookup/colleges"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([{"text":"School","code":"code"}])))
        .mount(s)
        .await;
    let phase_w = phase.clone();
    let deny_create = fail == "create";
    Mock::given(method("POST"))
        .and(path("/applicant/api/academicHistory/colleges"))
        .respond_with(move |_: &wiremock::Request| {
            if deny_create {
                return ResponseTemplate::new(500).set_body_string("no");
            }
            phase_w.store(1, Ordering::SeqCst);
            ResponseTemplate::new(200).set_body_json(json!({"id": 8}))
        })
        .with_priority(1)
        .mount(s)
        .await;
    let deny_second_apply = fail == "second-apply";
    Mock::given(method("POST"))
        .and(path("/applicant/api/academicHistory/courses/collegeId/8/termId/20"))
        .respond_with(move |_: &wiremock::Request| {
            if deny_second_apply {
                ResponseTemplate::new(500).set_body_string("no")
            } else {
                ResponseTemplate::new(200).set_body_json(json!({"id": 30}))
            }
        })
        .with_priority(1)
        .mount(s)
        .await;
}

fn login_stdin(w: &Workspace, extra: &[(&str, &str)]) -> Output {
    let mut cmd = w.command(&["session", "login"]);
    for (k, v) in extra {
        cmd.env(k, v);
    }
    let mut child = cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().expect("login");
    child.stdin.take().expect("stdin").write_all(b"user\npassword\n").expect("input");
    child.wait_with_output().expect("output")
}

#[tokio::test]
async fn liaison_errors_are_each_calls_own_failure() {
    tokio::task::unconstrained(async move {
    let s = support::portal().await;
    let w = Workspace::new();
    w.config("liaison", &s.uri(), "");
    http_500(&s, "GET", "/applicant/api/lookup/countries").await;
    w.run(&["plan"], false, "HTTP 500");
    w.run(&["apply"], false, "HTTP 500");
    w.run(&["pull"], false, "HTTP 500");

    let s = support::portal().await;
    let w = Workspace::new();
    w.config("liaison", &s.uri(), "");
    w.write("profile/achievements.toml", "not = [");
    w.run(&["plan"], false, "parsing");

    let s = support::portal().await;
    let w = Workspace::new();
    w.config("liaison", &s.uri(), "");
    http_500(&s, "GET", "/applicant/api/supportingInfo/achievements").await;
    w.run(&["plan"], false, "HTTP 500");
    w.run(&["apply"], false, "HTTP 500");
    w.run(&["pull"], false, "HTTP 500");

    let s = support::portal().await;
    let w = Workspace::new();
    w.config("liaison", &s.uri(), "");
    http_500(&s, "GET", "/applicant/api/academicHistory/courseWork").await;
    w.run(&["courses"], false, "HTTP 500");
    http_500(&s, "GET", "/applicant/api/getProgramNavigation").await;
    w.run(&["-p", "test", "audit"], false, "HTTP 500");

    let s = support::portal().await;
    let w = Workspace::new();
    w.config("liaison", &s.uri(), "");
    http_500(&s, "GET", "/applicant/api/lookup/countries").await;
    w.run(&["-p", "test", "audit"], false, "HTTP 500");
    let s = support::portal().await;
    w.config("liaison", &s.uri(), "");
    http_500(&s, "GET", "/applicant/api/supportingInfo/achievements").await;
    w.run(&["-p", "test", "audit"], false, "HTTP 500");

    let s = support::portal().await;
    let w = Workspace::new();
    w.config("liaison", &s.uri(), "");
    std::fs::remove_dir_all(w.dir.join("profile")).expect("profile dir");
    std::fs::write(w.dir.join("profile"), "not a directory").expect("file");
    w.run(&["pull"], false, "File exists");
    std::fs::remove_file(w.dir.join("profile")).expect("undo");
    std::fs::create_dir_all(w.dir.join("profile/high_school.toml")).expect("blocks the write");
    w.run(&["pull", "--force"], false, "Is a directory");

    w.stdin(&["session", "login"], "user\npassword\n", false, "credentials were not stored");
    http_500(&s, "POST", "/applicant/authenticate").await;
    Mock::given(method("GET"))
        .and(path("/applicant/initialize"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&s)
        .await;
    let out = login_stdin(&w, &[("TEST_USER", "user"), ("TEST_PASSWORD", "password")]);
    check(&out, false, "sign-in rejected");
    let s = support::portal().await;
    let w = Workspace::new();
    w.config("liaison", &s.uri(), "");
    Mock::given(method("GET"))
        .and(path("/applicant/initialize"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&s)
        .await;
    Mock::given(method("POST"))
        .and(path("/applicant/authenticate"))
        .respond_with(
            ResponseTemplate::new(200).insert_header("set-cookie", "SESSION=fresh; path=/").set_body_json(json!({})),
        )
        .with_priority(1)
        .mount(&s)
        .await;
    w.script(
        "security",
        "case \"$1\" in\n find-generic-password) case \"$5\" in *-user) printf '%s\\n' user;; *-password) printf '%s\\n' password;; *) exit 1;; esac;;\n *) n=0; [ -f \"$HOME/sec-n\" ] && n=$(cat \"$HOME/sec-n\"); n=$((n+1)); echo \"$n\" > \"$HOME/sec-n\"; [ \"$n\" -lt 3 ] || exit 1;;\nesac",
    );
    let out = login_stdin(&w, &[]);
    check(&out, false, "add-generic-password");
    let s = support::portal().await;
    let w = Workspace::new();
    w.config("liaison", &s.uri(), "");
    Mock::given(method("GET"))
        .and(path("/applicant/initialize"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&s)
        .await;
    Mock::given(method("POST"))
        .and(path("/applicant/authenticate"))
        .respond_with(
            ResponseTemplate::new(200).insert_header("set-cookie", "SESSION=fresh; path=/").set_body_json(json!({})),
        )
        .with_priority(1)
        .mount(&s)
        .await;
    http_500(&s, "GET", "/applicant/api/navigation").await;
    let out = login_stdin(&w, &[("TEST_KEYCHAIN_CREDS", "1")]);
    check(&out, false, "HTTP 500");
})
    .await
}

#[tokio::test]
async fn coursework_passes_fail_for_apply_and_for_sync() {
    tokio::task::unconstrained(async move {
    for fail in ["create", "replan", "second-apply", "second-plan"] {
        for syncing in [false, true] {
            let s = support::portal().await;
            mount_coursework(&s, fail).await;
            let w = Workspace::new();
            prepare_coursework(&w, &s.uri(), syncing);
            let expected = "HTTP 500";
            if syncing {
                w.run(&["sync"], false, expected);
            } else {
                w.run(&["apply", "--only", "coursework", "--yes"], false, expected);
            }
        }
    }
    let s = support::portal().await;
    mount_coursework(&s, "stick").await;
    let w = Workspace::new();
    prepare_coursework(&w, &s.uri(), true);
    w.run(&["sync"], false, "remain after apply");

    let s = support::portal().await;
    let w = Workspace::new();
    std::fs::create_dir(w.dir.join("inbox")).expect("inbox");
    w.config(
        "liaison",
        &s.uri(),
        &format!(
            "[sync]\ninbox='{}'\nprefix='transcript'\ntranscript='laccd.pdf'\nreports='reports'",
            w.dir.join("inbox").display()
        ),
    );
    std::fs::create_dir(w.dir.join("reports")).expect("reports");
    std::fs::create_dir(w.dir.join("reports/last-sync")).expect("blocks the stamp");
    w.run(&["sync"], false, "Is a directory");

    w.script("pdftotext", "cat \"$2\"");
    let old = coursework_text();
    w.write("profile/laccd.pdf", old);
    let installed = w.dir.join("profile/laccd.pdf");
    let when = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000);
    std::fs::File::options().write(true).open(&installed).expect("open").set_modified(when).expect("mtime");
    let date = time::OffsetDateTime::from(when).date();
    std::fs::create_dir(w.dir.join(format!("profile/laccd-{date}.pdf"))).expect("blocks the backup");
    w.write("inbox/transcript-new.pdf", &old.replace("3.00 3.00 A 12", "3.00 3.00 B 9"));
    std::fs::remove_dir(w.dir.join("reports/last-sync")).expect("stamp");
    w.run(&["sync"], false, "Is a directory");

    std::fs::remove_dir_all(w.dir.join("profile")).expect("profile");
    std::fs::create_dir(w.dir.join("profile")).expect("profile");
    std::fs::set_permissions(w.dir.join("profile"), std::fs::Permissions::from_mode(0o555)).expect("mode");
    w.write("inbox/transcript-newer.pdf", old);
    let denied = w.command(&["sync"]).output().expect("sync");
    std::fs::set_permissions(w.dir.join("profile"), std::fs::Permissions::from_mode(0o755)).expect("restore");
    check(&denied, false, "Permission denied");

    let s = support::portal().await;
    let w = Workspace::new();
    std::fs::create_dir(w.dir.join("inbox")).expect("inbox");
    w.config(
        "liaison",
        &s.uri(),
        &format!(
            "[sync]\ninbox='{}'\nprefix='transcript'\ntranscript='laccd.pdf'\nreports='reports'",
            w.dir.join("inbox").display()
        ),
    );
    w.write(
        "profile/coursework.toml",
        "[[transcript]]\npath='missing.pdf'\nparser='laccd'\nsummer_term='Summer'\nacademic_status='Freshman'\n[transcript.colleges]\nLAVC='School'\n",
    );
    http_500(&s, "GET", "/applicant/api/lookup/countries").await;
    w.run(&["sync"], false, "HTTP 500");
    let s = support::portal().await;
    w.config(
        "liaison",
        &s.uri(),
        &format!(
            "[sync]\ninbox='{}'\nprefix='transcript'\ntranscript='laccd.pdf'\nreports='reports'",
            w.dir.join("inbox").display()
        ),
    );
    w.script("pdftotext", "cat \"$2\"");
    w.write("profile/laccd.pdf", coursework_text());
    w.write(
        "profile/coursework.toml",
        "[[transcript]]\npath='laccd.pdf'\nparser='laccd'\nsummer_term='Summer'\nacademic_status='Freshman'\n[transcript.colleges]\nLAVC='School'\n[transcript.subjects]\n'BIO 1'='Art'\n",
    );
    http_500(&s, "GET", "/applicant/api/academicHistory/courseWork").await;
    w.run(&["sync"], false, "HTTP 500");
})
    .await
}

#[tokio::test]
async fn a_new_college_is_followed_by_its_courses() {
    tokio::task::unconstrained(async move {
        let s = support::portal().await;
        mount_coursework(&s, "pass").await;
        let w = Workspace::new();
        prepare_coursework(&w, &s.uri(), true);
        w.run(&["sync"], false, "remain after apply");

        // Unattended too: no recording, no writes.
        let s = support::portal().await;
        mount_coursework(&s, "pass").await;
        let w = Workspace::new();
        prepare_coursework(&w, &s.uri(), true);
        std::fs::remove_file(w.dir.join("shapes/test.json")).expect("recording");
        w.run(&["sync"], false, "writes are checked against the recorded response shapes first");
        let wrote = s
            .received_requests()
            .await
            .expect("requests")
            .iter()
            .any(|r| r.method.as_str() != "GET" && !r.url.path().ends_with("/search/filteredQueries"));
        assert!(!wrote, "nothing written");
    })
    .await
}

#[tokio::test]
async fn setting_a_session_reports_a_failed_check() {
    tokio::task::unconstrained(async move {
        let s = support::portal().await;
        http_500(&s, "GET", "/applicant/api/navigation").await;
        let w = Workspace::new();
        w.config("liaison", &s.uri(), "");
        w.stdin(&["session", "set"], "Cookie: SESSION=test\n", false, "HTTP 500");
    })
    .await
}

#[tokio::test]
async fn slate_errors_skip_blank_pages_and_stop_on_a_violation() {
    tokio::task::unconstrained(async move {
    let s = slate_server().await;
    let w = Workspace::new();
    w.config("slate", &s.uri(), "");
    let config = std::fs::read_to_string(w.dir.join("admit.toml")).expect("config");
    w.write("admit.toml", &format!("{config}\napplication='app'"));
    w.run(&["pull"], true, "wrote");
    let answers = std::fs::read_to_string(w.dir.join("profile/slate/test.toml")).expect("answers");
    http_500(&s, "GET", "/apply/review").await;
    w.run(&["plan"], false, "HTTP 500");
    w.run(&["questions"], false, "HTTP 500");

    let s = slate_server().await;
    let w = Workspace::new();
    w.config("slate", &s.uri(), "");
    let cfg = std::fs::read_to_string(w.dir.join("admit.toml")).expect("config");
    w.write("admit.toml", &format!("{cfg}\napplication='app'"));
    std::fs::create_dir_all(w.dir.join("profile/slate")).expect("answers dir");
    w.write(
        "profile/slate/test.toml",
        "[[institutions]]\nname='Connecticut College'\nkey='003284'\ncity='New London'\nregion='CT'\ncountry='US'\nfrom='2017-08'\nto='2021-05'\nlevel='U'\ntranscript='t.pdf'\n",
    );
    http_500(&s, "GET", "/apply/aca").await;
    w.run(&["plan"], false, "HTTP 500");

    let s = slate_server().await;
    let w = Workspace::new();
    w.config("slate", &s.uri(), "");
    let cfg = std::fs::read_to_string(w.dir.join("admit.toml")).expect("config");
    w.write("admit.toml", &format!("{cfg}\napplication='app'"));
    std::fs::create_dir_all(w.dir.join("profile/slate")).expect("answers dir");
    w.write(
        "profile/slate/test.toml",
        "[[jobs]]\nemployer='Clinic'\ntitle='Care'\nfrom='2020-01'\nhours=10\ndescription='Care'\n",
    );
    http_500(&s, "GET", "/apply/job").await;
    w.run(&["plan"], false, "HTTP 500");

    let s = slate_server().await;
    let w = Workspace::new();
    w.config("slate", &s.uri(), "");
    let cfg = std::fs::read_to_string(w.dir.join("admit.toml")).expect("config");
    w.write("admit.toml", &format!("{cfg}\napplication='app'"));
    std::fs::create_dir_all(w.dir.join("profile/slate")).expect("answers dir");
    w.write("profile/slate/test.toml", &answers);
    w.write("profile/achievements.toml", "not = [");
    w.run(&["apply"], false, "parsing");
    w.write("profile/achievements.toml", "[[achievement]]\nkind='award'\nname=''\nstate='California'\n");
    w.run(&["-p", "test", "audit"], false, "no name");

    std::fs::remove_dir_all(w.dir.join("profile/slate")).expect("answers");
    std::fs::write(w.dir.join("profile/slate"), "not a directory").expect("file");
    let s = slate_server().await;
    w.config("slate", &s.uri(), "");
    let cfg = std::fs::read_to_string(w.dir.join("admit.toml")).expect("config");
    w.write("admit.toml", &format!("{cfg}\napplication='app'"));
    w.run(&["pull"], false, "File exists");
    std::fs::remove_file(w.dir.join("profile/slate")).expect("undo");
    std::fs::create_dir_all(w.dir.join("profile/slate/test.toml")).expect("blocks the write");
    w.run(&["pull", "--force"], false, "Is a directory");

    w.stdin(&["session", "set"], "not a cookie", false, "Cookie header");
    w.stdin(&["session", "login"], "", false, "two lines");
    w.stdin(&["session", "login"], "user\npassword\n", false, "credentials were not stored");
    Mock::given(method("GET")).and(path("/account/login")).respond_with(ResponseTemplate::new(200)).mount(&s).await;
    Mock::given(method("POST")).and(path("/account/login")).respond_with(ResponseTemplate::new(500)).mount(&s).await;
    let out = login_stdin(&w, &[("TEST_USER", "user"), ("TEST_PASSWORD", "password")]);
    check(&out, false, "sign-in rejected");
    let out = w.command(&["session", "clear"]).env("TEST_SECURITY_STATUS", "1").output().expect("clear");
    check(&out, false, "no stored session");

    let s = slate_server().await;
    let w = Workspace::new();
    w.config("slate", &s.uri(), "");
    let cfg = std::fs::read_to_string(w.dir.join("admit.toml")).expect("config");
    w.write("admit.toml", &format!("{cfg}\napplication='app'\nsubmitted=true"));
    for (p, body) in [
        ("/apply/status", include_str!("../../slate/tests/fixtures/apu_status.html")),
        ("/apply/update", include_str!("../../slate/tests/fixtures/apu_update.html")),
    ] {
        Mock::given(method("GET"))
            .and(path(p))
            .respond_with(ResponseTemplate::new(200).set_body_string(body))
            .mount(&s)
            .await;
    }
    std::fs::write(w.dir.join("reports"), "not a directory").expect("reports");
    w.run(&["status"], false, "File exists");
    std::fs::remove_file(w.dir.join("reports")).expect("undo");
    std::fs::create_dir(w.dir.join("reports")).expect("reports");
    let streams = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let streams_r = streams.clone();
    Mock::given(method("GET"))
        .and(path("/apply/update"))
        .and(wiremock::matchers::query_param("cmd", "stream"))
        .respond_with(move |_: &wiremock::Request| {
            if streams_r.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
                ResponseTemplate::new(500).set_body_string("no")
            } else {
                ResponseTemplate::new(200).set_body_bytes(b"%PDF".to_vec())
            }
        })
        .with_priority(1)
        .mount(&s)
        .await;
    w.run(&["status"], false, "HTTP 500");
    let posted = "June 2, 2026";
    let slug: String = posted.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
    std::fs::create_dir(w.dir.join(format!("reports/test-update-{slug}.pdf"))).expect("blocks the pdf");
    w.run(&["status"], false, "Is a directory");

    let s = MockServer::start().await;
    let review = "<h1>Review</h1><div id=\"menu\"><a href=\"frm?page=basic\" onclick=\"return apply_navigate(this);\">Basic</a><a href=\"frm?page=locked\" onclick=\"return apply_navigate(this);\">Locked</a></div></ul>";
    let basic = "<h1>Basic</h1><div class=\"form_question\" data-id=\"name\" data-type=\"text\" data-export=\"sys:first\"><label>Name</label><input name=\"form_name\" value=\"Pat\"></div>";
    let locked = "<h1>Locked</h1><div class=\"form_question\" data-id=\"x\" data-type=\"text\" data-export=\"sys:x\" data-readonly=\"1\"><label>X</label><input name=\"form_x\" value=\"a\"></div>";
    Mock::given(method("GET"))
        .and(path("/apply/review"))
        .respond_with(ResponseTemplate::new(200).set_body_string(review))
        .mount(&s)
        .await;
    Mock::given(method("GET"))
        .and(path("/apply/frm"))
        .and(wiremock::matchers::query_param("page", "basic"))
        .respond_with(ResponseTemplate::new(200).set_body_string(basic))
        .mount(&s)
        .await;
    Mock::given(method("GET"))
        .and(path("/apply/frm"))
        .and(wiremock::matchers::query_param("page", "locked"))
        .respond_with(ResponseTemplate::new(200).set_body_string(locked))
        .mount(&s)
        .await;
    Mock::given(method("POST"))
        .and(path("/apply/"))
        .respond_with(ResponseTemplate::new(200).insert_header("set-cookie", ".application=app; path=/"))
        .mount(&s)
        .await;
    let w = Workspace::new();
    w.config("slate", &s.uri(), "");
    let cfg = std::fs::read_to_string(w.dir.join("admit.toml")).expect("config");
    w.write("admit.toml", &format!("{cfg}\napplication='app'"));
    let out = w.command(&["questions"]).output().expect("questions");
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success(), "{text}");
    assert!(text.contains("sys:first"), "{text}");
    assert!(!text.contains("Locked"), "{text}");
})
    .await
}

#[test]
fn a_closed_stdin_and_a_refused_keychain_are_errors() {
    let w = Workspace::new();
    w.config("liaison", "http://127.0.0.1:1", "");
    let read = |args: &[&str], needle: &str| {
        let child = w
            .command(args)
            .stdin(std::fs::File::open(&w.dir).expect("dir"))
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn");
        check(&child.wait_with_output().expect("output"), false, needle);
    };
    read(&["session", "set"], "reading the cookie");
    read(&["session", "login"], "reading credentials");
    let mut child = w
        .command(&["session", "login"])
        .env("TEST_SECURITY_STATUS", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("login");
    child.stdin.take().expect("stdin").write_all(b"user\nsecret\n").expect("input");
    check(&child.wait_with_output().expect("output"), false, "add-generic-password");
}

#[tokio::test]
async fn each_liaison_error_is_that_calls_own() {
    tokio::task::unconstrained(async move {
        let w = Workspace::new();
        w.config("liaison", "http://127.0.0.1:9", "");
        w.run(&["apply", "--only", "nope"], false, "unknown section");
        w.write("profile/achievements.toml", "not = [");
        w.run(&["validate"], false, "parsing");
        w.run(&["apply"], false, "parsing");
        w.run(&["-p", "test", "audit"], false, "parsing");

        let s = support::portal().await;
        http_500(&s, "GET", "/applicant/api/navigation").await;
        let w = Workspace::new();
        w.config("liaison", &s.uri(), "");
        w.run(&["apply", "--only", "achievements"], false, "HTTP 500");
        w.run(&["apply", "--only", "education"], false, "HTTP 500");
        w.run(&["pull"], false, "HTTP 500");
        w.run(&["courses"], false, "HTTP 500");
        w.run(&["-p", "test", "audit"], false, "HTTP 500");
    })
    .await
}

#[tokio::test]
async fn a_missing_inbox_is_reported() {
    tokio::task::unconstrained(async move {
        let w = Workspace::new();
        w.config(
            "liaison",
            "http://127.0.0.1:9",
            "[sync]\ninbox='no-such-inbox'\nprefix='laccd'\ntranscript='laccd.pdf'\nreports='reports'\nfetch='laccd'",
        );
        w.run(&["sync"], false, "reading");
    })
    .await
}

fn slate_submitted(w: &Workspace, url: &str) {
    w.config("slate", url, "");
    let config = std::fs::read_to_string(w.dir.join("admit.toml")).expect("config");
    w.write("admit.toml", &format!("{config}\nsubmitted=true"));
}

fn slate_app(w: &Workspace, url: &str) {
    w.config("slate", url, "");
    let config = std::fs::read_to_string(w.dir.join("admit.toml")).expect("config");
    w.write("admit.toml", &format!("{config}\napplication='app'\n"));
}

fn slate_answers(w: &Workspace, body: &str) {
    std::fs::create_dir_all(w.dir.join("profile/slate")).expect("dir");
    w.write("profile/slate/test.toml", body);
}

const SCHOOL: &str = "[[institutions]]\nname='Connecticut College'\nkey='003284'\ncity='New London'\nregion='CT'\ncountry='US'\nfrom='2017-08'\nto='2021-05'\nlevel='U'\ndegree='Bachelor of Arts'\nconferred='2021-05'\ntranscript='t.pdf'\n";
const JOB: &str = "[[jobs]]\nemployer='Clinic'\ntitle='Care'\nfrom='2020-01'\nhours=10\ndescription='Care'\n";

fn slate_menu(links: &str) -> String {
    format!(
        "<div id=\"menu\">{links}<a href=\"review\" onclick=\"return apply_navigate(this);\">Review</a></div></ul><h1>Review</h1>"
    )
}

fn basic_page() -> &'static str {
    "<h1>Basic</h1><div class=\"form_question\" data-id=\"name\" data-type=\"text\" data-export=\"sys:first\" data-required=\"1\"><label>Name</label><input name=\"form_name\" value=\"Pat\"></div>"
}

async fn mount_open(s: &MockServer) {
    Mock::given(method("POST"))
        .and(path("/apply/"))
        .respond_with(ResponseTemplate::new(200).insert_header("set-cookie", ".application=app; path=/"))
        .mount(s)
        .await;
}

async fn mount_review(s: &MockServer, body: &str) {
    Mock::given(method("GET"))
        .and(path("/apply/review"))
        .respond_with(ResponseTemplate::new(200).set_body_string(body.to_string()))
        .mount(s)
        .await;
}

async fn mount_basic(s: &MockServer) {
    Mock::given(method("GET"))
        .and(path("/apply/frm"))
        .respond_with(ResponseTemplate::new(200).set_body_string(basic_page()))
        .mount(s)
        .await;
}

async fn status_then_fail(s: &MockServer) {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let seen = std::sync::Arc::new(AtomicUsize::new(0));
    Mock::given(method("GET"))
        .and(path("/apply/status"))
        .respond_with(move |_: &wiremock::Request| {
            if seen.fetch_add(1, Ordering::SeqCst) >= 1 {
                ResponseTemplate::new(500).set_body_string("no")
            } else {
                ResponseTemplate::new(200).set_body_string(include_str!("../../slate/tests/fixtures/apu_status.html"))
            }
        })
        .mount(s)
        .await;
}

async fn nth_review(s: &MockServer, body: String, fail_at: usize) {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let n = std::sync::Arc::new(AtomicUsize::new(0));
    Mock::given(method("GET"))
        .and(path("/apply/review"))
        .respond_with(move |_: &wiremock::Request| {
            if n.fetch_add(1, Ordering::SeqCst) >= fail_at {
                ResponseTemplate::new(500).set_body_string("no")
            } else {
                ResponseTemplate::new(200).set_body_string(body.clone())
            }
        })
        .with_priority(1)
        .mount(s)
        .await;
}

async fn slate_pages(links: &str) -> MockServer {
    let s = MockServer::start().await;
    mount_open(&s).await;
    mount_review(&s, &slate_menu(links)).await;
    mount_basic(&s).await;
    s
}

async fn listed_colleges(s: &MockServer) {
    use wiremock::matchers::query_param;
    Mock::given(method("GET"))
        .and(path("/apply/aca"))
        .and(wiremock::matchers::query_param_is_missing("id"))
        .respond_with(ResponseTemplate::new(200).set_body_string(include_str!("../../slate/tests/fixtures/aca.html")))
        .mount(s)
        .await;
    Mock::given(method("GET"))
        .and(path("/apply/aca"))
        .and(query_param("id", ""))
        .respond_with(
            ResponseTemplate::new(200).set_body_string(include_str!("../../slate/tests/fixtures/aca_add.html")),
        )
        .with_priority(1)
        .mount(s)
        .await;
}

/// Every error the production binary can reach: a missing cookie and a refused read.
#[tokio::test]
async fn each_slate_error_is_that_calls_own() {
    tokio::task::unconstrained(async move {
    let dead = "http://127.0.0.1:9";
    let w = Workspace::new();
    slate_app(&w, dead);
    for cmd in ["plan", "apply", "pull", "questions", "status", "session"] {
        let args: &[&str] = if cmd == "session" { &["session", "check"] } else { &[cmd] };
        // one workspace, cookie removed each time
        w.run_env(args, &[], &["TEST_COOKIE"], false, "no session");
    }
    w.write("profile/achievements.toml", "not = [");
    w.run(&["-p", "test", "audit"], false, "parsing");

    let links = "<a href=\"frm?page=basic\" onclick=\"return apply_navigate(this);\">Basic</a>";
    let s = slate_pages(links).await;
    let w = Workspace::new();
    slate_app(&w, &s.uri());
    w.run(&["apply"], false, "create it with");
    http_500(&s, "GET", "/apply/review").await;
    slate_answers(&w, "[answers]\n'sys:first'='Pat'\n");
    w.run(&["apply"], false, "HTTP 500");
    w.run(&["pull", "--force"], false, "HTTP 500");

    let s = slate_pages(links).await;
    http_500(&s, "GET", "/apply/aca").await;
    let w = Workspace::new();
    slate_app(&w, &s.uri());
    slate_answers(&w, SCHOOL);
    w.write("profile/t.pdf", "%PDF");
    w.run(&["plan"], false, "HTTP 500");
    w.run(&["apply", "--yes"], false, "HTTP 500");

    let s = slate_pages(links).await;
    http_500(&s, "GET", "/apply/job").await;
    let w = Workspace::new();
    slate_app(&w, &s.uri());
    slate_answers(&w, JOB);
    w.run(&["plan"], false, "HTTP 500");
    w.run(&["apply", "--yes"], false, "HTTP 500");

    let info = "<a href=\"frm?page=basic\" onclick=\"return apply_navigate(this);\">Basic</a><a href=\"frm?page=info\" onclick=\"return apply_navigate(this);\">Info</a>";
    let s = slate_pages(info).await;
    listed_colleges(&s).await;
    use std::sync::atomic::{AtomicUsize, Ordering};
    let n = std::sync::Arc::new(AtomicUsize::new(0));
    let seen = n.clone();
    Mock::given(method("GET"))
        .and(path("/apply/frm"))
        .and(wiremock::matchers::query_param("page", "info"))
        .respond_with(move |_: &wiremock::Request| {
            if seen.fetch_add(1, Ordering::SeqCst) >= 1 {
                ResponseTemplate::new(500).set_body_string("no")
            } else {
                ResponseTemplate::new(200).set_body_string("<h1>Additional Academic Info</h1>")
            }
        })
        .with_priority(1)
        .mount(&s)
        .await;
    let w = Workspace::new();
    slate_app(&w, &s.uri());
    slate_answers(&w, SCHOOL);
    w.run(&["plan"], false, "HTTP 500");

    let s = slate_pages(links).await;
    Mock::given(method("POST"))
        .and(path("/apply/frm"))
        .respond_with(ResponseTemplate::new(500).set_body_string("no"))
        .with_priority(1)
        .mount(&s)
        .await;
    let w = Workspace::new();
    slate_app(&w, &s.uri());
    slate_answers(&w, "[answers]\n'sys:first'='Changed'\n");
    w.run(&["apply", "--yes"], false, "HTTP 500");

    let s = slate_pages(links).await;
    listed_colleges(&s).await;
    Mock::given(method("POST"))
        .and(path("/apply/aca"))
        .respond_with(ResponseTemplate::new(500).set_body_string("no"))
        .with_priority(1)
        .mount(&s)
        .await;
    let w = Workspace::new();
    slate_app(&w, &s.uri());
    slate_answers(&w, SCHOOL);
    w.write("profile/t.pdf", "%PDF");
    w.run(&["apply", "--yes"], false, "HTTP 500");

    let s = slate_pages(links).await;
    Mock::given(method("GET"))
        .and(path("/apply/job"))
        .respond_with(ResponseTemplate::new(200).set_body_string(include_str!("../../slate/tests/fixtures/job.html")))
        .mount(&s)
        .await;
    Mock::given(method("POST"))
        .and(path("/apply/job"))
        .respond_with(ResponseTemplate::new(500).set_body_string("no"))
        .with_priority(1)
        .mount(&s)
        .await;
    let w = Workspace::new();
    slate_app(&w, &s.uri());
    slate_answers(&w, JOB);
    w.run(&["apply", "--yes"], false, "HTTP 500");

    let s = MockServer::start().await;
    mount_open(&s).await;
    status_then_fail(&s).await;
    let w = Workspace::new();
    slate_submitted(&w, &s.uri());
    w.run(&["status"], false, "HTTP 500");
    let s = MockServer::start().await;
    mount_open(&s).await;
    status_then_fail(&s).await;
    let w = Workspace::new();
    slate_submitted(&w, &s.uri());
    w.run(&["session", "check"], false, "HTTP 500");
    let s = MockServer::start().await;
    mount_open(&s).await;
    status_then_fail(&s).await;
    let w = Workspace::new();
    slate_submitted(&w, &s.uri());
    w.run(&["-p", "test", "audit"], false, "HTTP 500");

    let menu = slate_menu(links);
    let s = slate_pages(links).await;
    nth_review(&s, menu.clone(), 1).await;
    let w = Workspace::new();
    slate_app(&w, &s.uri());
    slate_answers(&w, "[answers]\n'sys:first'='Pat'\n");
    w.run(&["apply"], false, "HTTP 500");
    let s = slate_pages(links).await;
    nth_review(&s, menu.clone(), 1).await;
    Mock::given(method("POST"))
        .and(path("/apply/frm"))
        .respond_with(ResponseTemplate::new(200).set_body_string("saved"))
        .mount(&s)
        .await;
    let w = Workspace::new();
    slate_app(&w, &s.uri());
    slate_answers(&w, "[answers]\n'sys:first'='Changed'\n");
    w.run(&["apply", "--yes"], false, "HTTP 500");
    let s = slate_pages(links).await;
    nth_review(&s, menu.clone(), 1).await;
    let w = Workspace::new();
    slate_app(&w, &s.uri());
    slate_answers(&w, "[answers]\n'sys:first'='Pat'\n");
    w.run(&["-p", "test", "audit"], false, "HTTP 500");
    let s = slate_pages(links).await;
    nth_review(&s, menu, 2).await;
    let w = Workspace::new();
    slate_app(&w, &s.uri());
    slate_answers(&w, "[answers]\n'sys:first'='Pat'\n");
    w.run(&["-p", "test", "audit"], false, "HTTP 500");
    let s = slate_pages(links).await;
    let w = Workspace::new();
    slate_app(&w, &s.uri());
    w.run(&["-p", "test", "audit"], false, "create it with");

    let s = MockServer::start().await;
    mount_open(&s).await;
    http_500(&s, "GET", "/apply/").await;
    let w = Workspace::new();
    slate_app(&w, &s.uri());
    w.run(&["session", "check"], false, "HTTP 500");

    let s = slate_pages(links).await;
    let w = Workspace::new();
    slate_app(&w, &s.uri());
    w.script(
        "security",
        "case \"$1\" in\n  find-generic-password) case \"$5\" in *-user) echo user;; *-password) echo password;; *) exit 1;; esac;;\n  add-generic-password) n=$(cat \"$HOME/n\" 2>/dev/null || echo 0); n=$((n+1)); echo \"$n\" > \"$HOME/n\"; [ \"$n\" -ge 3 ] && exit 1; exit 0;;\n  *) exit 0;;\nesac",
    );
    Mock::given(method("GET"))
        .and(path("/account/login"))
        .respond_with(ResponseTemplate::new(200).set_body_string("login"))
        .mount(&s)
        .await;
    Mock::given(method("POST"))
        .and(path("/account/login"))
        .respond_with(
            ResponseTemplate::new(302)
                .insert_header("location", "/apply/")
                .insert_header("set-cookie", "_uid=abc; path=/"),
        )
        .mount(&s)
        .await;
    w.stdin(&["session", "login"], "user\npassword\n", false, "add-generic-password");
})
    .await
}

#[tokio::test]
async fn drift_records_then_compares_and_names_a_vanished_field() {
    tokio::task::unconstrained(async move {
        let s = support::portal().await;
        let w = Workspace::new();
        w.config("liaison", &s.uri(), "");
        w.run(&["drift"], false, "no recording at ./shapes/test.json (admit drift --record)");
        w.run(&["drift", "--record"], true, "recorded 35 endpoint(s)");
        let recorded = std::fs::read_to_string(w.dir.join("shapes/test.json")).expect("recording");
        assert!(!recorded.contains("pat@example.com"), "keys and types only");
        w.run(&["drift"], true, "35 endpoint(s) compared, 0 change(s) that break readers");

        // The fake portal never sends `$.renamedAway`, so the recording says it vanished.
        let edited = recorded.replacen(
            "\"/authenticatedUser\": {",
            "\"/authenticatedUser\": {\n    \"$.renamedAway\": \"string\",",
            1,
        );
        w.write("shapes/test.json", &edited);
        w.run(&["drift"], false, "VIOLATION  /authenticatedUser: $.renamedAway (string) is gone");
        w.write("shapes/test.json", "not json");
        w.run(&["drift"], false, "parsing ./shapes/test.json");

        std::fs::remove_dir_all(w.dir.join("shapes")).expect("shapes");
        w.write("shapes", "a file where the directory goes");
        w.run(&["drift", "--record"], false, "File exists");
        std::fs::remove_file(w.dir.join("shapes")).expect("file");
        std::fs::create_dir_all(w.dir.join("shapes/test.json")).expect("a directory where the file goes");
        w.run(&["drift", "--record"], false, "directory");

        http_500(&s, "GET", "/applicant/api/lookup/year").await;
        w.run(&["drift"], false, "HTTP 500");
        let out = w.command(&["drift"]).env_remove("TEST_COOKIE").output().expect("no session");
        check(&out, false, "session");
        w.run(&["-p", "missing", "drift"], false, "no portal");
        w.config("slate", &s.uri(), "");
        w.run(&["drift"], false, "drift watches Liaison JSON");
    })
    .await
}

#[tokio::test]
async fn recs_list_each_program_and_slate_points_to_its_review() {
    tokio::task::unconstrained(async move {
        let s = support::portal().await;
        let w = Workspace::new();
        w.config("liaison", &s.uri(), "");
        w.run(&["recs"], true, "  ASK      0/0 of 1  submitted              University - Nursing");
        let out = w.command(&["recs"]).env_remove("TEST_COOKIE").output().expect("no session");
        check(&out, false, "session");
        http_500(&s, "GET", "/applicant/api/getProgramNavigation").await;
        w.run(&["recs"], false, "HTTP 500");
        w.run(&["-p", "missing", "recs"], false, "no portal");
        w.config("slate", &s.uri(), "");
        w.run(&["recs"], false, "Review page");
    })
    .await
}

#[tokio::test]
async fn ready_and_remind_group_blockers_by_program_and_fail_until_clear() {
    tokio::task::unconstrained(async move {
        let s = support::portal().await;
        let soon = (time::OffsetDateTime::now_utc().date() + time::Duration::days(10)).to_string();
        Mock::given(method("POST"))
            .and(path("/applicant/api/search/filteredQueries"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"programs": [
                {"organizationName": "University", "programName": "Nursing", "programSelectedId": 1,
                 "deadline": soon, "submissionStatus": "In Progress"}]})))
            .with_priority(1)
            .mount(&s)
            .await;
        let w = Workspace::new();
        w.config("liaison", &s.uri(), "");
        w.run(&["ready"], false, "test: NOT READY, 0 of 1 program(s) ready");
        w.run(
            &["-p", "test", "ready"],
            false,
            "    PENDING    University - Nursing: 0 of 1 recommendation(s) requested",
        );
        w.run(&["remind", "--days", "30"], false, "test: University - Nursing - NOT READY, 1 blocking here");
        w.run(&["remind", "--days", "3"], true, "no unready application is due within 3 day(s)");

        w.config("unknown", &s.uri(), "");
        w.run(&["ready"], false, "test: UNCHECKED: unknown portal kind");
        w.run(&["remind"], false, "test: UNCHECKED, its deadlines are unknown");
        w.run(&["-p", "missing", "ready"], false, "no portal");
        std::fs::create_dir(w.dir.join("profile/personal.toml")).expect("not a file");
        w.run(&["ready"], false, "personal.toml");
        w.run(&["remind"], false, "personal.toml");
    })
    .await
}

#[tokio::test]
async fn prereqs_prints_unruled_programs_with_their_assigned_courses() {
    tokio::task::unconstrained(async move {
        let s = support::portal().await;
        let w = Workspace::new();
        w.config("liaison", &s.uri(), "");
        w.run(&["prereqs"], true, "every program with prerequisites on test is ruled in prerequisites.toml");

        let fx: serde_json::Value =
            serde_json::from_str(include_str!("../../liaison/src/sections/tests/fixtures/yale_prereqs.json")).expect("fixture");
        Mock::given(method("GET"))
            .and(path("/applicant/api/getProgramNavigation"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([{"orgName": "Yale University",
                "programName": "GEPN", "programSelectedId": 79439764, "selectedProgramHasPreReqSection": true}])))
            .with_priority(1)
            .mount(&s)
            .await;
        Mock::given(method("GET"))
            .and(path("/applicant/api/programMaterials/79439764/prerequisites"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&fx["get"]))
            .mount(&s)
            .await;
        Mock::given(method("GET"))
            .and(path("/applicant/api/programMaterials/prerequisites/transcripts"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": {"transcripts": []}})))
            .mount(&s)
            .await;
        w.run(&["prereqs"], true, "name = \"Yale University - GEPN\"\nportal = \"test\"");
        w.run(&["plan"], true, "PENDING    Yale University - GEPN: no prerequisite rulings in prerequisites.toml");
        w.write("profile/prerequisites.toml", "[[program]]\nname = 'Yale University - GEPN'\nportal = 'test'\n[[program.requirement]]\nname = 'Microbiology w/Lab'\ncourses = [{ college = 'LACC', code = 'MICRO 020' }]\n");
        // No course entered on this server: the ruled one can't be found.
        w.run(&["plan"], true, "VIOLATION  Yale University - GEPN: Microbiology w/Lab: LACC MICRO 020 is not entered on the portal");

        http_500(&s, "GET", "/applicant/api/programMaterials/79439764/prerequisites").await;
        w.run(&["prereqs"], false, "HTTP 500");
        let out = w.command(&["prereqs"]).env_remove("TEST_COOKIE").output().expect("no session");
        check(&out, false, "session");
        std::fs::create_dir(w.dir.join("profile/personal.toml")).expect("not a file");
        w.run(&["prereqs"], false, "personal.toml");
        w.config("slate", &s.uri(), "");
        w.run(&["prereqs"], false, "rulings are written by hand");
    })
    .await
}

#[tokio::test]
async fn liaison_questions_list_pull_and_plan_from_questions_toml() {
    tokio::task::unconstrained(async move {
        let s = support::portal().await;
        let w = Workspace::new();
        w.config("liaison", &s.uri(), "");
        w.run(&["questions", "--open"], true, "no unanswered required program questions on test");
        w.run(&["questions"], true, "no program questions on test");

        let fx: serde_json::Value =
            serde_json::from_str(include_str!("../../liaison/src/sections/tests/fixtures/yale_questions.json"))
                .expect("fixture");
        Mock::given(method("GET"))
            .and(path("/applicant/api/getProgramNavigation"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([{"orgName": "Yale University",
                "programName": "GEPN", "programSelectedId": 79439764, "subSections": [{"name": "Questions"}]}])))
            .with_priority(1)
            .mount(&s)
            .await;
        Mock::given(method("GET"))
            .and(path("/applicant/api/programMaterials/79439764/questions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&fx["after"]))
            .mount(&s)
            .await;
        w.run(&["questions", "--open"], true, "Yale University - GEPN\n  3113132 [required textarea, max 3500 chars]");
        w.run(&["pull"], true, "wrote ./profile/questions.toml");
        let pulled = std::fs::read_to_string(w.dir.join("profile/questions.toml")).expect("questions.toml");
        assert!(pulled.contains("3113119 = \"No\"") || pulled.contains("\"3113119\" = \"No\""), "{pulled}");
        w.run(&["plan", "--only", "questions"], true, "1 required question(s) unanswered: 3113132");

        http_500(&s, "GET", "/applicant/api/programMaterials/79439764/questions").await;
        w.run(&["questions"], false, "HTTP 500");
        w.run(&["plan", "--only", "questions"], false, "HTTP 500");
        w.run(&["pull", "--force"], false, "HTTP 500");
        let out = w.command(&["questions"]).env_remove("TEST_COOKIE").output().expect("no session");
        check(&out, false, "session");
    })
    .await
}

#[tokio::test]
async fn liaison_documents_list_and_plan_from_documents_toml() {
    tokio::task::unconstrained(async move {
        let s = support::portal().await;
        let w = Workspace::new();
        w.config("liaison", &s.uri(), "");
        w.run(&["documents", "--open"], true, "no missing required program documents on test");
        w.run(&["documents"], true, "no program documents on test");

        let fx: serde_json::Value =
            serde_json::from_str(include_str!("../../liaison/src/sections/tests/fixtures/yale_documents.json"))
                .expect("fixture");
        Mock::given(method("GET"))
            .and(path("/applicant/api/getProgramNavigation"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([{"orgName": "Yale University",
                "programName": "GEPN", "programSelectedId": 79439764, "status": "INCOMPLETE",
                "subSections": [{"name": "Documents"}]}])))
            .with_priority(1)
            .mount(&s)
            .await;
        Mock::given(method("GET"))
            .and(path("/applicant/api/programMaterials/79439764/documents"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&fx["before"]))
            .mount(&s)
            .await;
        w.run(&["documents", "--open"], true, "# documents.toml: [[program]] name/portal");
        w.run(&["documents"], true, "Yale University - GEPN\n  9540461 [required] CV/Resume\n");
        w.run(&["plan", "--only", "documents"], true, "1 required document(s) missing: 9540461 (CV/Resume)");
        std::fs::write(w.dir.join("profile/resume.pdf"), b"%PDF-1.7\n".repeat(200)).expect("pdf");
        std::fs::write(
            w.dir.join("profile/documents.toml"),
            "[[program]]\nname = \"Yale University - GEPN\"\nportal = \"test\"\n[program.files]\n9540461 = \"resume.pdf\"\n",
        )
        .expect("documents.toml");
        w.run(&["plan", "--only", "documents"], true, "documents: 1 file(s) for Yale University - GEPN\n  + 9540461 CV/Resume: ");
        w.run(&["plan", "--only", "documents"], true, "  = mark Documents done");

        http_500(&s, "GET", "/applicant/api/programMaterials/79439764/documents").await;
        w.run(&["documents"], false, "HTTP 500");
        let out = w.command(&["documents"]).env_remove("TEST_COOKIE").output().expect("no session");
        check(&out, false, "session");
        w.config("slate", &s.uri(), "");
        w.run(&["documents"], false, "Slate uploads are not automated");
    })
    .await
}

#[tokio::test]
async fn references_are_not_requested_without_consent() {
    tokio::task::unconstrained(async move {
        let s = support::portal().await;
        let w = Workspace::new();
        w.config("liaison", &s.uri(), "");
        w.write(
            "profile/references.toml",
            "[[recommender]]\nfirst_name = 'Jane'\nlast_name = 'Doe'\nemail = 'jane@school.example'\ndue = '2099-01-01'\nwaive = true\nmessage = 'Thanks'\nprograms = ['University - Nursing']\n",
        );
        w.run(&["apply", "--only", "references", "--yes"], false, "consent = true is required");
        w.run(&["plan", "--only", "references"], true, "0 change(s) to apply");
    })
    .await
}

/// A Slate application whose "Program" page appears only once `choice` is
/// saved, as UCLA's program pages do after the program is chosen; the
/// program page keeps what is posted to it.
struct Revealing {
    chosen: std::sync::Arc<std::sync::atomic::AtomicBool>,
    saved: std::sync::Arc<std::sync::Mutex<String>>,
}

impl wiremock::Respond for Revealing {
    fn respond(&self, r: &wiremock::Request) -> ResponseTemplate {
        use std::sync::atomic::Ordering::SeqCst;
        let body = String::from_utf8_lossy(&r.body).to_string();
        let query = r.url.query().unwrap_or_default().to_string();
        if r.method.as_str() == "POST" {
            if query.starts_with("prog") {
                if let Some(v) = body.split("name=\"form_prog\"\r\n\r\n").nth(1).and_then(|v| v.split("\r\n").next()) {
                    *self.saved.lock().expect("lock") = v.to_string();
                }
            } else if body.contains("name=\"form_choice\"\r\n\r\n1") {
                self.chosen.store(true, SeqCst);
            }
            return ResponseTemplate::new(200).set_body_string("saved");
        }
        let chosen = self.chosen.load(SeqCst);
        let html = match r.url.path() {
            "/apply/review" => {
                let prog =
                    if chosen { "<a href='frm?prog' onclick='return apply_navigate(this);'>Program</a>" } else { "" };
                format!(
                    "<div id='menu'><a href='frm?basic' onclick='return apply_navigate(this);'>Basic</a>{prog}<a href='review' onclick='return apply_navigate(this);'>Review</a></div><h1>Review</h1>"
                )
            }
            _ if query.starts_with("prog") => format!(
                "<h1>Program</h1><div class='form_question' data-id='prog' data-type='text' data-export='prog'><label>Program</label><input name='form_prog' value='{}'></div>",
                self.saved.lock().expect("lock")
            ),
            _ => format!(
                "<h1>Basic</h1><div class='form_question' data-id='choice' data-type='select' data-export='choice'><label>Choice</label><select name='form_choice'><option value=''></option><option value='1'{}>Yes</option></select></div>",
                if chosen { " selected" } else { "" }
            ),
        };
        ResponseTemplate::new(200).set_body_string(html.replace('\'', "\""))
    }
}

async fn revealing_server() -> MockServer {
    let s = MockServer::start().await;
    let r = Revealing { chosen: Default::default(), saved: Default::default() };
    let shared = || Revealing { chosen: r.chosen.clone(), saved: r.saved.clone() };
    for p in ["/apply/review", "/apply/frm"] {
        Mock::given(path(p)).respond_with(shared()).mount(&s).await;
    }
    Mock::given(method("GET"))
        .and(path("/apply/"))
        .respond_with(
            ResponseTemplate::new(200).set_body_string(include_str!("../../slate/tests/fixtures/applications.html")),
        )
        .mount(&s)
        .await;
    Mock::given(method("POST"))
        .and(path("/apply/"))
        .respond_with(ResponseTemplate::new(200).insert_header("set-cookie", ".application=app; path=/"))
        .mount(&s)
        .await;
    s
}

#[tokio::test]
async fn apply_reveal_saves_until_the_pages_a_choice_adds_take_their_answers() {
    tokio::task::unconstrained(async move {
        let s = revealing_server().await;
        let w = Workspace::new();
        w.config("slate", &s.uri(), "");
        let config = std::fs::read_to_string(w.dir.join("admit.toml")).expect("config");
        w.write("admit.toml", &format!("{config}\napplication='app'\n"));
        std::fs::create_dir_all(w.dir.join("profile/slate")).expect("slate dir");
        w.write("profile/slate/test.toml", "[answers]\nchoice='Yes'\nprog='Hello'\n");
        w.run(&["plan"], true, "answer \"prog\": no writable question with that key");
        w.run(&["apply", "--yes"], false, "fix the answers");
        w.run(&["apply", "--reveal"], false, "re-run with --yes");
        w.run(&["apply", "--reveal", "--yes"], true, "revealed 1 question(s); applying again:\nProgram");
        w.run(&["plan"], true, "0 change(s)");
        w.run(&["apply", "--reveal", "--yes"], true, "nothing to apply");

        // An answer no save reveals: what fits is written, then the run fails.
        w.write("profile/slate/test.toml", "[answers]\nchoice='Yes'\nprog='Again'\nnothere='x'\n");
        w.run(&["apply", "--reveal", "--yes"], false, "1 answer(s) still have no question: nothere");
        w.run(&["apply", "--reveal", "--yes"], false, "nothing left to save could reveal one");
        // An invalid choice still stops it before anything is written.
        w.write("profile/slate/test.toml", "[answers]\nchoice='Maybe'\nnothere='x'\n");
        w.run(&["apply", "--reveal", "--yes"], false, "fix the answers");
        w.config("liaison", &s.uri(), "");
        w.run(&["apply", "--reveal", "--yes"], false, "--reveal is for Slate portals");
    })
    .await
}

#[tokio::test]
async fn a_same_as_application_shares_the_answers_file_and_sign_in() {
    tokio::task::unconstrained(async move {
        let s = revealing_server().await;
        let w = Workspace::new();
        w.config("slate", &s.uri(), "");
        let config = std::fs::read_to_string(w.dir.join("admit.toml")).expect("config");
        w.write(
            "admit.toml",
            &format!("{config}\napplication='app'\n[[portal]]\nname='mirror'\nkind='slate'\nbase_url='{}'\napplication='app'\nsame_as='test'\n", s.uri()),
        );
        std::fs::create_dir_all(w.dir.join("profile/slate")).expect("slate dir");
        w.write("profile/slate/test.toml", "[answers]\nchoice='Yes'\n");
        // The mirror's session is test's (the Keychain item "test").
        w.run_env(&["-p", "mirror", "plan"], &[("TEST_STORED_COOKIE", "_uid=x")], &[], true, "~ choice: (blank) -> \"Yes\"");
        w.run_env(&["-p", "mirror", "pull"], &[("TEST_STORED_COOKIE", "_uid=x")], &[], false, "follows test's answers file (same_as); pull from test");
        w.run_env(&["-p", "mirror", "plan"], &[], &["TEST_STORED_COOKIE"], false, "no session for \"test\"");
    })
    .await
}

#[tokio::test]
async fn an_apply_that_fails_midway_names_the_journal_of_what_it_sent() {
    tokio::task::unconstrained(async move {
        let s = support::portal().await;
        Mock::given(method("GET"))
            .and(path("/applicant/api/supportingInfo/achievements"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"achievements":[{"id":1,"achievement":"Awards","name":"Award","stateId":339,"countryId":222,"description":"Old"}]})))
            .with_priority(1)
            .mount(&s)
            .await;
        Mock::given(method("PUT"))
            .and(path("/applicant/api/supportingInfo/achievements/1"))
            .respond_with(ResponseTemplate::new(500))
            .with_priority(1)
            .mount(&s)
            .await;
        let w = Workspace::new();
        w.config("liaison", &s.uri(), "");
        w.record();
        w.write("profile/achievements.toml", "[[achievement]]\nkind='award'\nname='Award'\nstate='California'\ndescription='New'\n");
        w.run(&["apply", "--only", "achievements", "--yes"], false, "apply stopped partway; each write it sent is in");
        let log = std::fs::read_to_string(w.dir.join("reports/journal-test.log")).expect("journal");
        let steps: Vec<&str> = log.lines().map(|l| l.split('\t').nth(1).expect("step")).collect();
        assert_eq!(steps, ["started", "failed"], "{log}");
        assert!(log.contains("PUT /supportingInfo/achievements/1\tPUT /supportingInfo/achievements/1: HTTP 500"), "{log}");
    })
    .await;
}

#[tokio::test]
async fn a_reshaped_response_or_no_recording_stops_apply_before_any_write() {
    tokio::task::unconstrained(async move {
        let s = support::portal().await;
        let w = Workspace::new();
        w.config("liaison", &s.uri(), "");
        w.write("profile/achievements.toml", "[[achievement]]\nkind='award'\nname='Award'\nstate='California'\ndescription='New'\n");
        let original = json!({"achievements":[{"id":1,"achievement":"Awards","name":"Award","stateId":339,"countryId":222,"description":"Old"}]});
        Mock::given(method("GET"))
            .and(path("/applicant/api/supportingInfo/achievements"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&original))
            .with_priority(2)
            .mount(&s)
            .await;
        Mock::given(method("PUT")).respond_with(ResponseTemplate::new(200)).expect(0).with_priority(1).mount(&s).await;
        // Nothing to compare with: no write.
        w.run(&["apply", "--yes"], false, "writes are checked against the recorded response shapes first");
        w.record();
        // The portal renames `name`: a body built from it would carry "".
        let mut renamed = original.clone();
        let a = renamed["achievements"][0].as_object_mut().expect("object");
        let v = a.remove("name").expect("name");
        a.insert("title".into(), v);
        Mock::given(method("GET"))
            .and(path("/applicant/api/supportingInfo/achievements"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&renamed))
            .with_priority(1)
            .mount(&s)
            .await;
        w.run(&["apply", "--yes"], false, "response shape(s) the writes depend on changed");
        let out = w.command(&["apply", "--yes"]).output().expect("apply");
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(text.contains("/supportingInfo/achievements: $.achievements[].name (string) is gone"), "{text}");
        assert!(!w.dir.join("reports/journal-test.log").exists(), "nothing was sent");
        // The check's own read failing stops the run too (the plan read it
        // first; the recording is made before the failure is mounted).
        let s = support::portal().await;
        let w = Workspace::new();
        w.config("liaison", &s.uri(), "");
        w.write("profile/achievements.toml", "[[achievement]]\nkind='award'\nname='Award'\nstate='California'\ndescription='New'\n");
        Mock::given(method("GET"))
            .and(path("/applicant/api/supportingInfo/achievements"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&original))
            .up_to_n_times(2)
            .with_priority(1)
            .mount(&s)
            .await;
        Mock::given(method("GET"))
            .and(path("/applicant/api/supportingInfo/achievements"))
            .respond_with(ResponseTemplate::new(500).set_body_string("down"))
            .with_priority(2)
            .mount(&s)
            .await;
        w.record();
        w.run(&["apply", "--only", "achievements", "--yes"], false, "GET /supportingInfo/achievements: HTTP 500");
        assert!(!w.dir.join("reports/journal-test.log").exists(), "nothing was sent");
    })
    .await;
}

/// `admit transcripts` and `admit parchment login` reach their commands;
/// neither touches a service here (nothing declared, nothing on stdin).
#[test]
fn transcript_orders_commands_are_dispatched() {
    let w = Workspace::new();
    w.config("liaison", "http://127.0.0.1:9", "");
    w.run(&["transcripts"], true, "declares no transcripts to send");
    w.stdin(&["parchment", "login"], "", false, "expected two lines");
}
