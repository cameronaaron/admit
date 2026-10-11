use super::*;
use std::path::Path;
use std::process::Command;

#[test]
fn normalize_keeps_letters_and_digits() {
    assert_eq!(normalize("VOC ED 384 CE"), "VOCED384CE");
    assert_eq!(normalize("fam & cs 021"), "FAMCS021");
}

#[test]
fn unknown_parser_names_itself() {
    let err = parse_file("banner", Path::new("x"), &[]).expect_err("parser");
    assert!(err.to_string().contains("banner"), "{err}");
}

/// Poppler accepts this stub (status 0) and emits no text.
const PDF: &[u8] = b"%PDF-1.1
1 0 obj<< /Type /Catalog /Pages 2 0 R >>endobj
2 0 obj<< /Type /Pages /Kids [3 0 R] /Count 1 >>endobj
3 0 obj<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 144] /Contents 4 0 R >>endobj
4 0 obj<< /Length 55 >>stream
BT /F1 12 Tf 100 100 Td (Hello Coverage) Tj ET
endstream
endobj
trailer<< /Root 1 0 R >>
%%EOF
";

#[test]
fn parsers_read_a_pdf_and_report_a_failed_one() {
    let dir = std::env::temp_dir().join(format!("admit-pdf-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("directory");
    let pdf = dir.join("t.pdf");
    std::fs::write(&pdf, PDF).expect("write");
    for parser in PARSERS {
        let err = parse_file(parser, &pdf, &laccd::COLLEGES).expect_err(parser);
        assert!(matches!(err, ParseError::Empty), "{parser}: {err}");
    }
    let missing = dir.join("missing.pdf");
    for parser in PARSERS {
        let err = parse_file(parser, &missing, &[]).expect_err(parser);
        assert!(err.to_string().contains("pdftotext"), "{parser}: {err}");
        assert!(matches!(err, ParseError::PdftotextStatus(_)), "{parser}: {err}");
    }
    std::fs::remove_dir_all(&dir).expect("cleanup");
}

#[test]
fn pdftotext_cannot_spawn() {
    if std::env::var_os("ADMIT_COVER_PDFTOTEXT_SPAWN").is_some() {
        let err = run_pdftotext(Path::new("unused.pdf"), "-raw").expect_err("spawn");
        assert!(matches!(err, ParseError::Pdftotext(_)), "{err}");
        assert!(err.to_string().contains("pdftotext"), "{err}");
        return;
    }
    let empty = std::env::temp_dir().join(format!("admit-nopath-{}", std::process::id()));
    std::fs::create_dir_all(&empty).expect("directory");
    let out = Command::new(std::env::current_exe().expect("test binary"))
        .env("ADMIT_COVER_PDFTOTEXT_SPAWN", "1")
        .env("PATH", &empty)
        .args(["--exact", "parse_tests::pdftotext_cannot_spawn", "--test-threads", "1"])
        .output()
        .expect("re-exec");
    let text = String::from_utf8_lossy(&out.stdout);
    let err_text = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success() && text.contains("pdftotext_cannot_spawn"), "{text}\n{err_text}");
    std::fs::remove_dir_all(&empty).expect("cleanup");
}
