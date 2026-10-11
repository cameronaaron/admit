use std::io::Write;
use std::path::{Path, PathBuf};

use super::*;

fn scratch(kind: &str, portals: bool) -> (PathBuf, crate::ctx::Ctx) {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "admit-slate-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    std::fs::create_dir_all(dir.join("profile")).expect("profile");
    let ctx = crate::ctx::Ctx {
        profile_dir: dir.join("profile"),
        base: dir.clone(),
        portal_name: None,
        config: crate::ctx::Config {
            profile: "profile".into(),
            portals: if portals {
                vec![crate::ctx::PortalConfig {
                    name: "test".into(),
                    kind: kind.into(),
                    base_url: "http://127.0.0.1:9".into(),
                    har_dir: None,
                    cookie_env: None,
                    application: Some("app".into()),
                    submitted: false,
                    deadline: None,
                    same_as: None,
                }]
            } else {
                vec![]
            },
            sync: None,
            website: None,
        },
    };
    (dir, ctx)
}

#[test]
fn an_answers_path_without_a_parent_is_rejected_and_a_real_one_is_created() {
    let err = ensure_parent(Path::new("/")).expect_err("root");
    assert!(err.to_string().contains("answers path has a parent"), "{err:#}");
    let dir = std::env::temp_dir().join(format!("admit-slate-parent-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("dir");
    let blocked = dir.join("not-a-directory");
    std::fs::write(&blocked, "x").expect("file");
    let err = ensure_parent(&blocked.join("answers.toml")).expect_err("parent is a file");
    assert!(err.to_string().contains("creating"), "{err:#}");
    let path = dir.join("slate/test.toml");
    ensure_parent(&path).expect("created");
    assert!(path.parent().expect("parent").is_dir());
    std::fs::remove_dir_all(dir).expect("cleanup");
}

#[tokio::test]
async fn slate_commands_fail_before_any_network_call_when_the_config_cannot_work() {
    tokio::task::unconstrained(async move {
        let (dir, ctx) = scratch("slate", false);
        assert!(answers(&ctx).expect_err("answers").to_string().contains("no [[portal]]"));
        assert!(editable(&ctx).map(drop).expect_err("editable").to_string().contains("no [[portal]]"));
        assert!(session(&ctx, &SessionCmd::Check).await.expect_err("session").to_string().contains("no [[portal]]"));
        assert!(
            findings(&ctx, &admit_domain::Profile::default())
                .await
                .expect_err("findings")
                .to_string()
                .contains("no [[portal]]")
        );
        std::fs::remove_dir_all(&dir).expect("cleanup");

        let (dir, ctx) = scratch("slate", true);
        std::fs::write(dir.join("profile/achievements.toml"), "not = [").expect("toml");
        assert!(audit(&ctx).await.expect_err("audit").to_string().contains("parsing"));
        std::fs::remove_dir_all(&dir).expect("cleanup");

        // Liaison is rejected inside slate() before a cookie or the Keychain is read.
        let (dir, ctx) = scratch("liaison", true);
        for err in [
            plan(&ctx).await.expect_err("plan").to_string(),
            apply(&ctx, false, false).await.expect_err("apply").to_string(),
            pull(&ctx, false).await.expect_err("pull").to_string(),
            questions(&ctx, false).await.expect_err("questions").to_string(),
            status(&ctx).await.expect_err("status").to_string(),
            session(&ctx, &SessionCmd::Check).await.expect_err("session").to_string(),
            findings(&ctx, &admit_domain::Profile::default()).await.expect_err("findings").to_string(),
            audit(&ctx).await.expect_err("audit").to_string(),
        ] {
            assert!(err.contains("not slate"), "{err}");
        }
        std::fs::remove_dir_all(&dir).expect("cleanup");
    })
    .await
}

fn slate_at(url: &str, submitted: bool) -> (PathBuf, crate::ctx::Ctx) {
    let (dir, mut ctx) = scratch("slate", true);
    ctx.config.portals[0].base_url = url.to_string();
    ctx.config.portals[0].cookie_env = Some("TEST_COOKIE".into());
    ctx.config.portals[0].submitted = submitted;
    (dir, ctx)
}

fn answers_file(dir: &Path, body: &str) {
    std::fs::create_dir_all(dir.join("profile/slate")).expect("answers dir");
    std::fs::write(dir.join("profile/slate/test.toml"), body).expect("answers");
}

async fn mount_open(s: &wiremock::MockServer) {
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, ResponseTemplate};
    Mock::given(method("POST"))
        .and(path("/apply/"))
        .respond_with(ResponseTemplate::new(200).insert_header("set-cookie", ".application=app; path=/"))
        .mount(s)
        .await;
}

fn menu(links: &str) -> String {
    format!(
        "<div id=\"menu\">{links}<a href=\"review\" onclick=\"return apply_navigate(this);\">Review</a></div></ul><h1>Review</h1>"
    )
}

fn basic_page() -> &'static str {
    "<h1>Basic</h1><div class=\"form_question\" data-id=\"name\" data-type=\"text\" data-export=\"sys:first\" data-required=\"1\"><label>Name</label><input name=\"form_name\" value=\"Pat\"></div>"
}

