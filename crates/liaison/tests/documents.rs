//! Program documents over the fake portal: the upload must carry the parts
//! the browser sent (Yale, 2026-10-07) in its order, a replaced file is
//! saved, then deleted, and put back if its replacement fails, and a filled
//! section is marked done.
mod support;
use admit_domain::{DocumentsFile, Profile, ProgramDocuments};
use admit_liaison::{Client, Portal, Section, documents};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

const DOCS: &str = "/applicant/api/programMaterials/79439764/documents";
const VIEW: &str = "/applicant/api/programMaterials/documents/view/26677663";
const COMPLETE: &str = "/applicant/api/updateProgramMaterialsStatus/programSelected/79439764/section/2013507/optOut/false/completionStatus/true";

fn fixture() -> Value {
    serde_json::from_str(include_str!("../src/sections/tests/fixtures/yale_documents.json")).expect("fixture")
}

fn api(p: &Value) -> String {
    format!("/applicant/api{}", p.as_str().expect("path"))
}

/// The upload body as the browser built it: the file part (its captured
/// headers, then the bytes), then the `model` part holding `model`.
struct Upload {
    file_part: String,
    bytes: Vec<u8>,
    model: Value,
}

impl wiremock::Match for Upload {
    fn matches(&self, r: &Request) -> bool {
        let body = String::from_utf8_lossy(&r.body);
        let file = format!("{}\r\n\r\n{}\r\n", self.file_part, String::from_utf8_lossy(&self.bytes));
        let model_head = "Content-Disposition: form-data; name=\"model\"; filename=\"blob\"\r\nContent-Type: application/json\r\n\r\n";
        let (Some(f), Some(m)) = (body.find(&file), body.find(model_head)) else { return false };
        let json = &body[m + model_head.len()..];
        let json = &json[..json.find("\r\n--").unwrap_or(json.len())];
        f < m && serde_json::from_str::<Value>(json).ok().as_ref() == Some(&self.model)
    }
}

/// The path exactly as sent: wiremock's `path()` does not match one with
/// `%20` in it ("In%20Progress").
fn raw_path(p: impl Into<String>) -> impl Fn(&Request) -> bool + Send + Sync + 'static {
    let p = p.into();
    move |r: &Request| r.url.path() == p
}

