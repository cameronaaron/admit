//! Every Slate save is journalled around its POST; one the journal can't
//! record is never sent.
use admit_domain::journal::Journal;
use admit_slate::Client;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn saves_are_journalled_and_an_unrecordable_one_is_not_sent() {
    tokio::task::unconstrained(async move {
        let s = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/apply/ok"))
            .respond_with(ResponseTemplate::new(200).set_body_string("saved"))
            .mount(&s)
            .await;
        Mock::given(method("POST")).and(path("/apply/bad")).respond_with(ResponseTemplate::new(500)).mount(&s).await;
        let dir = std::env::temp_dir().join(format!("admit-slate-journal-{}", std::process::id()));
        let c = Client::new(&s.uri(), "c=1".into()).with_journal(Journal::to(dir.join("j.log")));
        let f = [("a".to_string(), "1".to_string())];
        c.save("ok", &f, &[]).await.expect("page save");
        c.save_form("ok", &f).await.expect("form save");
        c.save_file("ok", &f, "file1", "t.pdf", b"%PDF".to_vec()).await.expect("file save");
        c.save("bad", &f, &[]).await.expect_err("fails");
        let log = std::fs::read_to_string(dir.join("j.log")).expect("journal");
        let steps: Vec<String> =
            log.lines().map(|l| l.split('\t').skip(1).take(2).collect::<Vec<_>>().join(" ")).collect();
        assert_eq!(
            steps,
            [
                "started POST ok",
                "done POST ok",
                "started POST ok",
                "done POST ok",
                "started POST ok",
                "done POST ok",
                "started POST bad",
                "failed POST bad"
            ]
        );

        let blocked = Client::new(&s.uri(), "c=1".into()).with_journal(Journal::to(dir.join("j.log/inside")));
        let e = blocked.save("ok", &f, &[]).await.expect_err("unrecorded").to_string();
        assert!(e.starts_with("journal "), "{e}");
        assert_eq!(s.received_requests().await.expect("recorded").len(), 4, "the unrecordable save never went out");
        let _ = std::fs::remove_dir_all(dir);
    })
    .await;
}

#[tokio::test]
async fn a_save_whose_end_cannot_be_journalled_is_reported() {
    tokio::task::unconstrained(async move {
        use std::os::unix::fs::PermissionsExt;
        let s = MockServer::start().await;
        let dir = std::env::temp_dir().join(format!("admit-slate-journal-end-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let file = dir.join("j.log");
        let lock = file.clone();
        Mock::given(method("POST"))
            .respond_with(move |_: &wiremock::Request| {
                std::fs::set_permissions(&lock, std::fs::Permissions::from_mode(0o444)).expect("chmod");
                ResponseTemplate::new(200).set_body_string("saved")
            })
            .mount(&s)
            .await;
        let c = Client::new(&s.uri(), "c=1".into()).with_journal(Journal::to(&file));
        let e = c.save_form("ok", &[]).await.expect_err("end not recorded").to_string();
        assert!(e.starts_with("journal "), "{e}");
        let _ = std::fs::remove_dir_all(dir);
    })
    .await;
}