fn info_page() -> &'static str {
    "<h1>Additional Academic Info</h1>"
}

async fn mount_listed(s: &wiremock::MockServer, list: &str) {
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, ResponseTemplate};
    Mock::given(method("GET"))
        .and(path("/apply/aca"))
        .and(wiremock::matchers::query_param_is_missing("id"))
        .respond_with(ResponseTemplate::new(200).set_body_string(list.to_string()))
        .mount(s)
        .await;
    Mock::given(method("GET"))
        .and(path("/apply/aca"))
        .and(query_param("id", ""))
        .respond_with(
            ResponseTemplate::new(200).set_body_string(include_str!("../../../../slate/tests/fixtures/aca_add.html")),
        )
        .with_priority(1)
        .mount(s)
        .await;
}

const SCHOOL: &str = "[[institutions]]\nname='Connecticut College'\nkey='003284'\ncity='New London'\nregion='CT'\ncountry='US'\nfrom='2017-08'\nto='2021-05'\nlevel='U'\ndegree='Bachelor of Arts'\nconferred='2021-05'\ntranscript='t.pdf'\n";
const JOB: &str = "[[jobs]]\nemployer='Clinic'\ntitle='Care'\nfrom='2020-01'\nhours=10\ndescription='Care'\n";

fn must_fail(label: &str, err: anyhow::Error, needle: &str) {
    let msg = format!("{err:#}");
    assert!(msg.contains(needle), "{label}: {msg}");
}

/// Every `?` in plan/apply/pull/findings/status/check fails on its own call.
/// The cookie comes from the environment; `security` is a stub.
#[test]
fn each_portal_call_fails_on_its_own() {
    if std::env::var_os("ADMIT_SLATE_CHILD").is_some() {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");
        rt.block_on(drive());
        let dir = std::env::var("ADMIT_SLATE_DIR").expect("dir");
        std::fs::write(format!("{dir}/done"), "ok").expect("done");
        return;
    }
    let dir = std::env::temp_dir().join(format!("admit-slate-child-{}", std::process::id()));
    let bin = dir.join("bin");
    std::fs::create_dir_all(&bin).expect("bin");
    let security = bin.join("security");
    std::fs::write(&security, "#!/bin/sh\nexit 1\n").expect("script");
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&security, std::fs::Permissions::from_mode(0o755)).expect("mode");
    let current = std::thread::current();
    let name = current.name().expect("test name");
    let out = std::process::Command::new(std::env::current_exe().expect("exe"))
        .args(["--exact", name, "--test-threads=1"])
        .env("ADMIT_SLATE_CHILD", "1")
        .env("ADMIT_SLATE_DIR", &dir)
        .env("TEST_COOKIE", "_uid=test")
        .env("PATH", &bin)
        .env_remove("TEST_USER")
        .env_remove("TEST_PASSWORD")
        .stdin(std::process::Stdio::null())
        .output()
        .expect("child");
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success() && dir.join("done").is_file(), "{text}");
    std::fs::remove_dir_all(dir).expect("cleanup");
}

