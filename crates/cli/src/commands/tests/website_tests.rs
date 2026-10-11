use admit_domain::Finding;
use wiremock::matchers::method;
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::ctx::{Config, Ctx, WebsiteConfig};

fn ctx(dir: &std::path::Path, url: &str) -> Ctx {
    Ctx {
        profile_dir: dir.to_path_buf(),
        base: dir.to_path_buf(),
        portal_name: None,
        config: Config {
            profile: "profile".into(),
            portals: vec![],
            sync: None,
            website: Some(WebsiteConfig { url: url.into() }),
        },
    }
}

const PAGE: &str = r#"<script type="application/ld+json">{"@type":"Person","hasCredential":[{"name":"BLS","recognizedBy":{"name":"Board"}}]}</script>"#;

#[tokio::test]
async fn fetch_publish_and_profile_errors_are_returned() {
    tokio::task::unconstrained(async move {
        let dir = std::env::temp_dir().join(format!("admit-website-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        let missing = ctx(&dir, "http://127.0.0.1:1");
        let err = super::findings(&missing, &Default::default()).await.expect_err("fetch");
        assert!(err.to_string().contains("fetching"), "{err}");
        let err = super::run(&missing).await.expect_err("run");
        assert!(err.to_string().contains("fetching"), "{err}");

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_string("<html>no data</html>"))
            .mount(&server)
            .await;
        let blank = ctx(&dir, &server.uri());
        let err = super::findings(&blank, &Default::default()).await.expect_err("published");
        assert!(err.to_string().contains("JSON-LD"), "{err}");

        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_string(PAGE))
            .with_priority(1)
            .mount(&server)
            .await;
        std::fs::create_dir(dir.join("licenses.toml")).expect("not a file");
        let err = super::run(&blank).await.expect_err("profile");
        assert!(err.to_string().contains("licenses.toml"), "{err}");
        std::fs::remove_dir(dir.join("licenses.toml")).expect("undo");

        let found =
            super::findings(&blank, &blank.profile().expect("profile")).await.expect("findings").expect("configured");
        assert!(found.iter().any(|f| matches!(f, Finding::Pending(_))), "{found:?}");
        super::run(&blank).await.expect("no violations");
        std::fs::remove_dir_all(&dir).expect("cleanup");
    })
    .await
}

#[tokio::test]
async fn a_missing_website_is_skipped() {
    tokio::task::unconstrained(async move {
        let ctx = Ctx {
            profile_dir: ".".into(),
            base: ".".into(),
            portal_name: None,
            config: Config { profile: "profile".into(), portals: vec![], sync: None, website: None },
        };
        assert!(super::findings(&ctx, &Default::default()).await.expect("findings").is_none());
        let err = super::run(&ctx).await.expect_err("required");
        assert!(err.to_string().contains("no [website]"), "{err}");
    })
    .await
}
