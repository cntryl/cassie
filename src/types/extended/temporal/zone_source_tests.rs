use super::{RuleSource, Zone};

#[test]
fn should_interrupt_a_long_formatted_timezone_abbreviation_search() {
    // Arrange
    let zone = Zone::from_tzif("Test/Long", &long_dynamic_tzif()).expect("valid TZif");
    let mut checks = 0;
    let mut check = || {
        checks += 1;
        if checks == 2 {
            Err("canceled")
        } else {
            Ok(())
        }
    };

    // Act
    let result = zone.abbreviation_offset_with_checks("AAA", 0, &mut check);

    // Assert
    assert_eq!(result, Err("canceled"));
    assert_eq!(checks, 2);
}

#[test]
fn should_build_bundled_iana_zone_from_its_retained_tzif_bytes() {
    // Arrange
    let expected_name = "America/New_York";
    let expected_bytes = jiff_tzdb::get("america/new_york")
        .expect("bundled source")
        .1;

    // Act
    let zone = Zone::from_bundled_iana("america/new_york").expect("bundled zone");

    // Assert
    assert_eq!(zone.name(), expected_name);
    let Zone::Rules { tz, .. } = zone else {
        panic!("bundled IANA zones use transition rules");
    };
    let RuleSource::Tzif(source) = &tz.source else {
        panic!("bundled IANA zones retain TZif source bytes");
    };
    assert_eq!(source.bytes(), expected_bytes);
    assert_eq!(
        tz.time_zone
            .to_offset_info(jiff::Timestamp::UNIX_EPOCH)
            .abbreviation(),
        "EST"
    );
}

#[test]
fn should_resolve_case_insensitive_bundled_iana_aliases() {
    // Arrange
    let canonical = jiff_tzdb::get("Africa/Asmara").expect("bundled canonical zone");
    let alias = jiff_tzdb::get("africa/asmera").expect("bundled case-insensitive alias");

    // Act
    let zone = Zone::from_bundled_iana("africa/asmera").expect("bundled alias zone");

    // Assert
    assert_eq!(alias.0, "Africa/Asmera");
    assert_eq!(canonical.1, alias.1);
    assert_eq!(zone.name(), "Africa/Asmera");
    let Zone::Rules { tz, .. } = zone else {
        panic!("bundled aliases use transition rules");
    };
    let RuleSource::Tzif(source) = &tz.source else {
        panic!("bundled aliases retain the matched raw TZif source");
    };
    assert_eq!(source.bytes(), alias.1);
}

#[test]
fn should_keep_supplied_tzif_bytes_coupled_to_the_jiff_zone() {
    // Arrange
    let bytes = single_type_tzif(0, b"AAA\0");
    let expected_bytes = bytes.clone();

    // Act
    let zone = Zone::from_tzif("Test/AAA", &bytes).expect("valid TZif");

    // Assert
    let Zone::Rules { tz, .. } = zone else {
        panic!("TZif zones use transition rules");
    };
    let RuleSource::Tzif(source) = &tz.source else {
        panic!("TZif constructor retains its raw source");
    };
    assert_eq!(source.bytes(), expected_bytes.as_slice());
    let info = tz.time_zone.to_offset_info(jiff::Timestamp::UNIX_EPOCH);
    assert_eq!((info.offset().seconds(), info.abbreviation()), (0, "AAA"));
}

#[test]
fn should_resolve_an_orphan_raw_tzif_abbreviation() {
    // Arrange
    let bytes = raw_types_tzif(&[(0, false, 0), (-21_600, false, 4)], b"UTC\0CST\0");
    let zone = Zone::from_tzif("Test/Orphan", &bytes).expect("valid TZif");

    // Act
    let result = zone.abbreviation_offset("CST", 0);
    let mut check = || Ok::<(), String>(());
    let classification = zone.abbreviation_is_dynamic_with_checks("CST", &mut check);

    // Assert
    assert_eq!(result, Some((-21_600, false)));
    assert_eq!(classification, Ok(Some(false)));
}