async fn drive() {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    async fn review(s: &MockServer, body: &str) {
        Mock::given(method("GET"))
            .and(path("/apply/review"))
            .respond_with(ResponseTemplate::new(200).set_body_string(body.to_string()))
            .mount(s)
            .await;
    }
    async fn basic(s: &MockServer) {
        Mock::given(method("GET"))
            .and(path("/apply/frm"))
            .and(query_param("page", "basic"))
            .respond_with(ResponseTemplate::new(200).set_body_string(basic_page()))
            .with_priority(1)
            .mount(s)
            .await;
    }

    // apply's answers() ?, and pull/plan/questions pages() ?.
    let s = MockServer::start().await;
    mount_open(&s).await;
    review(&s, &menu("<a href=\"frm?page=basic\" onclick=\"return apply_navigate(this);\">Basic</a>")).await;
    basic(&s).await;
    let (dir, ctx) = slate_at(&s.uri(), false);
    must_fail("answers", apply(&ctx, true, false).await.expect_err("answers"), "reading");
    std::fs::remove_dir_all(dir).expect("cleanup");

    let s = MockServer::start().await;
    mount_open(&s).await;
    Mock::given(method("GET"))
        .and(path("/apply/review"))
        .respond_with(ResponseTemplate::new(500).set_body_string("no"))
        .mount(&s)
        .await;
    let (dir, ctx) = slate_at(&s.uri(), false);
    answers_file(&dir, "[answers]\n'sys:first'='Pat'\n");
    must_fail("plan pages", plan(&ctx).await.expect_err("plan"), "HTTP 500");
    must_fail("apply pages", apply(&ctx, true, false).await.expect_err("apply"), "HTTP 500");
    must_fail("pull pages", pull(&ctx, true).await.expect_err("pull"), "HTTP 500");
    must_fail("questions pages", questions(&ctx, false).await.expect_err("questions"), "HTTP 500");
    must_fail(
        "findings audit",
        findings(&ctx, &ctx.profile().expect("profile")).await.expect_err("findings"),
        "HTTP 500",
    );
    std::fs::remove_dir_all(dir).expect("cleanup");

    // plan_institutions ? on apply and findings, plan_jobs ? likewise.
    let s = MockServer::start().await;
    mount_open(&s).await;
    review(&s, &menu("<a href=\"frm?page=basic\" onclick=\"return apply_navigate(this);\">Basic</a>")).await;
    basic(&s).await;
    Mock::given(method("GET"))
        .and(path("/apply/aca"))
        .respond_with(ResponseTemplate::new(500).set_body_string("no"))
        .mount(&s)
        .await;
    let (dir, ctx) = slate_at(&s.uri(), false);
    answers_file(&dir, SCHOOL);
    must_fail("apply institutions", apply(&ctx, true, false).await.expect_err("institutions"), "HTTP 500");
    must_fail(
        "findings institutions",
        findings(&ctx, &ctx.profile().expect("profile")).await.expect_err("findings"),
        "HTTP 500",
    );
    std::fs::remove_dir_all(dir).expect("cleanup");

    let s = MockServer::start().await;
    mount_open(&s).await;
    review(&s, &menu("<a href=\"frm?page=basic\" onclick=\"return apply_navigate(this);\">Basic</a>")).await;
    basic(&s).await;
    Mock::given(method("GET"))
        .and(path("/apply/job"))
        .respond_with(ResponseTemplate::new(500).set_body_string("no"))
        .mount(&s)
        .await;
    let (dir, ctx) = slate_at(&s.uri(), false);
    answers_file(&dir, JOB);
    must_fail("apply jobs", apply(&ctx, true, false).await.expect_err("jobs"), "HTTP 500");
    must_fail(
        "findings jobs",
        findings(&ctx, &ctx.profile().expect("profile")).await.expect_err("findings"),
        "HTTP 500",
    );
    std::fs::remove_dir_all(dir).expect("cleanup");

    // plan_school_details ?: the page loads, the refetch fails.
    async fn school_page(s: &MockServer, fail_at: usize) {
        let n = Arc::new(AtomicUsize::new(0));
        Mock::given(method("GET"))
            .and(path("/apply/frm"))
            .and(query_param("page", "info"))
            .respond_with(move |_: &wiremock::Request| {
                if n.fetch_add(1, Ordering::SeqCst) >= fail_at {
                    ResponseTemplate::new(500).set_body_string("no")
                } else {
                    ResponseTemplate::new(200).set_body_string(info_page())
                }
            })
            .with_priority(1)
            .mount(s)
            .await;
    }
    let links = "<a href=\"frm?page=basic\" onclick=\"return apply_navigate(this);\">Basic</a><a href=\"frm?page=info\" onclick=\"return apply_navigate(this);\">Info</a>";
    let s = MockServer::start().await;
    mount_open(&s).await;
    review(&s, &menu(links)).await;
    basic(&s).await;
    school_page(&s, 1).await;
    mount_listed(&s, include_str!("../../../../slate/tests/fixtures/aca.html")).await;
    let (dir, ctx) = slate_at(&s.uri(), false);
    answers_file(&dir, SCHOOL);
    must_fail("plan school", plan(&ctx).await.expect_err("school"), "HTTP 500");
    std::fs::remove_dir_all(dir).expect("cleanup");
    let s = MockServer::start().await;
    mount_open(&s).await;
    review(&s, &menu(links)).await;
    basic(&s).await;
    school_page(&s, 1).await;
    mount_listed(&s, include_str!("../../../../slate/tests/fixtures/aca.html")).await;
    let (dir, ctx) = slate_at(&s.uri(), false);
    answers_file(&dir, SCHOOL);
    must_fail("apply school", apply(&ctx, true, false).await.expect_err("school"), "HTTP 500");
    std::fs::remove_dir_all(dir).expect("cleanup");
    let s = MockServer::start().await;
    mount_open(&s).await;
    review(&s, &menu(links)).await;
    basic(&s).await;
    // findings reads Info in coverage() and pages() first: the third read is
    // plan_school_details'. The count proves that is the read that failed.
    school_page(&s, 2).await;
    mount_listed(&s, include_str!("../../../../slate/tests/fixtures/aca.html")).await;
    let (dir, ctx) = slate_at(&s.uri(), false);
    answers_file(&dir, SCHOOL);
    must_fail(
        "findings school",
        findings(&ctx, &ctx.profile().expect("profile")).await.expect_err("school"),
        "HTTP 500",
    );
    let info_reads =
        s.received_requests().await.expect("recording").iter().filter(|r| r.url.query() == Some("page=info")).count();
    assert_eq!(info_reads, 3);
    std::fs::remove_dir_all(dir).expect("cleanup");

    // A changed answer whose save, institution add, or job add is rejected.
    let s = MockServer::start().await;
    mount_open(&s).await;
    review(&s, &menu("<a href=\"frm?page=basic\" onclick=\"return apply_navigate(this);\">Basic</a>")).await;
    basic(&s).await;
    Mock::given(method("POST"))
        .and(path("/apply/frm"))
        .respond_with(ResponseTemplate::new(500).set_body_string("no"))
        .with_priority(1)
        .mount(&s)
        .await;
    let (dir, ctx) = slate_at(&s.uri(), false);
    answers_file(&dir, "[answers]\n'sys:first'='Changed'\n");
    must_fail("apply post", apply(&ctx, true, false).await.expect_err("post"), "HTTP 500");
    std::fs::remove_dir_all(dir).expect("cleanup");

    let s = MockServer::start().await;
    mount_open(&s).await;
    review(&s, &menu("<a href=\"frm?page=basic\" onclick=\"return apply_navigate(this);\">Basic</a>")).await;
    basic(&s).await;
    Mock::given(method("GET"))
        .and(path("/apply/aca"))
        .respond_with(
            ResponseTemplate::new(200).set_body_string(include_str!("../../../../slate/tests/fixtures/aca.html")),
        )
        .mount(&s)
        .await;
    Mock::given(method("GET"))
        .and(path("/apply/aca"))
        .and(query_param("id", ""))
        .respond_with(
            ResponseTemplate::new(200).set_body_string(include_str!("../../../../slate/tests/fixtures/aca_add.html")),
        )
        .with_priority(1)
        .mount(&s)
        .await;
    Mock::given(method("POST"))
        .and(path("/apply/aca"))
        .respond_with(ResponseTemplate::new(500).set_body_string("no"))
        .with_priority(1)
        .mount(&s)
        .await;
    let (dir, ctx) = slate_at(&s.uri(), false);
    answers_file(&dir, SCHOOL);
    std::fs::write(dir.join("profile/t.pdf"), "%PDF-test").expect("pdf");
    must_fail("add institution", apply(&ctx, true, false).await.expect_err("add"), "HTTP 500");
    std::fs::remove_dir_all(dir).expect("cleanup");

    let s = MockServer::start().await;
    mount_open(&s).await;
    review(&s, &menu("<a href=\"frm?page=basic\" onclick=\"return apply_navigate(this);\">Basic</a>")).await;
    basic(&s).await;
    Mock::given(method("GET"))
        .and(path("/apply/job"))
        .respond_with(
            ResponseTemplate::new(200).set_body_string(include_str!("../../../../slate/tests/fixtures/job.html")),
        )
        .mount(&s)
        .await;
    Mock::given(method("POST"))
        .and(path("/apply/job"))
        .respond_with(ResponseTemplate::new(500).set_body_string("no"))
        .with_priority(1)
        .mount(&s)
        .await;
    let (dir, ctx) = slate_at(&s.uri(), false);
    answers_file(&dir, JOB);
    must_fail("add job", apply(&ctx, true, false).await.expect_err("job"), "HTTP 500");
    std::fs::remove_dir_all(dir).expect("cleanup");

    // Re-plan: the first pass saves, the second pages/institutions/jobs/school read fails.
    async fn nth_review(s: &MockServer, body: String, fail_at: usize) {
        let n = Arc::new(AtomicUsize::new(0));
        Mock::given(method("GET"))
            .and(path("/apply/review"))
            .respond_with(move |_: &wiremock::Request| {
                if n.fetch_add(1, Ordering::SeqCst) >= fail_at {
                    ResponseTemplate::new(500).set_body_string("no")
                } else {
                    ResponseTemplate::new(200).set_body_string(body.clone())
                }
            })
            .mount(s)
            .await;
    }
    let page = menu("<a href=\"frm?page=basic\" onclick=\"return apply_navigate(this);\">Basic</a>");
    let s = MockServer::start().await;
    mount_open(&s).await;
    nth_review(&s, page.clone(), 1).await;
    basic(&s).await;
    Mock::given(method("POST"))
        .and(path("/apply/frm"))
        .respond_with(ResponseTemplate::new(200).set_body_string("saved"))
        .mount(&s)
        .await;
    let (dir, ctx) = slate_at(&s.uri(), false);
    answers_file(&dir, "[answers]\n'sys:first'='Changed'\n");
    must_fail("replan pages", apply(&ctx, true, false).await.expect_err("replan"), "HTTP 500");
    std::fs::remove_dir_all(dir).expect("cleanup");

    let listed = "<h1>Academic History</h1><div id=\"part_table\"><table><tbody><tr><td>Connecticut College</td></tr></tbody></table></div>";
    let s = MockServer::start().await;
    mount_open(&s).await;
    review(&s, &menu("<a href=\"frm?page=basic\" onclick=\"return apply_navigate(this);\">Basic</a>")).await;
    basic(&s).await;
    let n = Arc::new(AtomicUsize::new(0));
    Mock::given(method("GET"))
        .and(path("/apply/aca"))
        .and(wiremock::matchers::query_param_is_missing("id"))
        .respond_with(move |_: &wiremock::Request| {
            if n.fetch_add(1, Ordering::SeqCst) >= 1 {
                ResponseTemplate::new(500).set_body_string("no")
            } else {
                ResponseTemplate::new(200).set_body_string(listed)
            }
        })
        .mount(&s)
        .await;
    Mock::given(method("GET"))
        .and(path("/apply/aca"))
        .and(query_param("id", ""))
        .respond_with(
            ResponseTemplate::new(200).set_body_string(include_str!("../../../../slate/tests/fixtures/aca_add.html")),
        )
        .with_priority(1)
        .mount(&s)
        .await;
    Mock::given(method("POST"))
        .and(path("/apply/frm"))
        .respond_with(ResponseTemplate::new(200).set_body_string("saved"))
        .mount(&s)
        .await;
    let (dir, ctx) = slate_at(&s.uri(), false);
    answers_file(&dir, &format!("[answers]\n'sys:first'='Changed'\n{SCHOOL}"));
    must_fail("replan institutions", apply(&ctx, true, false).await.expect_err("replan"), "HTTP 500");
    std::fs::remove_dir_all(dir).expect("cleanup");

    let employed = "<h1>Professional Experience</h1><div id=\"part_table\"><table><tbody><tr><td>Clinic</td><td>01/2020–Present</td></tr></tbody></table></div>";
    let s = MockServer::start().await;
    mount_open(&s).await;
    review(&s, &menu("<a href=\"frm?page=basic\" onclick=\"return apply_navigate(this);\">Basic</a>")).await;
    basic(&s).await;
    let n = Arc::new(AtomicUsize::new(0));
    Mock::given(method("GET"))
        .and(path("/apply/job"))
        .respond_with(move |_: &wiremock::Request| {
            if n.fetch_add(1, Ordering::SeqCst) >= 1 {
                ResponseTemplate::new(500).set_body_string("no")
            } else {
                ResponseTemplate::new(200).set_body_string(employed)
            }
        })
        .mount(&s)
        .await;
    Mock::given(method("POST"))
        .and(path("/apply/frm"))
        .respond_with(ResponseTemplate::new(200).set_body_string("saved"))
        .mount(&s)
        .await;
    let (dir, ctx) = slate_at(&s.uri(), false);
    answers_file(&dir, &format!("[answers]\n'sys:first'='Changed'\n{JOB}"));
    must_fail("replan jobs", apply(&ctx, true, false).await.expect_err("replan"), "HTTP 500");
    std::fs::remove_dir_all(dir).expect("cleanup");

    let s = MockServer::start().await;
    mount_open(&s).await;
    review(&s, &menu(links)).await;
    basic(&s).await;
    school_page(&s, 3).await;
    Mock::given(method("GET"))
        .and(path("/apply/aca"))
        .and(wiremock::matchers::query_param_is_missing("id"))
        .respond_with(ResponseTemplate::new(200).set_body_string(listed))
        .mount(&s)
        .await;
    Mock::given(method("GET"))
        .and(path("/apply/aca"))
        .and(query_param("id", ""))
        .respond_with(
            ResponseTemplate::new(200).set_body_string(include_str!("../../../../slate/tests/fixtures/aca_add.html")),
        )
        .with_priority(1)
        .mount(&s)
        .await;
    Mock::given(method("POST"))
        .and(path("/apply/frm"))
        .respond_with(ResponseTemplate::new(200).set_body_string("saved"))
        .mount(&s)
        .await;
    let (dir, ctx) = slate_at(&s.uri(), false);
    answers_file(&dir, &format!("[answers]\n'sys:first'='Changed'\n{SCHOOL}"));
    must_fail("replan school", apply(&ctx, true, false).await.expect_err("replan"), "HTTP 500");
    std::fs::remove_dir_all(dir).expect("cleanup");

    // still_open's coverage ? and findings' coverage/pages/answers ?.
    let s = MockServer::start().await;
    mount_open(&s).await;
    nth_review(&s, page.clone(), 1).await;
    basic(&s).await;
    let (dir, ctx) = slate_at(&s.uri(), false);
    answers_file(&dir, "[answers]\n'sys:first'='Pat'\n");
    must_fail("coverage", apply(&ctx, true, false).await.expect_err("coverage"), "HTTP 500");
    std::fs::remove_dir_all(dir).expect("cleanup");

    let s = MockServer::start().await;
    mount_open(&s).await;
    review(&s, &page).await;
    basic(&s).await;
    let (dir, ctx) = slate_at(&s.uri(), false);
    must_fail(
        "findings answers",
        findings(&ctx, &ctx.profile().expect("profile")).await.expect_err("answers"),
        "reading",
    );
    std::fs::remove_dir_all(dir).expect("cleanup");

    let s = MockServer::start().await;
    mount_open(&s).await;
    nth_review(&s, page.clone(), 1).await;
    basic(&s).await;
    let (dir, ctx) = slate_at(&s.uri(), false);
    answers_file(&dir, "[answers]\n'sys:first'='Pat'\n");
    must_fail(
        "findings coverage",
        findings(&ctx, &ctx.profile().expect("profile")).await.expect_err("coverage"),
        "HTTP 500",
    );
    std::fs::remove_dir_all(dir).expect("cleanup");

    let s = MockServer::start().await;
    mount_open(&s).await;
    nth_review(&s, page, 2).await;
    basic(&s).await;
    let (dir, ctx) = slate_at(&s.uri(), false);
    answers_file(&dir, "[answers]\n'sys:first'='Pat'\n");
    must_fail("findings pages", findings(&ctx, &ctx.profile().expect("profile")).await.expect_err("pages"), "HTTP 500");
    std::fs::remove_dir_all(dir).expect("cleanup");

    // status() ?, a submitted check, and findings: opening the submitted
    // application reads status once, and the command's own read fails.
    async fn status_then_fail(s: &MockServer) {
        let n = Arc::new(AtomicUsize::new(0));
        let body = include_str!("../../../../slate/tests/fixtures/apu_status.html").to_string();
        Mock::given(method("GET"))
            .and(path("/apply/status"))
            .respond_with(move |_: &wiremock::Request| {
                if n.fetch_add(1, Ordering::SeqCst) == 0 {
                    ResponseTemplate::new(200).set_body_string(body.clone())
                } else {
                    ResponseTemplate::new(500).set_body_string("no")
                }
            })
            .mount(s)
            .await;
    }
    let s = MockServer::start().await;
    status_then_fail(&s).await;
    let (dir, ctx) = slate_at(&s.uri(), true);
    must_fail("status", status(&ctx).await.expect_err("status"), "HTTP 500");
    std::fs::remove_dir_all(dir).expect("cleanup");
    let s = MockServer::start().await;
    status_then_fail(&s).await;
    let (dir, ctx) = slate_at(&s.uri(), true);
    must_fail("check status", session(&ctx, &SessionCmd::Check).await.expect_err("check"), "HTTP 500");
    std::fs::remove_dir_all(dir).expect("cleanup");
    let s = MockServer::start().await;
    status_then_fail(&s).await;
    let (dir, ctx) = slate_at(&s.uri(), true);
    must_fail(
        "findings status",
        findings(&ctx, &ctx.profile().expect("profile")).await.expect_err("findings"),
        "HTTP 500",
    );
    std::fs::remove_dir_all(dir).expect("cleanup");

    // check()'s application list ?.
    let s = MockServer::start().await;
    mount_open(&s).await;
    Mock::given(method("GET"))
        .and(path("/apply/"))
        .respond_with(ResponseTemplate::new(500).set_body_string("no"))
        .mount(&s)
        .await;
    let (dir, ctx) = slate_at(&s.uri(), false);
    must_fail("applications", session(&ctx, &SessionCmd::Check).await.expect_err("apps"), "HTTP 500");
    std::fs::remove_dir_all(dir).expect("cleanup");
}