/// A fresh directory holding `name`: tests run in parallel, so each call
/// gets its own.
fn dir(name: &str, bytes: &[u8]) -> PathBuf {
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let d = std::env::temp_dir().join(format!("admit-documents-wire-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&d).expect("mkdir");
    std::fs::write(d.join(name), bytes).expect("write");
    d
}

fn profile(file: &str) -> Profile {
    Profile {
        documents: DocumentsFile {
            programs: vec![ProgramDocuments {
                name: "Yale University - GEPN".into(),
                portal: "test".into(),
                files: [("9540461".to_string(), file.to_string())].into(),
            }],
        },
        ..Default::default()
    }
}

/// What the fake portal holds as attachment 26677663 unless a test says
/// otherwise ([`view`]).
fn portal_copy() -> Vec<u8> {
    b"%PDF-1.7\nportal\n".repeat(200)
}

/// The portal serves `bytes` as attachment 26677663.
async fn view(s: &MockServer, bytes: &[u8]) {
    Mock::given(method("GET"))
        .and(path(VIEW))
        .respond_with(ResponseTemplate::new(200).insert_header("Content-Type", "application/pdf").set_body_bytes(bytes))
        .with_priority(1)
        .mount(s)
        .await;
}

/// The backup a replace of attachment 26677663 writes.
fn backup(d: &Path) -> PathBuf {
    d.join(documents::BACKUP).join("26677663-Authorization.pdf")
}

/// The portal with Yale's section reading `first`, then `then` once
/// written; `done`: the navigation shows Documents completed.
async fn server(first: &Value, then: &Value, done: bool) -> MockServer {
    let s = support::portal().await;
    Mock::given(method("GET"))
        .and(path(VIEW))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(portal_copy()))
        .with_priority(3)
        .mount(&s)
        .await;
    let nav = json!([{"orgName": "Yale University", "programName": "GEPN", "programSelectedId": 79439764,
        "status": "INCOMPLETE", "subSections": [{"name": "Questions"}, {"name": "Documents", "completed": done}]}]);
    Mock::given(method("GET"))
        .and(path("/applicant/api/getProgramNavigation"))
        .respond_with(ResponseTemplate::new(200).set_body_json(nav))
        .with_priority(1)
        .mount(&s)
        .await;
    Mock::given(method("GET"))
        .and(path(DOCS))
        .respond_with(ResponseTemplate::new(200).set_body_json(first))
        .up_to_n_times(1)
        .with_priority(1)
        .mount(&s)
        .await;
    Mock::given(method("GET"))
        .and(path(DOCS))
        .respond_with(ResponseTemplate::new(200).set_body_json(then))
        .with_priority(2)
        .mount(&s)
        .await;
    s
}

/// Plans and applies; the count of changes and the plan as printed.
async fn plan_and_apply(s: &MockServer, dir: &Path, file: &str) -> admit_liaison::Result<(usize, String)> {
    let p = Portal { name: "test".into(), client: Client::new(&s.uri(), "SESSION=test".into()) };
    let lk = p.lookups().await.expect("lookups");
    let only: BTreeSet<Section> = [Section::Documents].into();
    let plan = p.plan(&profile(file), dir, &only, &lk).await?;
    p.apply(&plan, &lk).await?;
    Ok((plan.change_count(), plan.render()))
}

#[tokio::test]
async fn an_upload_carries_the_browsers_parts_then_marks_the_section_done() {
    tokio::task::unconstrained(async move {
        let fx = fixture();
        let bytes = b"%PDF-1.7\n".repeat(200);
        let d = dir("Authorization.pdf", &bytes);
        let s = server(&fx["before"], &fx["after"], false).await;
        Mock::given(method("POST"))
            .and(raw_path(api(&fx["upload"]["path"])))
            .and(Upload {
                file_part: fx["upload"]["file_part"].as_str().expect("part").into(),
                bytes: bytes.clone(),
                model: fx["upload"]["model"].clone(),
            })
            .respond_with(ResponseTemplate::new(200).set_body_json(&fx["upload"]["response"]))
            .with_priority(1)
            .expect(1)
            .mount(&s)
            .await;
        Mock::given(method("PUT"))
            .and(path(COMPLETE))
            .and(wiremock::matchers::body_string(""))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
            .with_priority(1)
            .expect(1)
            .mount(&s)
            .await;
        Mock::given(method("DELETE"))
            .respond_with(ResponseTemplate::new(204))
            .expect(0)
            .with_priority(1)
            .mount(&s)
            .await;
        let (n, shown) = plan_and_apply(&s, &d, "Authorization.pdf").await.expect("apply");
        assert_eq!(n, 2, "upload + done");
        assert!(shown.ends_with(".pdf\n  = mark Documents done\n"), "{shown}");
        assert!(!d.join(documents::LOCK).exists(), "an upload into an empty slot keeps no record");
        let _ = std::fs::remove_dir_all(d);
    })
    .await;
}

#[tokio::test]
async fn a_different_file_deletes_the_old_attachment_before_uploading() {
    tokio::task::unconstrained(async move {
        let fx = fixture();
        let bytes = b"%PDF-1.7\n".repeat(200);
        let d = dir("Resume 2026.pdf", &bytes);
        let mut done = fx["after"].clone();
        done["statusCompleted"] = json!(true);
        // After the delete the browser re-reads the empty slot; that is the
        // model the upload sends.
        let mut emptied = fx["before"].clone();
        emptied["statusCompleted"] = json!(true);
        let s = server(&done, &emptied, true).await;
        Mock::given(method("DELETE"))
            .and(path(api(&fx["delete"]["path"])))
            .respond_with(ResponseTemplate::new(204))
            .with_priority(1)
            .expect(1)
            .mount(&s)
            .await;
        let mut model = fx["upload"]["model"].clone();
        model["statusCompleted"] = json!(true);
        Mock::given(method("POST"))
            .and(raw_path("/applicant/api/programMaterials/79439764/documents/9540461/2013507/1/In%20Progress/1"))
            .and(Upload {
                file_part: "Content-Disposition: form-data; name=\"file\"; filename=\"Resume 2026.pdf\"\r\nContent-Type: application/pdf"
                    .into(),
                bytes,
                model,
            })
            .respond_with(ResponseTemplate::new(200).set_body_json(&fx["upload"]["response"]))
            .with_priority(1)
            .expect(1)
            .mount(&s)
            .await;
        Mock::given(method("PUT")).respond_with(ResponseTemplate::new(200)).expect(0).with_priority(1).mount(&s).await;
        let (n, shown) = plan_and_apply(&s, &d, "Resume 2026.pdf").await.expect("apply");
        assert_eq!(n, 1);
        assert!(shown.ends_with("Resume 2026.pdf (replaces \"Authorization.pdf\")\n"), "{shown}");
        // The deleted file was saved first, and the finished replace is off the lock.
        assert_eq!(std::fs::read(backup(&d)).expect("backup"), portal_copy());
        assert_eq!(documents::Lock::read(&d).expect("lock"), documents::Lock::default());
        let _ = std::fs::remove_dir_all(d);
    })
    .await;
}

#[tokio::test]
async fn each_call_fails_on_its_own() {
    tokio::task::unconstrained(async move {
        let fx = fixture();
        let d = dir("Authorization.pdf", &b"%PDF-1.7\n".repeat(200));
        for (verb, p, why) in [
            ("POST", api(&fx["upload"]["path"]), "POST /programMaterials/79439764/documents/9540461"),
            ("PUT", COMPLETE.to_string(), "PUT /updateProgramMaterialsStatus"),
        ] {
            let s = server(&fx["before"], &fx["after"], false).await;
            for (v, q) in [("POST", api(&fx["upload"]["path"])), ("PUT", COMPLETE.to_string())] {
                let status = if (v, &q) == (verb, &p) { 500 } else { 200 };
                let body = if v == "POST" { fx["upload"]["response"].clone() } else { json!({}) };
                Mock::given(method(v))
                    .and(raw_path(q.as_str()))
                    .respond_with(ResponseTemplate::new(status).set_body_json(body))
                    .with_priority(1)
                    .mount(&s)
                    .await;
            }
            let e = plan_and_apply(&s, &d, "Authorization.pdf").await.expect_err(why).to_string();
            assert!(e.contains(why) && e.contains("HTTP 500"), "{e}");
        }
        let s = server(&fx["after"], &fx["after"], false).await;
        let other = dir("Other.pdf", &b"%PDF-1.7\n".repeat(200));
        Mock::given(method("DELETE")).respond_with(ResponseTemplate::new(500)).with_priority(1).mount(&s).await;
        let e = plan_and_apply(&s, &other, "Other.pdf").await.expect_err("delete").to_string();
        assert!(e.contains("DELETE /programMaterials/79439764/documents/9540461/26677663"), "{e}");
        // The section is re-read after the delete and after the upload; each
        // read's failure stops the run.
        for (first, then) in [(&fx["after"], json!("down")), (&fx["before"], json!("down"))] {
            let s = server(first, &Value::Null, false).await;
            Mock::given(method("GET"))
                .and(path(DOCS))
                .respond_with(ResponseTemplate::new(500).set_body_json(&then))
                .with_priority(1)
                .mount(&s)
                .await;
            Mock::given(method("DELETE")).respond_with(ResponseTemplate::new(200)).with_priority(1).mount(&s).await;
            Mock::given(method("POST"))
                .respond_with(ResponseTemplate::new(200).set_body_json(&fx["upload"]["response"]))
                .with_priority(1)
                .mount(&s)
                .await;
            let e = plan_and_apply(&s, &other, "Other.pdf").await.expect_err("re-read").to_string();
            assert!(e.contains("GET /programMaterials/79439764/documents: HTTP 500"), "{e}");
        }
        // The portal's copy can't be read: no plan, so nothing is replaced blind.
        let s = server(&fx["after"], &fx["after"], false).await;
        Mock::given(method("GET"))
            .and(path(VIEW))
            .respond_with(ResponseTemplate::new(500))
            .with_priority(1)
            .mount(&s)
            .await;
        let e = plan_and_apply(&s, &d, "Authorization.pdf").await.expect_err("view").to_string();
        assert!(e.contains("GET /programMaterials/documents/view/26677663: HTTP 500"), "{e}");
        let c = Client::new(&support::portal().await.uri(), "SESSION=test".into());
        let e = c.post_file("/x", "a.pdf", "not a type", vec![], &json!({})).await.expect_err("mime").to_string();
        assert!(e.starts_with("/x: "), "{e}");
        let s = support::portal().await;
        Mock::given(method("GET"))
            .and(path("/applicant/api/getProgramNavigation"))
            .respond_with(ResponseTemplate::new(500))
            .with_priority(1)
            .mount(&s)
            .await;
        let e = plan_and_apply(&s, &d, "Authorization.pdf").await.expect_err("nav").to_string();
        assert!(e.contains("getProgramNavigation"), "{e}");
        let _ = (std::fs::remove_dir_all(d), std::fs::remove_dir_all(other));
    })
    .await;
}

/// The slot already holds "Authorization.pdf" (the fixture's `after`); the
/// DELETE and POST it would take to replace it, each expected `n` times.
async fn replacement(fx: &Value, n: u64) -> MockServer {
    let mut done = fx["after"].clone();
    done["statusCompleted"] = json!(true);
    let s = server(&done, &done, true).await;
    Mock::given(method("DELETE"))
        .and(path(api(&fx["delete"]["path"])))
        .respond_with(ResponseTemplate::new(204))
        .with_priority(1)
        .expect(n)
        .mount(&s)
        .await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&fx["upload"]["response"]))
        .with_priority(1)
        .expect(n)
        .mount(&s)
        .await;
    s
}

