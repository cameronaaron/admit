use super::*;

fn personal() -> Personal {
    Personal {
        legal_name: Some("Jordan Avery Rivera".into()),
        email: Some("jordan@example.org".into()),
        birth_city: Some("Lake Forest".into()),
        birth_county: Some("Los Angeles".into()),
        address: vec![PostalAddress {
            kind: "mailing".into(),
            street: "120 Maple Ave Apt 4".into(), // oss-publish:allow: fictional
            city: "Pine Valley".into(),
            region: "CA".into(),
            country: "US".into(),
            postal: "90000-1234".into(),
        }],
        ..Default::default()
    }
}

#[test]
fn the_differences_found_on_nursingcas_2026_10_06() {
    let s = Stated {
        fields: [
            ("legal_name", "Jordan Avery Rivera".to_string()),
            ("email", "jrivera@example.org".into()),
            ("birth_city", "Pine valley".into()),
            ("birth_county", "Los Angeles County".into()),
            ("sex", "Male".into()), // not in personal(): not compared
        ]
        .into(),
        addresses: vec![PostalAddress {
            kind: "mailing".into(),
            street: "120 Maple Ave. Apt 4".into(), // oss-publish:allow: fictional
            city: "Pine Valley".into(),
            region: "CA".into(),
            country: "US".into(),
            postal: "90000".into(),
        }],
    };
    let f = compare(&personal(), "nursingcas", &s);
    assert_eq!(
        f,
        vec![
            Finding::Violation(
                "nursingcas: email is \"jrivera@example.org\"; personal.toml says \"jordan@example.org\"".into()
            ),
            Finding::Violation("nursingcas: birth_city is \"Pine valley\"; personal.toml says \"Lake Forest\"".into()),
        ],
        "\"Los Angeles County\" = \"Los Angeles\", \"Ave.\" = \"Ave\", 90000 = 90000-1234"
    );
}

#[test]
fn capitals_only_are_a_note_and_blanks_are_pending() {
    let s = Stated {
        fields: [("birth_city", "Lake forest".to_string()), ("legal_name", String::new())].into(),
        addresses: vec![],
    };
    let f = compare(&personal(), "ucla", &s);
    assert!(matches!(&f[0], Finding::Pending(m) if m.contains("legal_name is blank")), "{f:?}");
    assert!(matches!(&f[1], Finding::Note(m) if m.contains("written \"Lake forest\"")), "{f:?}");
}

#[test]
fn a_missing_or_different_zip_is_reported() {
    let p = personal();
    let mut got = p.address[0].clone();
    got.postal = "99999".into();
    let s = Stated { addresses: vec![got.clone()], ..Default::default() };
    assert!(compare(&p, "test", &s).iter().any(|f| matches!(f,Finding::Violation(m) if m.contains("postal code"))));
    got.postal.clear();
    assert!(
        compare(&p, "test", &Stated { addresses: vec![got], ..Default::default() })
            .iter()
            .any(|f| matches!(f,Finding::Pending(m) if m.contains("postal code")))
    );
}

#[test]
fn equal_empty_zips_still_report_the_blank() {
    let mut p = personal();
    p.address[0].postal.clear();
    let mut got = p.address[0].clone();
    got.postal.clear();
    let f = compare(&p, "test", &Stated { addresses: vec![got], ..Default::default() });
    assert!(f.iter().any(|x| matches!(x, Finding::Pending(m) if m.contains("postal code is blank"))), "{f:?}");
}
