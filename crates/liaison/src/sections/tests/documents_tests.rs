use super::*;
use serde_json::json;

const YALE: &str = "Yale University - GEPN";

fn fixture() -> Value {
    serde_json::from_str(include_str!("fixtures/yale_documents.json")).expect("fixture")
}

fn program(doc: Value) -> LiveProgram {
    LiveProgram { id: 79439764, label: YALE.into(), status: "INCOMPLETE".into(), done: false, doc }
}

/// The program as the navigation shows it once Documents is marked done.
fn done(doc: Value) -> LiveProgram {
    LiveProgram { done: true, ..program(doc) }
}

fn wanted(pairs: &[(&str, &str)]) -> Vec<ProgramDocuments> {
    vec![ProgramDocuments {
        name: YALE.into(),
        portal: "nursingcas".into(),
        files: pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
    }]
}

/// A fresh directory holding `name` with `bytes`; removed when dropped.
struct Dir(PathBuf);

impl Dir {
    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn dir_with(name: &str, bytes: &[u8]) -> Dir {
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let d = std::env::temp_dir().join(format!("admit-documents-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&d).expect("mkdir");
    std::fs::write(d.join(name), bytes).expect("write");
    Dir(d)
}

fn pdf(len: usize) -> Vec<u8> {
    let mut b = b"%PDF-1.7\n".to_vec();
    b.resize(len, b' ');
    b
}

#[test]
fn a_file_for_an_empty_slot_is_the_upload_the_browser_sent() {
    let fx = fixture();
    let d = dir_with("Authorization.pdf", &pdf(7853));
    let (saves, f) = plan(
        &wanted(&[("9540461", "Authorization.pdf")]),
        "nursingcas",
        &[program(fx["before"].clone())],
        &BTreeMap::new(),
        d.path(),
    );
    assert_eq!(f, vec![]);
    let [sv] = saves.as_slice() else { panic!("{saves:?}") };
    let [u] = sv.uploads.as_slice() else { panic!("{:?}", sv.uploads) };
    assert_eq!(
        (u.document_id, u.kind.as_str(), u.name.as_str(), u.mime),
        (9540461, "CV/Resume", "Authorization.pdf", "application/pdf")
    );
    assert_eq!(u.bytes.len(), 7853);
    assert_eq!(u.replaces, None);
    assert!(sv.complete, "the one required slot is then filled");
    assert_eq!(upload_path(sv, &sv.doc, u.document_id), fx["upload"]["path"].as_str().expect("path"));
    assert_eq!(model(&sv.doc), fx["upload"]["model"], "the model part is the section document");
    assert_eq!(delete_path(sv, 9540461, 26677663), fx["delete"]["path"].as_str().expect("path"));
}

#[test]
fn a_slot_holding_the_file_plans_nothing_and_another_name_is_replaced() {
    let fx = fixture();
    let d = dir_with("Authorization.pdf", &pdf(2000));
    // Holding the file = the same name *and* the same bytes as the
    // portal's copy (its `view`).
    let held: BTreeMap<i64, Vec<u8>> = [(26677663, pdf(2000))].into();
    // Live 2026-10-07: after the completion PUT the navigation showed
    // Documents done while the documents GET kept `statusCompleted: false`.
    let after = fx["after"].clone();
    assert_eq!(after["statusCompleted"], json!(false));
    let (saves, f) =
        plan(&wanted(&[("9540461", "Authorization.pdf")]), "nursingcas", &[done(after.clone())], &held, d.path());
    assert_eq!((saves, f), (vec![], vec![]), "same name, navigation done: converged");

    std::fs::write(d.path().join("Resume 2026.pdf"), pdf(2000)).expect("write");
    let (saves, f) =
        plan(&wanted(&[("9540461", "Resume 2026.pdf")]), "nursingcas", &[done(after.clone())], &held, d.path());
    assert_eq!(f, vec![]);
    let u = &saves[0].uploads[0];
    assert_eq!(u.replaces, Some(Held { id: 26677663, name: "Authorization.pdf".into(), bytes: pdf(2000) }));
    assert!(!u.changed, "another name, not changed content");
    assert!(!saves[0].complete, "already marked done");
    // The portal's copy differs: replaced, and the backup is its bytes.
    let other: BTreeMap<i64, Vec<u8>> = [(26677663, pdf(3000))].into();
    let (saves, _) =
        plan(&wanted(&[("9540461", "Authorization.pdf")]), "nursingcas", &[done(after.clone())], &other, d.path());
    let u = &saves[0].uploads[0];
    assert!(u.changed && u.replaces.as_ref().is_some_and(|h| h.bytes == pdf(3000)), "{u:?}");
    // A held copy that was not read is never replaced blind.
    let (saves, f) =
        plan(&wanted(&[("9540461", "Resume 2026.pdf")]), "nursingcas", &[done(after)], &BTreeMap::new(), d.path());
    assert_eq!(saves, vec![]);
    assert_eq!(
        f,
        vec![Finding::Violation(format!(
            "{YALE}: document 9540461 (CV/Resume): \"Authorization.pdf\" (attachment 26677663) was not read; not replaced"
        ))]
    );
    // Filled but not marked done: only the completion is planned.
    let (saves, _) = plan(
        &wanted(&[("9540461", "Authorization.pdf")]),
        "nursingcas",
        &[program(fx["after"].clone())],
        &held,
        d.path(),
    );
    assert!(saves[0].uploads.is_empty() && saves[0].complete);
    let (saves, _) = plan(&wanted(&[]), "nursingcas", &[program(fx["after"].clone())], &BTreeMap::new(), d.path());
    assert!(saves[0].complete, "a slot filled in the browser counts too");
}

#[test]
fn a_missing_required_file_is_pending_and_nothing_is_marked_done() {
    let fx = fixture();
    let (saves, f) = plan(&[], "nursingcas", &[program(fx["before"].clone())], &BTreeMap::new(), Path::new("."));
    assert_eq!(saves, vec![]);
    assert_eq!(f, vec![Finding::Pending(format!("{YALE}: 1 required document(s) missing: 9540461 (CV/Resume)"))]);
    // An "Other" slot is named by its description (CBU's, live 2026-10-07).
    let mut doc = fx["before"].clone();
    doc["documents"].as_array_mut().expect("docs").push(json!({"id": 9520426, "attachmentType": "Other",
        "required": true, "description": "Prerequisite Self Evaluation", "attachment": null}));
    let (_, f) = plan(&[], "nursingcas", &[program(doc)], &BTreeMap::new(), Path::new("."));
    assert!(f[0].to_string().ends_with("9540461 (CV/Resume); 9520426 (Other (Prerequisite Self Evaluation))"), "{f:?}");
}

#[test]
fn the_model_is_the_document_with_opt_out_cleared() {
    assert_eq!(model(&json!({"optOut": true, "sectionId": 1})), json!({"optOut": false, "sectionId": 1}));
    assert_eq!(model(&Value::Null), Value::Null, "a non-object is sent as read");
}

#[test]
fn files_the_upload_form_would_refuse_are_violations() {
    let fx = fixture();
    let live = [program(fx["before"].clone())];
    let encrypted = [pdf(1500), b"/Encrypt 5 0 R".to_vec()].concat();
    for (name, bytes, why) in [
        ("a.exe", pdf(2000), "is not a type the portal takes (pdf, doc"),
        ("a.pdf", pdf(1023), "is 1023 bytes; the portal takes 1 KB to 15 MB"),
        ("a.pdf", pdf(MAX_BYTES + 1), "the portal takes 1 KB to 15 MB"),
        ("a.pdf", encrypted, "is password-protected"),
    ] {
        let d = dir_with(name, &bytes);
        let (saves, f) = plan(&wanted(&[("9540461", name)]), "nursingcas", &live, &BTreeMap::new(), d.path());
        assert_eq!(saves, vec![], "{name}");
        assert!(
            matches!(f.as_slice(), [Finding::Violation(v), Finding::Pending(_)] if v.starts_with(&format!("{YALE}: document 9540461 (CV/Resume): ")) && v.contains(why)),
            "{name}: {f:?}"
        );
    }
    let (_, f) = plan(&wanted(&[("9540461", "absent.pdf")]), "nursingcas", &live, &BTreeMap::new(), Path::new("."));
    assert!(matches!(&f[0], Finding::Violation(v) if v.contains("absent.pdf")), "{f:?}");
    // Every accepted extension maps to a type, and its size is checked.
    let d = dir_with("a.DOCX", &pdf(2000));
    let (saves, _) = plan(&wanted(&[("9540461", "a.DOCX")]), "nursingcas", &live, &BTreeMap::new(), d.path());
    assert_eq!(saves[0].uploads[0].mime, "application/vnd.openxmlformats-officedocument.wordprocessingml.document");
}

#[test]
fn unknown_programs_and_slots_are_violations() {
    let fx = fixture();
    let mut w = wanted(&[("1", "x.pdf")]);
    w.push(ProgramDocuments {
        name: "Nowhere - Nothing".into(),
        portal: "nursingcas".into(),
        files: Default::default(),
    });
    w.push(ProgramDocuments { name: "Elsewhere".into(), portal: "othercas".into(), files: Default::default() });
    let (_, f) = plan(&w, "nursingcas", &[program(fx["before"].clone())], &BTreeMap::new(), Path::new("."));
    assert!(f.contains(&Finding::Violation(
        "documents.toml: \"Nowhere - Nothing\" is not a program with documents on nursingcas".into()
    )));
    assert!(f.contains(&Finding::Violation(format!("{YALE}: no document 1 on the portal"))));
    assert!(!f.iter().any(|x| x.to_string().contains("Elsewhere")), "another portal's entries are not this one's");
}

#[test]
fn a_complete_program_uploads_as_complete_and_the_status_names_the_delete() {
    let fx = fixture();
    let mut done = fx["after"].clone();
    done["statusCompleted"] = json!(true);
    let sv = Save {
        portal: "test".into(),
        dir: ".".into(),
        program_id: 79439764,
        program: YALE.into(),
        status: "COMPLETE".into(),
        doc: done.clone(),
        uploads: vec![],
        complete: false,
    };
    assert_eq!(upload_path(&sv, &done, 9540461), "/programMaterials/79439764/documents/9540461/2013507/1/Complete/1");
    assert_eq!(delete_path(&sv, 9540461, 5), "/programMaterials/79439764/documents/9540461/5/2013507/1/COMPLETE");
}

#[test]
fn the_listing_shows_each_slot_and_open_only_the_unfilled_required_ones() {
    let fx = fixture();
    let mut doc = fx["after"].clone();
    doc["documents"].as_array_mut().expect("docs").push(json!({"id": 7, "attachmentType": "Other",
        "required": false, "description": "Line one\n\n- line two ", "attachment": null}));
    let all = listing(&[program(doc.clone())], false);
    assert_eq!(
        all,
        format!(
            "\n{YALE}\n  9540461 [required] CV/Resume\n      Please provide a current résumé or curriculum vitae (2 pages maximum, do not include personal photographs).\n      = \"Authorization.pdf\"\n  7 [optional] Other\n      Line one\n      - line two\n"
        )
    );
    assert_eq!(listing(&[program(doc)], true), "", "the required slot is filled");
    assert!(
        listing(&[done(fx["before"].clone())], true).starts_with(&format!("\n{YALE} [done]\n  9540461 [required]"))
    );
}
