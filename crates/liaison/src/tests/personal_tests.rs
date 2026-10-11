use serde_json::json;

use super::*;

fn q(name: &str, v: &str, opts: Value) -> Value {
    json!({"shortFieldName": name, "value": [v], "answerOptions": opts})
}

#[test]
fn reads_the_captured_biographic_and_contact_sections() {
    // Shapes as captured 2026-10-06 (PUT responses of sections 24164, 24166).
    let bio = json!({"questionBlocks": [
        {"id": "name", "questions": [q("first_name", "Jordan", json!([])), q("middle_name", "Avery", json!([])),
            q("last_name", "Rivera", json!([])), q("suffix", "", json!([]))]},
        {"id": "gender", "questions": [q("gender", "7154", json!([{"id": 7154, "text": "Male", "code": "MALE"}]))]},
        {"id": "customQuestions", "questions": [
            q("pronounsPreferred", "33004520", json!([{"id": 33004520, "text": "He/Him/His"}]))]},
        {"id": "birthInfo", "questions": [q("birth_date", "1990-01-15", json!([])),
            q("birth_country", "222", json!([{"id": 222, "text": null, "code": "US"}])),
            q("birth_state", "339", json!([{"id": 339, "text": null, "code": "US-CA"}])),
            q("birth_county", "763", json!([{"id": 763, "text": null, "code": "CA037"}])),
            q("birth_city", "Pine valley", json!([]))]}]});
    let contact = json!({"questionBlocks": [{"id": "currentAddress", "questions": [
        q("country_id", "222", json!([{"id": 222, "text": null, "code": "US"}])),
        q("street_address_1", "120 Maple Ave Apt 4", json!([])), q("street_address_2", "", json!([])), // oss-publish:allow: fictional
        q("city", "Pine Valley", json!([])),
        q("state_id", "339", json!([{"id": 339, "text": null, "code": "US-CA"}])),
        q("zip", "90000-1234", json!([])), q("pstreet_address_1", "", json!([]))]}]});
    let countries = Table::from_rows("country", &json!([{"id": 222, "text": "United States"}]));
    let states = Table::from_rows("state", &json!([{"id": 339, "text": "California"}]));
    let counties = Table::from_rows("county", &json!([{"id": 763, "text": "Los Angeles County"}]));
    let n = Names { countries: &countries, us_states: &states, counties: &counties };
    let st = stated(&bio, &contact, "jrivera@example.org", &n);
    let f = |k: &str| st.fields.get(k).cloned().unwrap_or_default();
    assert_eq!(f("legal_name"), "Jordan Avery Rivera");
    assert_eq!(f("birth_country"), "United States");
    assert_eq!(f("birth_state"), "California");
    assert_eq!(f("birth_county"), "Los Angeles County");
    assert_eq!(f("birth_city"), "Pine valley");
    assert_eq!(f("sex"), "Male");
    assert_eq!(f("pronouns"), "He/Him/His");
    assert_eq!(f("email"), "jrivera@example.org");
    assert_eq!(
        st.addresses,
        vec![PostalAddress {
            kind: "mailing".into(),
            street: "120 Maple Ave Apt 4".into(), // oss-publish:allow: fictional
            city: "Pine Valley".into(),
            region: "CA".into(),
            country: "US".into(),
            postal: "90000-1234".into(),
        }],
        "a blank permanent address is not one"
    );
}

#[test]
fn a_get_resolves_lookup_dropdowns_through_the_tables() {
    // GET shape (2026-10-06, section 24168): a lookup-backed dropdown's one
    // option names its table, so the chosen id is not among the options.
    // Reading codes from the options alone left region and country blank.
    let lk = |table: &str| json!([{"id": 7223, "text": table, "code": "7223"}]);
    let bio = json!({"questionBlocks": [{"id": "birthInfo", "questions": [
        q("birth_country", "222", lk("LK_Countries")), q("birth_state", "339", lk("LK_States"))]}]});
    let contact = json!({"questionBlocks": [{"id": "currentAddress", "questions": [
        q("country_id", "222", lk("LK_Countries")), q("street_address_1", "1 Main St", json!([])), // oss-publish:allow: fictional
        q("state_id", "339", lk("LK_States"))]}]});
    let countries = Table::from_rows("country", &json!([{"id": 222, "code": "US", "text": "United States"}]));
    let states = Table::from_rows("state", &json!([{"id": 339, "code": "US-CA", "text": "California"}]));
    let n = Names { countries: &countries, us_states: &states, counties: &Table::default() };
    let st = stated(&bio, &contact, "", &n);
    assert_eq!(st.fields.get("birth_country").map(String::as_str), Some("United States"));
    assert_eq!(st.fields.get("birth_state").map(String::as_str), Some("California"));
    assert_eq!((st.addresses[0].region.as_str(), st.addresses[0].country.as_str()), ("CA", "US"));
}

#[test]
fn an_unlisted_numeric_answer_without_a_lookup_stays_the_id() {
    let bio = json!({"questionBlocks": [{"id": "gender", "questions": [q("gender", "9999", json!([]))]}]});
    let contact = json!({"questionBlocks": []});
    let empty = Table::default();
    let n = Names { countries: &empty, us_states: &empty, counties: &empty };
    let st = stated(&bio, &contact, "", &n);
    assert_eq!(st.fields["sex"], "9999");
}

#[test]
fn an_unanswered_dropdown_has_no_code() {
    // A foreign address leaves the state dropdown blank: no region, rather
    // than the blank value read as an id.
    let bio = json!({"questionBlocks": []});
    let contact = json!({"questionBlocks": [{"id": "currentAddress", "questions": [
        q("street_address_1", "1 Rue X", json!([])), q("state_id", "", json!([])),
        q("country_id", "75", json!([{"id": 75, "text": "France", "code": "FR"}]))]}]});
    let empty = Table::default();
    let n = Names { countries: &empty, us_states: &empty, counties: &empty };
    let st = stated(&bio, &contact, "", &n);
    assert_eq!((st.addresses[0].region.as_str(), st.addresses[0].country.as_str()), ("", "FR"));
}

#[test]
fn finds_sections_in_navigation() {
    let nav = json!({"personalInfo": {"sections": [{"name": "biographicInfo", "id": 24164},
        {"name": "contactInfo", "id": 24166}]}});
    assert_eq!(section_id(&nav, "contactInfo"), Some(24166));
    assert_eq!(section_id(&nav, "familyInfo"), None);
}
