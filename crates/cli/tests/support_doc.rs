//! SUPPORT.md says, per Liaison section, how far each write is proven. A
//! section added without a row there would be supported by nobody's word.
use admit_liaison::Section;

#[test]
fn every_liaison_section_has_a_row_in_the_support_table() {
    let doc = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../SUPPORT.md")).expect("SUPPORT.md");
    let table = &doc[doc.find("## Liaison").expect("Liaison section")..];
    let table = &table[..table.find("\n## ").unwrap_or(table.len())];
    for s in Section::ALL {
        let row = format!("| {} ", s.name());
        assert!(table.lines().any(|l| l.starts_with(&row)), "SUPPORT.md has no row for {:?} ({row})", s.name());
    }
    for level in ["live", "captured", "bundle", "no"] {
        assert!(doc.contains(&format!("- **{level}**")), "level {level} undefined");
    }
}

/// SUPPORT.md's claim that only `base_url` names a deployment: no adapter
/// code (comments aside) mentions a CAS, cycle or school host.
#[test]
fn no_adapter_code_names_a_deployment() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut hits = Vec::new();
    for krate in ["liaison", "slate", "html"] {
        let mut stack = vec![root.join("crates").join(krate).join("src")];
        while let Some(dir) = stack.pop() {
            for e in std::fs::read_dir(&dir).expect("src") {
                let p = e.expect("entry").path();
                if p.is_dir() {
                    if p.file_name().is_some_and(|n| n != "tests") {
                        stack.push(p);
                    }
                    continue;
                }
                let text = std::fs::read_to_string(&p).expect("source");
                for (n, line) in text.lines().enumerate() {
                    // A URL in a string has `//` too: only whole comment lines are skipped.
                    let code = if line.trim_start().starts_with("//") { String::new() } else { line.to_lowercase() };
                    for host in ["myliaison.com", "nursingcas20", "grad.ucla.edu", "apu.edu"] {
                        if code.contains(host) {
                            hits.push(format!("{}:{}: {host}", p.display(), n + 1));
                        }
                    }
                }
            }
        }
    }
    assert!(hits.is_empty(), "deployment named in adapter code: {hits:?}");
}