#[test]
fn should_select_the_first_duplicate_tzif_designation_index() {
    // Arrange
    let bytes = raw_types_tzif(&[(0, false, 0), (3_600, true, 4)], b"AAA\0AAA\0");
    let zone = Zone::from_tzif("Test/Duplicate", &bytes).expect("valid TZif");

    // Act
    let result = zone.abbreviation_offset("AAA", 0);

    // Assert
    assert_eq!(result, Some((0, false)));
}

#[test]
fn should_classify_shared_raw_designations_as_dynamic() {
    // Arrange
    let bytes = raw_types_tzif(&[(0, false, 0), (3_600, true, 0)], b"AAA\0");
    let zone = Zone::from_tzif("Test/Dynamic", &bytes).expect("valid TZif");
    let mut check = || Ok::<(), String>(());

    // Act
    let result = zone.abbreviation_is_dynamic_with_checks("AAA", &mut check);

    // Assert
    assert_eq!(result, Ok(Some(true)));
}

#[test]
fn should_resolve_dynamic_types_by_the_exact_raw_transition_index() {
    // Arrange
    let bytes = transitions_tzif(
        &[(0, false, 0), (3_600, true, 0), (3_600, true, 4)],
        &[(0, 0), (10, 2), (20, 1)],
        b"AAA\0AAA\0",
    );
    let zone = Zone::from_tzif("Test/TransitionIndex", &bytes).expect("valid TZif");
    let at_unix_second_15 =
        (15 - crate::types::extended::temporal::calendar::UNIX_TO_POSTGRES_SECS) * 1_000_000;

    // Act
    let result = zone.abbreviation_offset("AAA", at_unix_second_15);

    // Assert
    assert_eq!(result, Some((0, false)));
}

#[test]
fn should_classify_duplicate_spelling_at_distinct_raw_indices_as_fixed() {
    // Arrange
    let bytes = raw_types_tzif(&[(0, false, 0), (3_600, true, 4)], b"AAA\0AAA\0");
    let zone = Zone::from_tzif("Test/Duplicate", &bytes).expect("valid TZif");
    let mut check = || Ok::<(), String>(());

    // Act
    let result = zone.abbreviation_is_dynamic_with_checks("AAA", &mut check);

    // Assert
    assert_eq!(result, Ok(Some(false)));
}

#[test]
fn should_ignore_a_later_type_when_the_first_matching_index_is_unreferenced() {
    // Arrange
    let bytes = raw_types_tzif(&[(3_600, true, 4)], b"AAA\0AAA\0");
    let zone = Zone::from_tzif("Test/UnreferencedFirst", &bytes).expect("valid TZif");
    let mut check = || Ok::<(), String>(());

    // Act
    let result = zone.abbreviation_is_dynamic_with_checks("AAA", &mut check);

    // Assert
    assert_eq!(result, Ok(None));
}

#[test]
fn should_keep_posix_rule_text_and_explicit_abbreviations() {
    // Arrange
    let rule = "AAA0BBB-1,M3.2.0,M11.1.0";

    // Act
    let zone = Zone::from_posix(rule).expect("valid POSIX rule");

    // Assert
    assert_eq!(zone.name(), rule);
    assert_eq!(zone.abbreviation_offset("AAA", 0), Some((0, false)));
    assert_eq!(zone.abbreviation_offset("BBB", 0), Some((3_600, true)));
    let Zone::Rules { tz, .. } = zone else {
        panic!("POSIX zones use transition rules");
    };
    let RuleSource::Posix(source) = &tz.source else {
        panic!("POSIX constructor retains its rule source");
    };
    assert_eq!(source.rule, rule);
    assert_eq!(
        (
            source.standard.abbreviation.as_str(),
            source.standard.offset_seconds_east
        ),
        ("AAA", 0)
    );
    let daylight = source.daylight.as_ref().expect("daylight token");
    assert_eq!(
        (daylight.abbreviation.as_str(), daylight.offset_seconds_east),
        ("BBB", 3_600)
    );
}

