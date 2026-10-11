//! Each program's Documents section (`documents.toml`): the file each
//! document slot should hold, keyed by the slot's id.
//!
//! Captured 2026-10-07 (Yale, one upload then its delete) and read from the
//! applicant bundle (`US`/`UC`, the q4Document upload and delete):
//! - `GET /programMaterials/{programSelectedId}/documents`: `sectionId`
//!   (the program's Documents subsection), `statusCompleted`, `documents[]`
//!   with `id`, `attachmentType`, `required`, `description` and the one
//!   `attachment` (`{id, fileName, uploadDate}`, or null).
//! - Upload = multipart `POST /programMaterials/{programSelectedId}/documents/{documentId}/{sectionId}/1/{"In Progress" while the
//!   program is INCOMPLETE, else "Complete"}/{statusCompleted as 0|1}`: a
//!   `file` part, then a `model` part (filename `blob`, JSON) that is the GET
//!   document with `optOut: false`. Answered `{id, fileName, uploadDate}`.
//! - Delete = `DELETE /programMaterials/{programSelectedId}/documents/{documentId}/{attachmentId}/{sectionId}/1/{program status}` (204).
//! - View = `GET /programMaterials/documents/view/{attachmentId}`: the file
//!   as held (captured 2026-10-07, the browser's "view" of a fresh upload).
//! - The bundle's file picker takes one file of 1 KB to 15 MB (`0xf00000`),
//!   of the types in [`MIME`], and refuses a password-protected PDF.
//!
//! A slot whose attachment has another file name than the profile's file is
//! replaced (delete, then upload): the slot is the profile's. A same-name
//! attachment is compared by content: plan fetches it through `view` and
//! replaces it when its SHA-256 differs from the profile file's, so what is
//! compared is what the portal holds, however it got there.
//!
//! A replace deletes before it uploads (a slot holds one attachment; an
//! upload into a full slot was never tried), so the slot is empty in
//! between. Before the DELETE the held file's bytes are written to
//! [`BACKUP`] and the replace to `documents.lock.json` (`replacing`), both
//! beside `documents.toml`. If the upload then fails, the backup is
//! uploaded again and the error says whether that worked. If the run dies
//! instead, a plan that finds the slot empty under a `replacing` record
//! names the backup, and uploads the profile's file. Once every
//! required slot holds a file, the section is marked done with the PUT the
//! Questions section uses, on the Documents subsection. Done is the
//! navigation's `subSections[].completed`: the documents GET's
//! `statusCompleted` stayed false after that PUT (live, 2026-10-07), while
//! the navigation flag turned true.

use std::path::{Path, PathBuf};

use std::collections::BTreeMap;

use admit_domain::{Finding, ProgramDocuments};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::client::{ApiError, Client, Result, arr, int, s};

/// The lock file's name, in the profile directory.
pub const LOCK: &str = "documents.lock.json";

/// Where a replace keeps the file it deletes, in the profile directory.
pub const BACKUP: &str = "documents.replaced";

/// A replace whose upload was not yet answered: the attachment it deleted
/// (or was about to), where its bytes were saved, and the file meant to
/// follow.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Replacing {
    pub deleted_attachment_id: i64,
    pub deleted_file_name: String,
    pub backup: PathBuf,
    pub file_name: String,
    pub sha256: String,
}

/// Replaces in progress, keyed `<portal>/<programSelectedId>/<documentId>`.
/// (A lock from before 2026-10-08 also has `slots`, hashes of what admit
/// uploaded; the portal's own copy is compared now, and it is ignored.)
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Lock {
    #[serde(default)]
    pub replacing: BTreeMap<String, Replacing>,
}

