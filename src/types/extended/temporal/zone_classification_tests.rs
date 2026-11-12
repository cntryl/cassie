use super::{classify, Classification};
use crate::types::extended::temporal::zone::lookup_zone;

#[test]
fn should_classify_fixed_dynamic_and_unknown_reachable_abbreviations() {
    // Arrange
    let new_york = lookup_zone("America/New_York").expect("New York");
    let moscow = lookup_zone("Europe/Moscow").expect("Moscow");

    // Act
    let lmt = classify(&new_york, "LMT", &mut || Ok::<_, ()>(()));
    let msk = classify(&moscow, "MSK", &mut || Ok::<_, ()>(()));
    let unknown = classify(&new_york, "notazone", &mut || Ok::<_, ()>(()));

    // Assert
    assert_eq!(
        lmt,
        Ok(Some(Classification::Fixed {
            east: -17_762,
            dst: false
        }))
    );
    assert_eq!(msk, Ok(Some(Classification::Dynamic)));
    assert_eq!(unknown, Ok(None));
}

#[test]
fn should_interrupt_during_an_actual_transition_scan() {
    // Arrange
    let zone = lookup_zone("America/New_York").expect("New York");
    let mut checks = 0;

    // Act
    let result = classify(&zone, "notazone", &mut || {
        checks += 1;
        if checks == 2 {
            Err("cancelled")
        } else {
            Ok(())
        }
    });

    // Assert
    assert_eq!(result, Err("cancelled"));
    assert_eq!(checks, 2);
}

#[test]
fn should_classify_distinct_dst_flags_as_dynamic_at_the_same_offset() {
    // Arrange
    fn block(bytes: &mut Vec<u8>, time_bytes: usize) {
        bytes.extend_from_slice(b"TZif2");
        bytes.extend_from_slice(&[0; 15]);
        for count in [0_u32, 0, 0, 1, 2, 4] {
            bytes.extend_from_slice(&count.to_be_bytes());
        }
        bytes.extend(std::iter::repeat_n(0, time_bytes));
        bytes.push(1);
        bytes.extend_from_slice(&0_i32.to_be_bytes());
        bytes.extend_from_slice(&[0, 0]);
        bytes.extend_from_slice(&0_i32.to_be_bytes());
        bytes.extend_from_slice(&[1, 0]);
        bytes.extend_from_slice(b"CST\0");
    }
    let mut bytes = Vec::new();
    block(&mut bytes, 4);
    block(&mut bytes, 8);
    bytes.extend_from_slice(b"\n\n");
    let zone = crate::types::extended::temporal::zone::Zone::from_tzif("Test/SameOffset", &bytes)
        .expect("TZif");

    // Act
    let result = classify(&zone, "CST", &mut || Ok::<_, ()>(()));

    // Assert
    assert_eq!(result, Ok(Some(Classification::Dynamic)));
}

#[test]
fn should_observe_the_same_transitions_for_distinct_raw_designation_indices() {
    // Arrange
    fn data(duplicate: bool) -> Vec<u8> {
        fn block(bytes: &mut Vec<u8>, time_bytes: usize, duplicate: bool) {
            bytes.extend_from_slice(b"TZif2");
            bytes.extend_from_slice(&[0; 15]);
            for count in [0_u32, 0, 0, 1, 2, if duplicate { 8 } else { 4 }] {
                bytes.extend_from_slice(&count.to_be_bytes());
            }
            if time_bytes == 4 {
                bytes.extend_from_slice(&1_711_929_600_i32.to_be_bytes());
            } else {
                bytes.extend_from_slice(&1_711_929_600_i64.to_be_bytes());
            }
            bytes.push(1);
            bytes.extend_from_slice(&0_i32.to_be_bytes());
            bytes.extend_from_slice(&[0, 0]);
            bytes.extend_from_slice(&3_600_i32.to_be_bytes());
            bytes.extend_from_slice(&[1, if duplicate { 4 } else { 0 }]);
            bytes.extend_from_slice(if duplicate { b"AAA\0AAA\0" } else { b"AAA\0" });
        }
        let mut bytes = Vec::new();
        block(&mut bytes, 4, duplicate);
        block(&mut bytes, 8, duplicate);
        bytes.extend_from_slice(b"\n\n");
        bytes
    }
    let zone = |duplicate| {
        crate::types::extended::temporal::zone::Zone::from_tzif("Test/Paired", &data(duplicate))
            .expect("TZif")
    };
    let shared = zone(false);
    let duplicate = zone(true);

    // Act
    let first = classify(&shared, "AAA", &mut || Ok::<_, ()>(()));
    let second = classify(&duplicate, "AAA", &mut || Ok::<_, ()>(()));

    // Assert
    // PostgreSQL's raw index scan classifies the first as dynamic and the
    // second as fixed; visible Jiff transitions instead classify both alike.
    assert_eq!(first, Ok(Some(Classification::Dynamic)));
    assert_eq!(second, first);
}