#[tokio::test]
async fn a_same_name_file_is_compared_with_the_portals_copy() {
    tokio::task::unconstrained(async move {
        let fx = fixture();
        // The bytes the portal holds: nothing to do, whoever uploaded them.
        let d = dir("Authorization.pdf", &portal_copy());
        let s = replacement(&fx, 0).await;
        assert_eq!(plan_and_apply(&s, &d, "Authorization.pdf").await.expect("unchanged").0, 0);
        // Same name, other bytes: replaced.
        std::fs::write(d.join("Authorization.pdf"), b"%PDF-1.7\nnew\n".repeat(200)).expect("revise");
        let s = replacement(&fx, 1).await;
        let (n, shown) = plan_and_apply(&s, &d, "Authorization.pdf").await.expect("revised");
        assert_eq!(n, 1);
        assert!(shown.ends_with("(replaces \"Authorization.pdf\": the portal's copy differs)\n"), "{shown}");
        // A lock from before the portal's copy was compared still reads; its
        // hashes are not used.
        let old = r#"{"slots": {"test/79439764/9540461": {"attachment_id": 1, "file_name": "a", "sha256": "b"}}}"#;
        std::fs::write(d.join(documents::LOCK), old).expect("old lock");
        assert_eq!(documents::Lock::read(&d).expect("old lock"), documents::Lock::default());
        let _ = std::fs::remove_dir_all(d);
    })
    .await;
}