impl Lock {
    /// The lock in `dir`; none yet is an empty one.
    pub fn read(dir: &Path) -> std::result::Result<Lock, String> {
        let path = dir.join(LOCK);
        match std::fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Lock::default()),
            Err(e) => Err(format!("{}: {e}", path.display())),
        }
    }

    /// The slot holds a file again (uploaded, or restored): any replace
    /// recorded for it is over. Re-reads the file first so two saves in one
    /// run never drop each other's record; writes only when one is dropped.
    fn finish(dir: &Path, key: &str) -> Result<()> {
        let mut lock = Lock::load(dir)?;
        if lock.replacing.remove(key).is_none() {
            return Ok(());
        }
        lock.save(dir)
    }

    /// Before a replace's DELETE: if the upload never follows, this says so.
    fn begin_replace(dir: &Path, key: String, r: Replacing) -> Result<()> {
        let mut lock = Lock::load(dir)?;
        lock.replacing.insert(key, r);
        lock.save(dir)
    }

    fn load(dir: &Path) -> Result<Lock> {
        Lock::read(dir).map_err(ApiError::Local)
    }

    fn save(&self, dir: &Path) -> Result<()> {
        let path = dir.join(LOCK);
        let text = serde_json::to_string_pretty(self).expect("the lock encodes as JSON");
        std::fs::write(&path, text + "\n").map_err(|e| ApiError::Local(format!("{}: {e}", path.display())))
    }
}

pub fn key(portal: &str, program_id: i64, document_id: i64) -> String {
    format!("{portal}/{program_id}/{document_id}")
}

pub fn sha256(bytes: &[u8]) -> String {
    ring::digest::digest(&ring::digest::SHA256, bytes).as_ref().iter().map(|b| format!("{b:02x}")).collect()
}

/// Extensions the upload accepts, and the type the browser sends for each.
const MIME: [(&str, &str); 10] = [
    ("pdf", "application/pdf"),
    ("doc", "application/msword"),
    ("docx", "application/vnd.openxmlformats-officedocument.wordprocessingml.document"),
    ("rtf", "application/rtf"),
    ("txt", "text/plain"),
    ("png", "image/png"),
    ("jpg", "image/jpeg"),
    ("jpeg", "image/jpeg"),
    ("heic", "image/heic"),
    ("heif", "image/heif"),
];
const MIN_BYTES: usize = 1024;
const MAX_BYTES: usize = 0xf0_0000;

/// A program's Documents section as the portal holds it.
#[derive(Debug, Clone, PartialEq)]
pub struct LiveProgram {
    pub id: i64,
    /// "Organization - Program".
    pub label: String,
    /// The program's own status (`INCOMPLETE`, …), part of the write paths.
    pub status: String,
    /// The navigation's Documents subsection is marked completed.
    pub done: bool,
    pub doc: Value,
}

fn mime(path: &Path) -> Option<&'static str> {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or_default().to_lowercase();
    MIME.iter().find(|(e, _)| *e == ext).map(|&(_, m)| m)
}

/// The files the portal holds in the slots `wanted` names on `portal`, by
/// attachment id: what plan compares and a replace backs up.
pub async fn held(
    c: &Client,
    wanted: &[ProgramDocuments],
    portal: &str,
    live: &[LiveProgram],
) -> Result<BTreeMap<i64, Vec<u8>>> {
    let mut out = BTreeMap::new();
    for l in live {
        let Some(w) = wanted.iter().find(|w| w.portal == portal && norm(&w.name) == norm(&l.label)) else {
            continue;
        };
        for d in slots(&l.doc).iter().filter(|d| attached(d).is_some() && w.files.contains_key(&s(&d["id"]))) {
            let id = int(&d["attachment"]["id"]);
            out.insert(id, c.get_bytes(&format!("/programMaterials/documents/view/{id}")).await?);
        }
    }
    Ok(out)
}

/// Every selected program that has a Documents subsection.
pub async fn live(c: &Client) -> Result<Vec<LiveProgram>> {
    let mut out = Vec::new();
    for p in arr(&c.get("/getProgramNavigation").await?) {
        let Some(sub) = arr(&p["subSections"]).iter().find(|x| s(&x["name"]) == "Documents") else {
            continue;
        };
        let id = int(&p["programSelectedId"]);
        out.push(LiveProgram {
            id,
            label: format!("{} - {}", s(&p["orgName"]), s(&p["programName"])),
            status: s(&p["status"]),
            done: sub["completed"].as_bool() == Some(true),
            doc: c.get(&format!("/programMaterials/{id}/documents")).await?,
        });
    }
    Ok(out)
}

fn slots(doc: &Value) -> &[Value] {
    arr(&doc["documents"])
}

fn attached(d: &Value) -> Option<String> {
    d["attachment"].is_object().then(|| s(&d["attachment"]["fileName"]))
}