/// Sign-in succeeds and the cookie write fails, so login's `put` is the error.
#[test]
fn login_fails_when_the_new_cookie_cannot_be_stored() {
    if std::env::var_os("ADMIT_SLATE_LOGIN").is_some() {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");
        rt.block_on(async {
            let s = wiremock::MockServer::start().await;
            wiremock::Mock::given(wiremock::matchers::method("GET"))
                .and(wiremock::matchers::path("/account/login"))
                .respond_with(wiremock::ResponseTemplate::new(200))
                .mount(&s)
                .await;
            wiremock::Mock::given(wiremock::matchers::method("POST"))
                .and(wiremock::matchers::path("/account/login"))
                .respond_with(
                    wiremock::ResponseTemplate::new(302)
                        .insert_header("location", "/apply/")
                        .insert_header("set-cookie", "_uid=abc; path=/"),
                )
                .mount(&s)
                .await;
            let (dir, ctx) = slate_at(&s.uri(), false);
            let err = session(&ctx, &SessionCmd::Login).await.expect_err("put");
            assert!(
                format!("{err:#}").contains("add-generic-password") || format!("{err:#}").contains("security"),
                "{err:#}"
            );
            std::fs::remove_dir_all(dir).expect("cleanup");
        });
        let dir = std::env::var("ADMIT_SLATE_DIR").expect("dir");
        std::fs::write(format!("{dir}/done"), "ok").expect("done");
        return;
    }
    let dir = std::env::temp_dir().join(format!("admit-slate-login-{}", std::process::id()));
    let bin = dir.join("bin");
    std::fs::create_dir_all(&bin).expect("bin");
    let security = bin.join("security");
    std::fs::write(
        &security,
        "#!/bin/sh\nif [ \"$1\" = find-generic-password ]; then\n  case \"$5\" in *-user) echo user;; *-password) echo password;; *) exit 1;; esac\n  exit 0\nfi\nn=0\nif [ -f \"$ADMIT_SLATE_DIR/n\" ]; then n=$(cat \"$ADMIT_SLATE_DIR/n\"); fi\necho $((n+1)) > \"$ADMIT_SLATE_DIR/n\"\nif [ \"$n\" -ge 2 ]; then exit 1; fi\nexit 0\n",
    )
    .expect("script");
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&security, std::fs::Permissions::from_mode(0o755)).expect("mode");
    let current = std::thread::current();
    let name = current.name().expect("test name");
    let mut child = std::process::Command::new(std::env::current_exe().expect("exe"))
        .args(["--exact", name, "--test-threads=1"])
        .env("ADMIT_SLATE_LOGIN", "1")
        .env("ADMIT_SLATE_DIR", &dir)
        .env("TEST_COOKIE", "_uid=test")
        .env("PATH", format!("{}:/bin:/usr/bin", bin.display()))
        .env_remove("TEST_USER")
        .env_remove("TEST_PASSWORD")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("child");
    child.stdin.take().expect("stdin").write_all(b"user\npassword\n").expect("creds");
    let out = child.wait_with_output().expect("output");
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success() && dir.join("done").is_file(), "{text}");
    std::fs::remove_dir_all(dir).expect("cleanup");
}