#[tokio::test]
async fn an_absent_file_or_an_unreadable_lock_never_replaces_a_same_name_attachment() {
    tokio::task::unconstrained(async move {
        let fx = fixture();
        let p = |s: &MockServer| Portal { name: "test".into(), client: Client::new(&s.uri(), "SESSION=test".into()) };
        let only: BTreeSet<Section> = [Section::Documents].into();
        // CI has no documents: the slot is not checked, and not touched.
        let d = dir("other.txt", b"x");
        let s = replacement(&fx, 0).await;
        let plan = p(&s)
            .plan(&profile("Authorization.pdf"), &d, &only, &p(&s).lookups().await.expect("lk"))
            .await
            .expect("plan");
        assert_eq!(plan.change_count(), 0);
        assert!(
            plan.findings.iter().any(|f| !f.is_violation() && f.to_string().contains("content not checked")),
            "{:?}",
            plan.findings
        );
        // A lock that can't be read is a violation; the content is still compared.
        let d = dir("Authorization.pdf", &portal_copy());
        std::fs::write(d.join(documents::LOCK), "{not json").expect("corrupt");
        let plan = p(&s)
            .plan(&profile("Authorization.pdf"), &d, &only, &p(&s).lookups().await.expect("lk"))
            .await
            .expect("plan");
        assert_eq!(plan.change_count(), 0);
        assert!(
            plan.findings
                .iter()
                .any(|f| f.is_violation() && f.to_string().contains("an interrupted replace would go unreported"))
        );
        let _ = std::fs::remove_dir_all(d);
    })
    .await;
}

#[tokio::test]
async fn an_upload_answer_without_an_attachment_id_stops_the_run() {
    tokio::task::unconstrained(async move {
        let fx = fixture();
        let d = dir("Authorization.pdf", &b"%PDF-1.7\n".repeat(200));
        let s = server(&fx["before"], &fx["after"], true).await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok": true})))
            .with_priority(1)
            .mount(&s)
            .await;
        let e = plan_and_apply(&s, &d, "Authorization.pdf").await.expect_err("shape").to_string();
        assert!(e.contains("no attachment id"), "{e}");
        assert!(!d.join(documents::LOCK).exists(), "nothing recorded");
        let _ = std::fs::remove_dir_all(d);
    })
    .await;
}

/// Answers with `answers` in turn, the last one from then on.
fn in_turn(answers: Vec<ResponseTemplate>) -> impl Fn(&Request) -> ResponseTemplate + Send + Sync + 'static {
    let n = std::sync::atomic::AtomicUsize::new(0);
    move |_: &Request| {
        let i = n.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        answers[i.min(answers.len() - 1)].clone()
    }
}