fn norm(t: &str) -> String {
    t.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

/// "CV/Resume" or "Other (Prerequisite Self Evaluation)", for messages.
fn kind(d: &Value) -> String {
    let desc: String = s(&d["description"]).chars().take(60).collect();
    if s(&d["attachmentType"]) == "Other" && !desc.is_empty() {
        format!("Other ({desc})")
    } else {
        s(&d["attachmentType"])
    }
}

/// One file to put in a slot.
#[derive(Debug, Clone, PartialEq)]
pub struct Upload {
    pub document_id: i64,
    pub kind: String,
    pub path: PathBuf,
    pub name: String,
    pub mime: &'static str,
    pub bytes: Vec<u8>,
    pub sha256: String,
    /// The attachment this replaces.
    pub replaces: Option<Held>,
    /// A same-name attachment is replaced: its content differs.
    pub changed: bool,
}

/// An attachment as the portal holds it.
#[derive(Debug, Clone, PartialEq)]
pub struct Held {
    pub id: i64,
    pub name: String,
    pub bytes: Vec<u8>,
}

/// One program's uploads, and whether the section is then marked done.
#[derive(Debug, Clone, PartialEq)]
pub struct Save {
    pub portal: String,
    /// Where `documents.lock.json` is written.
    pub dir: PathBuf,
    pub program_id: i64,
    pub program: String,
    pub status: String,
    pub doc: Value,
    pub uploads: Vec<Upload>,
    /// Every required slot holds a file and the navigation doesn't show the
    /// section done.
    pub complete: bool,
}

/// A file as the upload form would take it, or why it would refuse it.
fn read_file(path: &Path) -> std::result::Result<(&'static str, Vec<u8>), String> {
    let Some(mime) = mime(path) else {
        return Err(format!(
            "{} is not a type the portal takes ({})",
            path.display(),
            MIME.iter().map(|(e, _)| *e).collect::<Vec<_>>().join(", ")
        ));
    };
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if !(MIN_BYTES..=MAX_BYTES).contains(&bytes.len()) {
        return Err(format!("{} is {} bytes; the portal takes 1 KB to 15 MB", path.display(), bytes.len()));
    }
    if mime == "application/pdf" && bytes.windows(8).any(|w| w == b"/Encrypt") {
        return Err(format!("{} is password-protected; the portal refuses it", path.display()));
    }
    Ok((mime, bytes))
}

pub fn plan(
    wanted: &[ProgramDocuments],
    portal: &str,
    live: &[LiveProgram],
    held: &BTreeMap<i64, Vec<u8>>,
    profile_dir: &Path,
) -> (Vec<Save>, Vec<Finding>) {
    let (mut out, mut f) = (Vec::new(), Vec::new());
    let mine: Vec<&ProgramDocuments> = wanted.iter().filter(|w| w.portal == portal).collect();
    let lock = Lock::read(profile_dir)
        .map_err(|e| f.push(Finding::Violation(format!("{e}: an interrupted replace would go unreported"))))
        .ok();
    for w in &mine {
        if !live.iter().any(|l| norm(&l.label) == norm(&w.name)) {
            f.push(Finding::Violation(format!(
                "documents.toml: {:?} is not a program with documents on {portal}",
                w.name
            )));
        }
    }
    for l in live {
        let files = mine.iter().find(|w| norm(&w.name) == norm(&l.label)).map(|w| &w.files);
        let mut uploads = Vec::new();
        for (id, file) in files.into_iter().flatten() {
            let Some(d) = slots(&l.doc).iter().find(|d| s(&d["id"]) == *id) else {
                f.push(Finding::Violation(format!("{}: no document {id} on the portal", l.label)));
                continue;
            };
            let path = profile_dir.join(file);
            let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let current = attached(d);
            let interrupted = lock.as_ref().and_then(|k| k.replacing.get(&key(portal, l.id, int(&d["id"]))));
            if let (None, Some(r)) = (&current, interrupted) {
                f.push(Finding::Pending(format!(
                    "{}: document {id} ({}): an earlier run deleted {:?} (attachment {}, saved as {}) to replace it with {:?} and stopped before the upload; the slot is empty",
                    l.label,
                    kind(d),
                    r.deleted_file_name,
                    r.deleted_attachment_id,
                    r.backup.display(),
                    r.file_name
                )));
            }
            let same_name = current.as_deref() == Some(name.as_str());
            let (mime, bytes) = match read_file(&path) {
                Ok(file) => file,
                // Already there under this name: absent here (CI has no
                // documents) is not a reason to touch it.
                Err(e) if same_name => {
                    f.push(Finding::Pending(format!(
                        "{}: document {id} ({}): content not checked: {e}",
                        l.label,
                        kind(d)
                    )));
                    continue;
                }
                Err(e) => {
                    f.push(Finding::Violation(format!("{}: document {id} ({}): {e}", l.label, kind(d))));
                    continue;
                }
            };
            let sha256 = sha256(&bytes);
            let attachment = int(&d["attachment"]["id"]);
            let replaces = match (current, held.get(&attachment)) {
                (None, _) => None,
                (Some(name), Some(b)) => Some(Held { id: attachment, name, bytes: b.clone() }),
                (Some(n), None) => {
                    f.push(Finding::Violation(format!(
                        "{}: document {id} ({}): {n:?} (attachment {attachment}) was not read; not replaced",
                        l.label,
                        kind(d)
                    )));
                    continue;
                }
            };
            let changed = same_name && replaces.as_ref().is_some_and(|h| self::sha256(&h.bytes) != sha256);
            if same_name && !changed {
                continue;
            }
            uploads.push(Upload {
                document_id: int(&d["id"]),
                kind: kind(d),
                path,
                name,
                mime,
                bytes,
                sha256,
                replaces,
                changed,
            });
        }
        let missing: Vec<String> = slots(&l.doc)
            .iter()
            .filter(|d| d["required"].as_bool() == Some(true) && attached(d).is_none())
            .filter(|d| !uploads.iter().any(|u| u.document_id == int(&d["id"])))
            .map(|d| format!("{} ({})", s(&d["id"]), kind(d)))
            .collect();
        if !missing.is_empty() {
            f.push(Finding::Pending(format!(
                "{}: {} required document(s) missing: {}",
                l.label,
                missing.len(),
                missing.join("; ")
            )));
        }
        let complete = missing.is_empty() && !l.done;
        if !uploads.is_empty() || complete {
            out.push(Save {
                portal: portal.to_string(),
                dir: profile_dir.to_path_buf(),
                program_id: l.id,
                program: l.label.clone(),
                status: l.status.clone(),
                doc: l.doc.clone(),
                uploads,
                complete,
            });
        }
    }
    (out, f)
}

/// The `model` part: the section document as the form holds it.
pub fn model(doc: &Value) -> Value {
    let mut m = doc.clone();
    if let Some(o) = m.as_object_mut() {
        o.insert("optOut".into(), Value::Bool(false));
    }
    m
}

pub fn upload_path(sv: &Save, doc: &Value, document_id: i64) -> String {
    let state = if sv.status == "INCOMPLETE" { "In%20Progress" } else { "Complete" };
    let completed = u8::from(doc["statusCompleted"].as_bool() == Some(true));
    format!(
        "/programMaterials/{}/documents/{document_id}/{}/1/{state}/{completed}",
        sv.program_id,
        int(&doc["sectionId"])
    )
}

pub fn delete_path(sv: &Save, document_id: i64, attachment_id: i64) -> String {
    format!(
        "/programMaterials/{}/documents/{document_id}/{attachment_id}/{}/1/{}",
        sv.program_id,
        int(&sv.doc["sectionId"]),
        sv.status
    )
}

pub async fn apply(c: &Client, saves: &[Save]) -> Result<()> {
    for sv in saves {
        let section = format!("/programMaterials/{}/documents", sv.program_id);
        let mut doc = sv.doc.clone();
        // The browser re-reads the section after each write; the next model
        // is that document, not the stale one.
        for u in &sv.uploads {
            let slot = key(&sv.portal, sv.program_id, u.document_id);
            if let Some(h) = &u.replaces {
                let backup = backup(&sv.dir, h)?;
                let r = Replacing {
                    deleted_attachment_id: h.id,
                    deleted_file_name: h.name.clone(),
                    backup,
                    file_name: u.name.clone(),
                    sha256: u.sha256.clone(),
                };
                Lock::begin_replace(&sv.dir, slot.clone(), r)?;
                c.delete(&delete_path(sv, u.document_id, h.id)).await?;
                doc = c.get(&section).await?;
            }
            let path = upload_path(sv, &doc, u.document_id);
            let sent = upload(c, &path, &u.name, u.mime, u.bytes.clone(), &doc).await;
            if let (Err(e), Some(h)) = (&sent, &u.replaces) {
                return Err(restore(c, sv, &section, &slot, u.document_id, h, e).await);
            }
            sent?;
            // Also ends a replace an earlier run left with the slot empty.
            Lock::finish(&sv.dir, &slot)?;
            doc = c.get(&section).await?;
        }
        if sv.complete {
            c.put_empty(&format!(
                "/updateProgramMaterialsStatus/programSelected/{}/section/{}/optOut/false/completionStatus/true",
                sv.program_id,
                int(&doc["sectionId"])
            ))
            .await?;
        }
    }
    Ok(())
}

/// One file into a slot; the attachment id the portal answers with.
async fn upload(c: &Client, path: &str, name: &str, mime: &str, bytes: Vec<u8>, doc: &Value) -> Result<i64> {
    let answer = c.post_file(path, name, mime, bytes, &model(doc)).await?;
    answer["id"]
        .as_i64()
        .ok_or_else(|| ApiError::Shape(format!("POST {path}: the upload's answer has no attachment id: {answer}")))
}

/// Writes the attachment a replace is about to delete into [`BACKUP`].
fn backup(dir: &Path, h: &Held) -> Result<PathBuf> {
    let rel = Path::new(BACKUP).join(format!("{}-{}", h.id, h.name));
    let path = dir.join(&rel);
    let local = |e: std::io::Error| ApiError::Local(format!("{}: {e}", path.display()));
    std::fs::create_dir_all(dir.join(BACKUP)).map_err(local)?;
    std::fs::write(&path, &h.bytes).map_err(local)?;
    Ok(rel)
}

/// The upload after a replace's DELETE failed with `failed`: puts the
/// deleted file back, and says how that went.
async fn restore(
    c: &Client,
    sv: &Save,
    section: &str,
    slot: &str,
    document_id: i64,
    h: &Held,
    failed: &ApiError,
) -> ApiError {
    let back = async {
        let mime = mime(Path::new(&h.name))
            .ok_or_else(|| ApiError::Local(format!("{:?} is not a type the portal takes", h.name)))?;
        let doc = c.get(section).await?;
        let id = upload(c, &upload_path(sv, &doc, document_id), &h.name, mime, h.bytes.clone(), &doc).await?;
        Lock::finish(&sv.dir, slot)?;
        Ok::<i64, ApiError>(id)
    };
    let how = match back.await {
        Ok(id) => format!("{:?} was uploaded again (attachment {id})", h.name),
        Err(e) => format!(
            "putting {:?} back failed too ({e}); it is saved as {} in the profile directory and the slot is empty",
            h.name,
            Path::new(BACKUP).join(format!("{}-{}", h.id, h.name)).display()
        ),
    };
    ApiError::Local(format!(
        "{}: document {document_id}: the upload after deleting {:?} failed ({failed}); {how}",
        sv.program, h.name
    ))
}

/// Every program's document slots, for filling in `documents.toml`
/// (`open`: only required slots with no file).
pub fn listing(live: &[LiveProgram], open: bool) -> String {
    let mut out = String::new();
    for l in live {
        let shown: Vec<&Value> = slots(&l.doc)
            .iter()
            .filter(|d| !open || (d["required"].as_bool() == Some(true) && attached(d).is_none()))
            .collect();
        if shown.is_empty() {
            continue;
        }
        let done = if l.done { " [done]" } else { "" };
        out += &format!("\n{}{done}\n", l.label);
        for d in shown {
            let required = if d["required"].as_bool() == Some(true) { "required" } else { "optional" };
            out += &format!("  {} [{required}] {}\n", s(&d["id"]), s(&d["attachmentType"]));
            let desc = s(&d["description"]);
            for line in desc.lines().map(str::trim_end).filter(|l| !l.trim().is_empty()) {
                out += &format!("      {line}\n");
            }
            if let Some(n) = attached(d) {
                out += &format!("      = {n:?}\n");
            }
        }
    }
    out
}

#[cfg(test)]
#[path = "tests/documents_tests.rs"]
mod tests;
