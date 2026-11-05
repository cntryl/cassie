use super::KeyedMap;
use crate::config::CassieRuntimeLimits;
use crate::runtime::QueryExecutionControls;
use std::hash::{Hash, Hasher};

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Collision(u8);
impl Hash for Collision {
    fn hash<H: Hasher>(&self, state: &mut H) {
        0_u8.hash(state);
    }
}

#[test]
fn should_preserve_ordered_output_for_colliding_semantic_records() {
    // Arrange
    let controls = QueryExecutionControls::from_limits(
        &CassieRuntimeLimits::default(),
        std::time::Instant::now(),
    );
    let mut records = KeyedMap::new(Some(&controls)).expect("index admission");

    // Act
    records.insert(Collision(2), "two").expect("second record");
    records.insert(Collision(1), "one").expect("first record");
    let first = records.get(&Collision(1)).copied();
    let second = records.get(&Collision(2)).copied();
    let replacement = records
        .insert(Collision(1), "changed")
        .expect("exact duplicate");
    let entries = records.into_sorted_entries().expect("ordered drain");
    let retained = controls.current_query_memory_bytes();
    let values = entries
        .into_iter()
        .map(|(key, value)| (key.0, value))
        .collect::<Vec<_>>();

    // Assert
    assert_eq!(first, Some("one"));
    assert_eq!(second, Some("two"));
    assert_eq!(replacement, Some("one"));
    assert_eq!(values, vec![(1, "changed"), (2, "two")]);
    assert!(retained > 0);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_retain_a_jsonb_record_lease_until_the_last_index_owner_retires() {
    // Arrange
    let controls = QueryExecutionControls::from_limits(
        &CassieRuntimeLimits::default(),
        std::time::Instant::now(),
    );
    let key =
        crate::types::semantic::JsonbKey::from_text("1.0", Some(&controls)).expect("leased key");
    let equivalent = crate::types::semantic::JsonbKey::from_text("1.00", None).expect("lookup key");
    let mut records = KeyedMap::new(Some(&controls)).expect("index admission");

    // Act
    records
        .insert(key.clone(), 7)
        .expect("retain key in value record");
    drop(key);
    let lookup = records.get(&equivalent).copied();
    let retained = controls.current_query_memory_bytes();
    drop(records);

    // Assert
    assert_eq!(lookup, Some(7));
    assert!(retained > 0);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_reject_index_growth_before_retaining_a_partial_record() {
    // Arrange
    let limits = CassieRuntimeLimits {
        query_memory_budget_bytes: 1,
        ..CassieRuntimeLimits::default()
    };
    let controls = QueryExecutionControls::from_limits(&limits, std::time::Instant::now());
    let mut records = KeyedMap::new(Some(&controls)).expect("empty index");

    // Act
    let result = records.insert(Collision(1), "one");
    let length = records.len();
    drop(records);

    // Assert
    assert!(matches!(
        result,
        Err(crate::app::CassieError::ResourceLimit(_))
    ));
    assert_eq!(length, 0);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_preadmit_the_entire_collision_build_at_the_exact_limit() {
    // Arrange
    let required = KeyedMap::<Collision, u8>::build_storage_bytes(3).expect("build bound");
    let mut outcomes = Vec::new();

    // Act
    for budget in [required - 1, required] {
        let limits = CassieRuntimeLimits {
            query_memory_budget_bytes: budget,
            ..CassieRuntimeLimits::default()
        };
        let controls = QueryExecutionControls::from_limits(&limits, std::time::Instant::now());
        let result = KeyedMap::with_prepaid_capacity(&controls, 3).and_then(|mut records| {
            for key in 0..3 {
                records.insert(Collision(key), key)?;
            }
            assert_eq!(records.get(&Collision(2)), Some(&2));
            assert!(records.insert(Collision(3), 3).is_err());
            assert_eq!(records.insert(Collision(1), 7)?, Some(1));
            Ok(records.len())
        });
        outcomes.push((
            result,
            controls.peak_query_memory_bytes(),
            controls.current_query_memory_bytes(),
        ));
    }

    // Assert
    assert!(matches!(
        &outcomes[0].0,
        Err(crate::app::CassieError::ResourceLimit(_))
    ));
    assert_eq!(outcomes[0].1, 0);
    assert_eq!(outcomes[1].0.as_ref().expect("exact admission"), &3);
    assert_eq!(outcomes[1].1, required);
    assert!(outcomes.iter().all(|(_, _, retired)| *retired == 0));
}

#[derive(PartialEq, Eq)]
struct ProbeKey {
    value: usize,
    collide: bool,
}
impl Hash for ProbeKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        if self.collide {
            0_usize.hash(state);
        } else {
            self.value.hash(state);
        }
    }
}

#[test]
fn should_cover_observed_prepaid_collision_directory_replacement_capacity() {
    // Arrange
    use std::hash::BuildHasher;
    for count in [0_usize, 1, 2, 3, 4, 7, 8, 9, 16, 17] {
        for collide in [false, true] {
            let required =
                KeyedMap::<ProbeKey, usize>::build_storage_bytes(count).expect("build bound");
            let limits = CassieRuntimeLimits {
                query_memory_budget_bytes: required,
                ..CassieRuntimeLimits::default()
            };
            let controls = QueryExecutionControls::from_limits(&limits, std::time::Instant::now());
            let mut records =
                KeyedMap::with_prepaid_capacity(&controls, count).expect("full admission");
            let table = super::hash_table_bytes::<(u64, super::Bucket<ProbeKey, usize>)>(
                records.buckets.capacity(),
            )
            .expect("actual table capacity");

            // Act
            for value in 0..count {
                let key = ProbeKey { value, collide };
                let hash = records.hasher.hash_one(&key);
                let before = records
                    .buckets
                    .values()
                    .map(|bucket| bucket.values.capacity())
                    .sum::<usize>();
                let old = records
                    .buckets
                    .get(&hash)
                    .map_or(0, |bucket| bucket.values.capacity());
                records.insert(key, value).expect("admitted record");
                let new = records
                    .buckets
                    .get(&hash)
                    .expect("inserted fingerprint")
                    .values
                    .capacity();
                let overlap = if new > old { before + new } else { before };

                // Assert
                assert!(
                    table + overlap * size_of::<(ProbeKey, usize)>() <= required,
                    "actual live plus replacement directories remain within prepaid storage"
                );
                assert_eq!(controls.current_query_memory_bytes(), required);
            }
            assert_eq!(records.len(), count);
            drop(records);
            assert_eq!(controls.current_query_memory_bytes(), 0);
        }
    }
}