#[tokio::test]
async fn a_run_that_stops_between_the_delete_and_the_upload_is_recorded_and_recovered() {
    tokio::task::unconstrained(async move {
        let fx = fixture();
        let new = b"%PDF-1.7\nnew\n".repeat(200);
        let d = dir("Authorization.pdf", &new);
        let journal = admit_domain::journal::Journal::to(d.join("journal.log"));
        let portal = |s: &MockServer| Portal {
            name: "test".into(),
            client: Client::new(&s.uri(), "SESSION=test".into()).with_journal(journal.clone()),
        };
        let only: BTreeSet<Section> = [Section::Documents].into();

        // The DELETE goes through, the upload fails, and so does putting the
        // old file back: the slot is now empty.
        let mut done = fx["after"].clone();
        done["statusCompleted"] = json!(true);
        let s = server(&done, &fx["before"], true).await;
        Mock::given(method("DELETE"))
            .respond_with(ResponseTemplate::new(204))
            .with_priority(1)
            .expect(1)
            .mount(&s)
            .await;
        Mock::given(method("POST")).respond_with(ResponseTemplate::new(500)).with_priority(1).expect(2).mount(&s).await;
        let p = portal(&s);
        let lk = p.lookups().await.expect("lookups");
        let plan = p.plan(&profile("Authorization.pdf"), &d, &only, &lk).await.expect("plan");
        let e = p.apply(&plan, &lk).await.expect_err("upload fails").to_string();
        assert!(e.contains("the upload after deleting \"Authorization.pdf\" failed (POST"), "{e}");
        assert!(
            e.ends_with(
                "putting \"Authorization.pdf\" back failed too (POST /programMaterials/79439764/documents/9540461/2013507/1/In%20Progress/0: HTTP 500: ); it is saved as documents.replaced/26677663-Authorization.pdf in the profile directory and the slot is empty"
            ),
            "{e}"
        );

        // What ran is on record: the delete done, both uploads failed.
        let log = std::fs::read_to_string(d.join("journal.log")).expect("journal");
        let steps: Vec<String> =
            log.lines().map(|l| l.split('\t').skip(1).take(2).collect::<Vec<_>>().join(" ")).collect();
        let del = "DELETE /programMaterials/79439764/documents/9540461/26677663/2013507/1/INCOMPLETE";
        let post = "POST /programMaterials/79439764/documents/9540461/2013507/1/In%20Progress/0";
        assert_eq!(
            steps,
            [
                format!("started {del}"),
                format!("done {del}"),
                format!("started {post}"),
                format!("failed {post}"),
                format!("started {post}"),
                format!("failed {post}"),
            ]
        );
        // The deleted file is on disk, and the lock names the replace that didn't finish.
        assert_eq!(std::fs::read(backup(&d)).expect("backup"), portal_copy());
        let held = documents::Lock::read(&d).expect("lock");
        let r = &held.replacing["test/79439764/9540461"];
        assert_eq!((r.deleted_attachment_id, r.sha256.as_str()), (26677663, documents::sha256(&new).as_str()));
        assert_eq!(r.backup, Path::new("documents.replaced/26677663-Authorization.pdf"));

        // The next run says what happened and uploads into the empty slot.
        let s = server(&fx["before"], &fx["after"], true).await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&fx["upload"]["response"]))
            .with_priority(1)
            .expect(1)
            .mount(&s)
            .await;
        let p = portal(&s);
        let plan = p.plan(&profile("Authorization.pdf"), &d, &only, &lk).await.expect("plan");
        assert!(
            plan.findings.iter().any(|f| f.to_string().contains(
                "an earlier run deleted \"Authorization.pdf\" (attachment 26677663, saved as documents.replaced/26677663-Authorization.pdf)"
            )),
            "{:?}",
            plan.findings
        );
        assert_eq!(plan.change_count(), 1);
        p.apply(&plan, &lk).await.expect("recovered");
        let held = documents::Lock::read(&d).expect("lock");
        assert!(held.replacing.is_empty(), "the finished replace is no longer pending");
        let _ = std::fs::remove_dir_all(d);
    })
    .await;
}

/// A replace of the portal's "Authorization.pdf" by `new.pdf` whose POSTs
/// are answered by `posts`; the error, and the directory.
async fn failed_replace(posts: impl wiremock::Respond + 'static) -> (String, PathBuf) {
    let fx = fixture();
    let d = dir("new.pdf", &b"%PDF-1.7\nnew\n".repeat(200));
    let s = server(&fx["after"], &fx["before"], true).await;
    Mock::given(method("DELETE")).respond_with(ResponseTemplate::new(204)).with_priority(1).mount(&s).await;
    Mock::given(method("POST")).respond_with(posts).with_priority(1).mount(&s).await;
    let e = plan_and_apply(&s, &d, "new.pdf").await.expect_err("the upload fails").to_string();
    (e, d)
}