#[test]
fn should_interrupt_long_posix_abbreviation_classification_comparison() {
    // Arrange
    let abbreviation = "A".repeat(200);
    let rule = format!("<{abbreviation}>0");
    let zone = Zone::from_posix(&rule).expect("valid POSIX rule");
    let mut checks = 0;
    let mut check = || {
        checks += 1;
        if checks == 2 {
            Err("canceled")
        } else {
            Ok(())
        }
    };

    // Act
    let result = zone.abbreviation_is_dynamic_with_checks(&abbreviation, &mut check);

    // Assert
    assert_eq!(result, Err("canceled"));
    assert_eq!(checks, 2);
}

#[test]
fn should_interrupt_long_posix_abbreviation_offset_comparison() {
    // Arrange
    let abbreviation = "A".repeat(200);
    let rule = format!("<{abbreviation}>0");
    let zone = Zone::from_posix(&rule).expect("valid POSIX rule");
    let mut checks = 0;
    let mut check = || {
        checks += 1;
        if checks == 2 {
            Err("canceled")
        } else {
            Ok(())
        }
    };

    // Act
    let result = zone.abbreviation_offset_with_checks(&abbreviation, 0, &mut check);

    // Assert
    assert_eq!(result, Err("canceled"));
    assert_eq!(checks, 2);
}

fn single_type_tzif(offset: i32, designations: &[u8]) -> Vec<u8> {
    raw_types_tzif(&[(offset, false, 0)], designations)
}

fn raw_types_tzif(types: &[(i32, bool, u8)], designations: &[u8]) -> Vec<u8> {
    transitions_tzif(types, &[], designations)
}

fn transitions_tzif(
    types: &[(i32, bool, u8)],
    transitions: &[(i32, u8)],
    designations: &[u8],
) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"TZif\0");
    bytes.extend_from_slice(&[0; 15]);
    let type_count = u32::try_from(types.len()).expect("small TZif fixture");
    let transition_count = u32::try_from(transitions.len()).expect("small TZif fixture");
    let designation_count = u32::try_from(designations.len()).expect("small TZif fixture");
    for count in [0_u32, 0, 0, transition_count, type_count, designation_count] {
        bytes.extend_from_slice(&count.to_be_bytes());
    }
    for (time, _) in transitions {
        bytes.extend_from_slice(&time.to_be_bytes());
    }
    for (_, type_index) in transitions {
        bytes.push(*type_index);
    }
    for (offset, is_dst, designation_index) in types {
        bytes.extend_from_slice(&offset.to_be_bytes());
        bytes.push(u8::from(*is_dst));
        bytes.push(*designation_index);
    }
    bytes.extend_from_slice(designations);
    bytes
}

fn long_dynamic_tzif() -> Vec<u8> {
    let transition_count = 300_u32;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"TZif\0");
    bytes.extend_from_slice(&[0; 15]);
    for count in [0_u32, 0, 0, transition_count, 3, 8] {
        bytes.extend_from_slice(&count.to_be_bytes());
    }
    for transition in 0..transition_count {
        bytes.extend_from_slice(
            &i32::try_from(transition)
                .expect("transition fits")
                .to_be_bytes(),
        );
    }
    for transition in 0..transition_count {
        bytes.push(if transition + 1 == transition_count {
            2
        } else {
            u8::try_from(transition % 2).expect("type index fits")
        });
    }
    bytes.extend_from_slice(&0_i32.to_be_bytes());
    bytes.extend_from_slice(&[0, 0]);
    bytes.extend_from_slice(&3_600_i32.to_be_bytes());
    bytes.extend_from_slice(&[1, 0]);
    bytes.extend_from_slice(&7_200_i32.to_be_bytes());
    bytes.extend_from_slice(&[0, 4]);
    bytes.extend_from_slice(b"AAA\0BBB\0");
    bytes
}
