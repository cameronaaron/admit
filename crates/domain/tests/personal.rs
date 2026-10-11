//! personal.toml: stated answers load typed, and a misspelled key is an
//! error rather than a silently missing answer.

use admit_domain::Personal;

#[test]
fn stated_answers_load_typed() {
    let p: Personal = toml::from_str(
        r#"
        gender_identity = "Man"
        california_resident = true
        parent2_education = "Unknown"
        receiving_financial_aid = true
        [[address]]
        kind = "mailing"
        street = "1 Example St" # oss-publish:allow: fictional
        city = "Springfield"
        region = "CA"
        country = "US"
        postal = "00000"
        [fee_waiver]
        basis = "income"
        household_size = 1
        "#,
    )
    .expect("parses");
    assert_eq!(p.gender_identity.as_deref(), Some("Man"));
    assert_eq!(p.california_resident, Some(true));
    assert_eq!(p.receiving_financial_aid, Some(true));
    assert_eq!(p.address[0].kind, "mailing");
    assert_eq!(p.fee_waiver.and_then(|f| f.household_size), Some(1));
    assert_eq!(p.parent1_education, None, "unstated stays None: ask");
}

#[test]
fn a_misspelled_key_is_rejected() {
    let err = toml::from_str::<Personal>("gender_identiy = \"Man\"").expect_err("typo");
    assert!(err.to_string().contains("gender_identiy"), "{err}");
}