#[tokio::test]
async fn a_failed_replacement_puts_the_deleted_file_back() {
    tokio::task::unconstrained(async move {
        let fx = fixture();
        let ok = ResponseTemplate::new(200).set_body_json(&fx["upload"]["response"]);
        // The new file is refused, the old one goes back in.
        let (e, d) = failed_replace(in_turn(vec![ResponseTemplate::new(500), ok.clone()])).await;
        assert!(e.ends_with("\"Authorization.pdf\" was uploaded again (attachment 26677663)"), "{e}");
        assert_eq!(documents::Lock::read(&d).expect("lock"), documents::Lock::default(), "nothing left pending");
        let _ = std::fs::remove_dir_all(d);
        // The re-upload carries the deleted bytes under the deleted name.
        let s = server(&fx["after"], &fx["before"], true).await;
        let d = dir("new.pdf", &b"%PDF-1.7\nnew\n".repeat(200));
        Mock::given(method("DELETE")).respond_with(ResponseTemplate::new(204)).with_priority(1).mount(&s).await;
        Mock::given(method("POST"))
            .and(Upload {
                file_part: fx["upload"]["file_part"].as_str().expect("part").into(),
                bytes: portal_copy(),
                model: fx["upload"]["model"].clone(),
            })
            .respond_with(ok.clone())
            .with_priority(1)
            .expect(1)
            .mount(&s)
            .await;
        Mock::given(method("POST")).respond_with(ResponseTemplate::new(500)).with_priority(2).mount(&s).await;
        let e = plan_and_apply(&s, &d, "new.pdf").await.expect_err("new refused").to_string();
        assert!(e.contains("was uploaded again"), "{e}");
        let _ = std::fs::remove_dir_all(d);
    })
    .await;
}

#[tokio::test]
async fn a_restore_that_fails_says_where_the_deleted_file_is() {
    tokio::task::unconstrained(async move {
        let fx = fixture();
        // A held file whose type the upload form would refuse can't go back.
        let mut zip = fx["after"].clone();
        zip["documents"][0]["attachment"]["fileName"] = json!("Old.zip");
        let s = server(&zip, &fx["before"], true).await;
        let d = dir("new.pdf", &b"%PDF-1.7\nnew\n".repeat(200));
        Mock::given(method("DELETE")).respond_with(ResponseTemplate::new(204)).with_priority(1).mount(&s).await;
        Mock::given(method("POST")).respond_with(ResponseTemplate::new(500)).with_priority(1).expect(1).mount(&s).await;
        let e = plan_and_apply(&s, &d, "new.pdf").await.expect_err("refused").to_string();
        assert!(
            e.ends_with("putting \"Old.zip\" back failed too (\"Old.zip\" is not a type the portal takes); it is saved as documents.replaced/26677663-Old.zip in the profile directory and the slot is empty"),
            "{e}"
        );
        assert_eq!(std::fs::read(d.join("documents.replaced/26677663-Old.zip")).expect("backup"), portal_copy());
        let _ = std::fs::remove_dir_all(d);
        // The section can't be re-read for the restore's model.
        let s = support::portal().await;
        let nav = json!([{"orgName": "Yale University", "programName": "GEPN", "programSelectedId": 79439764,
            "status": "INCOMPLETE", "subSections": [{"name": "Documents", "completed": true}]}]);
        Mock::given(method("GET"))
            .and(path("/applicant/api/getProgramNavigation"))
            .respond_with(ResponseTemplate::new(200).set_body_json(nav))
            .with_priority(1)
            .mount(&s)
            .await;
        view(&s, &portal_copy()).await;
        Mock::given(method("GET"))
            .and(path(DOCS))
            .respond_with(in_turn(vec![
                ResponseTemplate::new(200).set_body_json(&fx["after"]),
                ResponseTemplate::new(200).set_body_json(&fx["before"]),
                ResponseTemplate::new(500),
            ]))
            .with_priority(1)
            .mount(&s)
            .await;
        Mock::given(method("DELETE")).respond_with(ResponseTemplate::new(204)).with_priority(1).mount(&s).await;
        Mock::given(method("POST")).respond_with(ResponseTemplate::new(500)).with_priority(1).expect(1).mount(&s).await;
        let d = dir("new.pdf", &b"%PDF-1.7\nnew\n".repeat(200));
        let e = plan_and_apply(&s, &d, "new.pdf").await.expect_err("re-read").to_string();
        assert!(e.contains("back failed too (GET /programMaterials/79439764/documents: HTTP 500"), "{e}");
        let _ = std::fs::remove_dir_all(d);
    })
    .await;
}

