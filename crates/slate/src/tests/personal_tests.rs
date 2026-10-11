use admit_domain::validate::personal::compare;
use admit_domain::{Finding, Personal};

use super::*;

#[test]
fn ucla_answers_are_compared_field_by_field() {
    // Keys and values as in profile/slate/ucla.toml (2026-10-06).
    let file: AnswersFile = toml::from_str(
        r#"
[answers]
"sys:first" = "Jordan"
"sys:middle" = "Avery"
"sys:last" = "Rivera"
"sys:email" = "jordan@example.org"
"sys:field:gd_birth_city" = "Lake forest"
"sys:field:gd_birth_ca_county" = "Los Angeles"
"sys:citizenship1" = "United States"

[[addresses]]
kind = "mailing"
street = "120 Maple Ave Apt 4" # oss-publish:allow: fictional
city = "Pine Valley"
region = "CA"
country = "US"
postal = "90000-1234"
"#,
    )
    .expect("answers");
    let s = stated(&file);
    assert_eq!(s.fields.get("legal_name").map(String::as_str), Some("Jordan Avery Rivera"));
    let personal = Personal {
        legal_name: Some("Jordan Avery Rivera".into()),
        email: Some("jordan@example.org".into()),
        birth_city: Some("Lake Forest".into()),
        birth_county: Some("Los Angeles".into()),
        citizenship: Some("Canada".into()),
        address: s.addresses.clone(),
        ..Default::default()
    };
    let f = compare(&personal, "ucla", &s);
    assert_eq!(
        f,
        vec![
            Finding::Note("ucla: birth_city is written \"Lake forest\"; personal.toml has \"Lake Forest\"".into()),
            Finding::Violation("ucla: citizenship is \"United States\"; personal.toml says \"Canada\"".into()),
        ]
    );
}

#[test]
fn a_multi_select_answer_is_compared_as_its_choices_joined() {
    let file: AnswersFile =
        toml::from_str("[answers]\n\"sys:field:gd_preferred_pronouns\" = [\"they/them\", \"he/him\"]\n")
            .expect("answers");
    assert_eq!(stated(&file).fields.get("pronouns").map(String::as_str), Some("they/them, he/him"));
}
