#[test]
fn non_object_values_are_reported_as_unserializable() {
    assert_eq!(super::changed_fields(&1, &2), ["(unserializable)"]);
}