#[tokio::test]
async fn a_backup_that_cannot_be_written_stops_before_the_delete() {
    tokio::task::unconstrained(async move {
        let fx = fixture();
        // A file where the backup directory goes, then a directory where the backup goes.
        for block in [documents::BACKUP.to_string(), format!("{}/26677663-Authorization.pdf/x", documents::BACKUP)] {
            let d = dir("new.pdf", &b"%PDF-1.7\nnew\n".repeat(200));
            let blocked = d.join(&block);
            std::fs::create_dir_all(blocked.parent().expect("parent")).expect("mkdir");
            std::fs::write(&blocked, b"x").expect("block");
            let s = replacement(&fx, 0).await;
            let e = plan_and_apply(&s, &d, "new.pdf").await.expect_err("unwritable").to_string();
            assert!(e.contains(documents::BACKUP), "{block}: {e}");
            let _ = std::fs::remove_dir_all(d);
        }
    })
    .await;
}

#[tokio::test]
async fn a_write_the_journal_cannot_record_is_never_sent() {
    tokio::task::unconstrained(async move {
        let s = support::portal().await;
        Mock::given(method("PUT")).respond_with(ResponseTemplate::new(200)).expect(0).with_priority(1).mount(&s).await;
        let blocked = dir("file", b"x").join("file").join("journal.log");
        let c = Client::new(&s.uri(), "SESSION=test".into()).with_journal(admit_domain::journal::Journal::to(blocked));
        let e = c.put_empty("/x").await.expect_err("unrecorded").to_string();
        assert!(e.starts_with("journal "), "{e}");
        c.get("/navigation").await.expect("reads are not journalled");
    })
    .await;
}

#[tokio::test]
async fn a_write_whose_end_cannot_be_journalled_is_reported() {
    tokio::task::unconstrained(async move {
        use std::os::unix::fs::PermissionsExt;
        let s = support::portal().await;
        let log = dir("journal.log", b"");
        let file = log.join("journal.log");
        let lock = file.clone();
        // The journal turns read-only while the request is in flight: the
        // write went out, its `done` line can't be added.
        Mock::given(method("PUT"))
            .respond_with(move |_: &Request| {
                std::fs::set_permissions(&lock, std::fs::Permissions::from_mode(0o444)).expect("chmod");
                ResponseTemplate::new(200)
            })
            .with_priority(1)
            .mount(&s)
            .await;
        let c = Client::new(&s.uri(), "SESSION=test".into()).with_journal(admit_domain::journal::Journal::to(&file));
        let e = c.put_empty("/x").await.expect_err("end not recorded").to_string();
        assert!(e.starts_with("journal "), "{e}");
        let text = std::fs::read_to_string(&file).expect("journal");
        assert_eq!(text.lines().count(), 1, "only `started`: read as interrupted, never as done");
        let _ = std::fs::remove_dir_all(log);
    })
    .await;
}

#[tokio::test]
async fn a_lock_that_cannot_be_rewritten_stops_before_the_delete_or_after_the_upload() {
    tokio::task::unconstrained(async move {
        use std::os::unix::fs::PermissionsExt;
        let fx = fixture();
        let new = b"%PDF-1.7\nnew\n".repeat(200);
        let read_only =
            |d: &Path| std::fs::set_permissions(d.join(documents::LOCK), std::fs::Permissions::from_mode(0o444));
        // A replace: the lock can be read but not written, so the replace
        // can't be recorded and its DELETE is never sent.
        let d = dir("Authorization.pdf", &new);
        std::fs::write(d.join(documents::LOCK), "{}").expect("lock");
        read_only(&d).expect("chmod");
        let s = replacement(&fx, 0).await;
        let e = plan_and_apply(&s, &d, "Authorization.pdf").await.expect_err("unwritable").to_string();
        assert!(e.contains(documents::LOCK), "{e}");
        let _ = std::fs::remove_dir_all(d);
        // The lock turns read-only while an upload is in flight: the upload,
        // or the restore after a failed one, went out and its end can't be
        // recorded.
        for first in [200, 500] {
            let d = dir("Authorization.pdf", &new);
            let lock = d.clone();
            let ok = ResponseTemplate::new(200).set_body_json(&fx["upload"]["response"]);
            let n = std::sync::atomic::AtomicUsize::new(0);
            let s = server(&fx["after"], &fx["after"], true).await;
            Mock::given(method("DELETE")).respond_with(ResponseTemplate::new(204)).with_priority(1).mount(&s).await;
            Mock::given(method("POST"))
                .respond_with(move |_: &Request| {
                    read_only(&lock).expect("chmod");
                    let i = n.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    if i == 0 { ResponseTemplate::new(first) } else { ok.clone() }
                })
                .with_priority(1)
                .mount(&s)
                .await;
            let e = plan_and_apply(&s, &d, "Authorization.pdf").await.expect_err("unwritable").to_string();
            assert!(e.contains(documents::LOCK), "{first}: {e}");
            let _ = std::fs::remove_dir_all(d);
        }
    })
    .await;
}

