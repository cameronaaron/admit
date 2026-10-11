use super::*;
use std::io::Write;

#[test]
fn apply_only_validates_the_sections_it_writes() {
    let mut p = Profile::default();
    let x = admit_domain::Experience {
        kind: admit_domain::ExperienceKind::Employment,
        organization: "A".into(),
        title: "T".into(),
        state: "California".into(),
        country: "United States".into(),
        start: "2020-01-01".into(),
        end: None,
        current: false,
        status: "Full time".into(),
        hours_per_week: 40,
        weeks: 1,
        total_hours: 40,
        compensated: true,
        volunteer: false,
        description: String::new(),
        supervisor: Default::default(),
        ids: Default::default(),
    };
    p.experiences.experiences.push(x); // no end and not current: a violation
    let violations = |q: &Profile| validate::validate(q, today()).into_iter().filter(Finding::is_violation).count();
    assert_eq!(violations(&scoped(&p, &BTreeSet::new())), 1, "no --only: everything is checked");
    assert_eq!(violations(&scoped(&p, &[Section::Experiences].into())), 1);
    assert_eq!(violations(&scoped(&p, &[Section::Coursework, Section::HighSchool].into())), 0);
    for section in [Section::Achievements, Section::Licenses, Section::Education, Section::HighSchool] {
        let kept = scoped(&p, &[section].into());
        assert!(kept.experiences.experiences.is_empty(), "{}", section.name());
        assert_eq!(kept.personal, p.personal);
        assert_eq!(kept.prerequisites, p.prerequisites);
    }
    let school = scoped(&p, &[Section::HighSchool].into());
    assert_eq!(school.education, p.education);
    assert_eq!(school.high_school, p.high_school);
    let degrees = scoped(&p, &[Section::Education].into());
    assert_eq!(degrees.education, p.education);
    assert_eq!(degrees.high_school, admit_domain::HighSchoolFile::default());
    let awards = scoped(&p, &[Section::Achievements].into());
    assert_eq!(awards.achievements, p.achievements);
    assert_eq!(awards.licenses, admit_domain::LicensesFile::default());
    let licenses = scoped(&p, &[Section::Licenses].into());
    assert_eq!(licenses.licenses, p.licenses);
    assert_eq!(licenses.coursework, admit_domain::CourseworkFile::default());
}

fn scratch(kind: &str, portals: bool) -> (std::path::PathBuf, crate::ctx::Ctx) {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "admit-portal-{}-{}",
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
                    application: None,
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

#[tokio::test]
async fn local_failures_never_reach_a_portal() {
    tokio::task::unconstrained(async move {
        let (dir, ctx) = scratch("liaison", true);
        std::fs::write(dir.join("profile/achievements.toml"), "not = [").expect("toml");
        assert!(validate(&ctx).expect_err("validate").to_string().contains("parsing"));
        assert!(apply(&ctx, &[], false).await.expect_err("apply").to_string().contains("parsing"));
        assert!(audit(&ctx).await.expect_err("audit").to_string().contains("parsing"));
        std::fs::remove_dir_all(&dir).expect("cleanup");

        let (dir, ctx) = scratch("liaison", true);
        assert!(
            apply(&ctx, &["nope".into()], false).await.expect_err("section").to_string().contains("unknown section")
        );
        std::fs::remove_dir_all(&dir).expect("cleanup");

        let (dir, ctx) = scratch("liaison", false);
        assert!(session(&ctx, &SessionCmd::Check).await.expect_err("no portal").to_string().contains("no [[portal]]"));
        std::fs::remove_dir_all(&dir).expect("cleanup");

        // A Slate portal is rejected before any cookie or Keychain lookup.
        let (dir, ctx) = scratch("slate", true);
        for err in [
            plan(&ctx, &[]).await.expect_err("plan").to_string(),
            apply(&ctx, &[], true).await.expect_err("apply").to_string(),
            pull(&ctx, false).await.expect_err("pull").to_string(),
            courses(&ctx).await.expect_err("courses").to_string(),
            findings(&ctx, &admit_domain::Profile::default()).await.expect_err("findings").to_string(),
            session(&ctx, &SessionCmd::Check).await.expect_err("session").to_string(),
        ] {
            assert!(err.contains("liaison portals only"), "{err}");
        }
        std::fs::remove_dir_all(&dir).expect("cleanup");
    })
    .await
}

/// Stores a cookie with a `security` stub, then the session check's navigation
/// read fails. Nothing talks to the real Keychain.
#[test]
fn a_stored_session_that_fails_its_check_is_an_error() {
    if std::env::var_os("ADMIT_PORTAL_CHILD").is_some() {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");
        rt.block_on(async {
            let s = wiremock::MockServer::start().await;
            wiremock::Mock::given(wiremock::matchers::method("GET"))
                .and(wiremock::matchers::path("/applicant/api/navigation"))
                .respond_with(wiremock::ResponseTemplate::new(500).set_body_string("no"))
                .mount(&s)
                .await;
            let (dir, mut ctx) = scratch("liaison", true);
            ctx.config.portals[0].base_url = s.uri();
            ctx.config.portals[0].cookie_env = Some("TEST_COOKIE".into());
            let err = session(&ctx, &SessionCmd::Set).await.expect_err("check");
            assert!(format!("{err:#}").contains("HTTP 500"), "{err:#}");
            std::fs::remove_dir_all(dir).expect("cleanup");
        });
        let dir = std::env::var("ADMIT_PORTAL_DIR").expect("dir");
        std::fs::write(format!("{dir}/done"), "ok").expect("done");
        return;
    }
    let dir = std::env::temp_dir().join(format!("admit-portal-child-{}", std::process::id()));
    let bin = dir.join("bin");
    std::fs::create_dir_all(&bin).expect("bin");
    let security = bin.join("security");
    std::fs::write(&security, "#!/bin/sh\nexit 0\n").expect("script");
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&security, std::fs::Permissions::from_mode(0o755)).expect("mode");
    let current = std::thread::current();
    let name = current.name().expect("test name");
    let mut child = std::process::Command::new(std::env::current_exe().expect("exe"))
        .args(["--exact", name, "--test-threads=1"])
        .env("ADMIT_PORTAL_CHILD", "1")
        .env("ADMIT_PORTAL_DIR", &dir)
        .env("TEST_COOKIE", "SESSION=test")
        .env("PATH", &bin)
        .env_remove("TEST_USER")
        .env_remove("TEST_PASSWORD")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("child");
    child.stdin.take().expect("stdin").write_all(b"Cookie: SESSION=abc\n").expect("cookie");
    let out = child.wait_with_output().expect("output");
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success() && dir.join("done").is_file(), "{text}");
    std::fs::remove_dir_all(dir).expect("cleanup");
}
