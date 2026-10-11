use super::*;

#[derive(Debug, Clone, PartialEq, Serialize)]
struct E {
    name: String,
    v: u32,
    ids: PortalIds,
}

fn e(name: &str, v: u32, id: Option<i64>) -> E {
    E { name: name.into(), v, ids: id.map(|i| PortalIds::from([("p".to_string(), i)])).unwrap_or_default() }
}

fn run(desired: &[E], live: &[(i64, E)]) -> Diff<E> {
    diff("p", desired, live, |x| x.name.clone(), |x| &x.ids)
}

#[test]
fn identical_is_a_no_op_and_ids_are_ignored_in_comparison() {
    let live = vec![(7, e("a", 1, None)), (8, e("b", 2, None))];
    let desired = vec![e("a", 1, Some(7)), e("b", 2, None)];
    assert_eq!(run(&desired, &live), Diff { steps: vec![], extras: vec![] });
}

#[test]
fn creates_updates_and_reports_extras_but_never_deletes() {
    let live = vec![(7, e("a", 1, None)), (9, e("zzz", 0, None))];
    let d = run(&[e("a", 2, None), e("new", 1, None)], &live);
    assert_eq!(
        d.steps,
        vec![
            Step::Update { id: 7, fields: vec!["v".into()], desired: e("a", 2, None) },
            Step::Create(e("new", 1, None))
        ]
    );
    assert_eq!(d.extras, vec!["zzz".to_string()]);
}

#[test]
fn an_id_lets_an_entry_be_renamed_in_place() {
    let live = vec![(7, e("Old title", 1, None))];
    let d = run(&[e("New title", 1, Some(7))], &live);
    assert_eq!(d.steps, vec![Step::Update { id: 7, fields: vec!["name".into()], desired: e("New title", 1, Some(7)) }]);
    assert!(d.extras.is_empty());
}

#[test]
fn a_value_that_sometimes_fails_to_serialize_covers_both_arms() {
    use serde::Serialize;
    use serde::ser::{Error as _, SerializeMap, Serializer};
    struct Flip {
        fail: bool,
        name: &'static str,
    }
    impl Serialize for Flip {
        fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            if self.fail {
                return Err(S::Error::custom("no"));
            }
            let mut map = serializer.serialize_map(Some(1))?;
            map.serialize_entry("name", self.name)?;
            map.end()
        }
    }
    let a = Flip { fail: false, name: "a" };
    let b = Flip { fail: false, name: "b" };
    assert_eq!(changed_fields(&a, &b), ["name"]);
    assert!(changed_fields(&a, &Flip { fail: false, name: "a" }).is_empty());
    assert_eq!(changed_fields(&Flip { fail: true, name: "a" }, &a), ["(unserializable)"]);
}

#[test]
fn one_live_record_matches_at_most_one_entry() {
    let live = vec![(7, e("a", 1, None))];
    let d = run(&[e("a", 1, None), e("a", 1, None)], &live);
    assert_eq!(d.steps, vec![Step::Create(e("a", 1, None))]);
}