#[tokio::test]
async fn profile_entries_the_portal_lacks_or_files_it_would_refuse_are_violations() {
    tokio::task::unconstrained(async move {
        let fx = fixture();
        let s = server(&fx["before"], &fx["after"], false).await;
        let d = dir("resume.exe", &b"MZ".repeat(600));
        let p = Portal { name: "test".into(), client: Client::new(&s.uri(), "SESSION=test".into()) };
        let lk = p.lookups().await.expect("lookups");
        let mut profile = profile("resume.exe");
        profile.documents.programs[0].files.insert("1".into(), "resume.exe".into());
        profile.documents.programs.push(ProgramDocuments {
            name: "Nowhere - Nothing".into(),
            portal: "test".into(),
            files: Default::default(),
        });
        let plan = p.plan(&profile, &d, &[Section::Documents].into(), &lk).await.expect("plan");
        let v: Vec<String> = plan.findings.iter().filter(|f| f.is_violation()).map(ToString::to_string).collect();
        for want in [
            "\"Nowhere - Nothing\" is not a program with documents on test",
            "no document 1 on the portal",
            "resume.exe is not a type the portal takes",
        ] {
            assert!(v.iter().any(|x| x.contains(want)), "{want}: {v:?}");
        }
        assert!(plan.documents.iter().all(|sv| sv.uploads.is_empty()), "nothing refused is uploaded");
        let _ = std::fs::remove_dir_all(d);
    })
    .await;
}

#[tokio::test]
async fn a_replace_whose_lock_cannot_be_read_never_deletes() {
    tokio::task::unconstrained(async move {
        let fx = fixture();
        // A new file name still plans a replace when the lock is unreadable;
        // the replace can't be recorded, so the DELETE never goes out.
        let d = dir("Other.pdf", &b"%PDF-1.7\n".repeat(200));
        std::fs::create_dir(d.join(documents::LOCK)).expect("a directory where the lock goes");
        let s = replacement(&fx, 0).await;
        let e = plan_and_apply(&s, &d, "Other.pdf").await.expect_err("unreadable").to_string();
        assert!(e.contains(documents::LOCK), "{e}");
        // An upload into an empty slot goes out, then the lock it would
        // clear of an earlier replace can't be read: reported, not skipped.
        let s = server(&fx["before"], &fx["after"], true).await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&fx["upload"]["response"]))
            .with_priority(1)
            .expect(1)
            .mount(&s)
            .await;
        let e = plan_and_apply(&s, &d, "Other.pdf").await.expect_err("unreadable").to_string();
        assert!(e.contains(documents::LOCK), "{e}");
        let _ = std::fs::remove_dir_all(d);
    })
    .await;
}

#[test]
fn a_held_copy_that_was_not_read_is_never_replaced_blind() {
    // `Portal::plan` reads every attachment it compares; called with less,
    // plan refuses the replace instead of deleting what it never saw.
    let fx = fixture();
    let d = dir("Other.pdf", &b"%PDF-1.7\n".repeat(200));
    let live = [documents::LiveProgram {
        id: 79439764,
        label: "Yale University - GEPN".into(),
        status: "INCOMPLETE".into(),
        done: true,
        doc: fx["after"].clone(),
    }];
    let (saves, f) = documents::plan(&profile("Other.pdf").documents.programs, "test", &live, &Default::default(), &d);
    assert_eq!(saves, vec![]);
    let f: Vec<String> = f.iter().map(ToString::to_string).collect();
    assert!(f.iter().any(|x| x.contains("(attachment 26677663) was not read; not replaced")), "{f:?}");
    let _ = std::fs::remove_dir_all(d);
}
