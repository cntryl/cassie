use std::sync::Arc;

/// One raw `ttinfo` record. `designation_index` is an offset into the TZif
/// character table, so equal text at different offsets remains distinguishable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RawLocalTimeType {
    pub(crate) offset_seconds_east: i32,
    pub(crate) is_dst: bool,
    pub(crate) designation_index: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RawTransition {
    pub(crate) unix_seconds: i64,
    pub(crate) type_index: usize,
}

/// The parsed local-time types and exact character table owned by one TZif.
#[derive(Debug, Clone)]
pub(crate) struct RawTzif {
    bytes: Arc<[u8]>,
    pub(crate) local_time_types: Vec<RawLocalTimeType>,
    pub(crate) transitions: Vec<RawTransition>,
    pub(crate) default_type_index: Option<usize>,
    designations: std::ops::Range<usize>,
}

const HEADER_LEN: usize = 44;
const TYPE_LEN: usize = 6;

#[derive(Clone, Copy)]
struct Header {
    version: u8,
    gmt_count: u32,
    standard_count: u32,
    leap_count: u32,
    transition_count: u32,
    type_count: u32,
    designation_count: u32,
}

struct SectionLayout {
    transition_times_start: usize,
    transition_type_indices_start: usize,
    types_start: usize,
    end: usize,
    designations: std::ops::Range<usize>,
}

impl RawTzif {
    #[must_use]
    pub(crate) fn parse(bytes: Arc<[u8]>) -> Result<Self, &'static str> {
        let first = read_header(&bytes, 0)?;
        let (header, section_start, width) = if first.version == 0 {
            (first, HEADER_LEN, 4)
        } else {
            let first_end = section_layout(&bytes, first, HEADER_LEN, 4)?.end;
            let second = read_header(&bytes, first_end)?;
            if second.version != first.version {
                return Err("mismatched TZif versions");
            }
            (second, first_end + HEADER_LEN, 8)
        };
        let section = section_layout(&bytes, header, section_start, width)?;

        let type_count = usize::try_from(header.type_count).map_err(|_| "too many TZif types")?;
        if type_count > usize::from(u8::MAX) + 1 {
            return Err("too many TZif types");
        }
        let mut local_time_types = Vec::with_capacity(type_count);
        for index in 0..type_count {
            let start = section.types_start + index * TYPE_LEN;
            let record = bytes
                .get(start..start + TYPE_LEN)
                .ok_or("truncated TZif type table")?;
            let offset_seconds_east = i32::from_be_bytes([record[0], record[1], record[2], record[3]]);
            let is_dst = match record[4] {
                0 => false,
                1 => true,
                _ => return Err("invalid TZif DST flag"),
            };
            let designation_index = usize::from(record[5]);
            let designation_start = section
                .designations
                .start
                .checked_add(designation_index)
                .ok_or("invalid TZif designation index")?;
            if designation_start >= section.designations.end
                || !bytes[designation_start..section.designations.end].contains(&0)
            {
                return Err("unterminated TZif designation");
            }
            local_time_types.push(RawLocalTimeType {
                offset_seconds_east,
                is_dst,
                designation_index,
            });
        }
        let transition_count = usize::try_from(header.transition_count)
            .map_err(|_| "too many TZif transitions")?;
        let mut transitions = Vec::with_capacity(transition_count);
        for index in 0..transition_count {
            let time_start = section
                .transition_times_start
                .checked_add(index * width)
                .ok_or("invalid TZif transition time offset")?;
            let unix_seconds = match width {
                4 => {
                    let time = bytes
                        .get(time_start..time_start + 4)
                        .ok_or("truncated TZif transition times")?;
                    i64::from(i32::from_be_bytes([time[0], time[1], time[2], time[3]]))
                }
                8 => {
                    let time = bytes
                        .get(time_start..time_start + 8)
                        .ok_or("truncated TZif transition times")?;
                    i64::from_be_bytes([
                        time[0], time[1], time[2], time[3], time[4], time[5], time[6], time[7],
                    ])
                }
                _ => return Err("unsupported TZif timestamp width"),
            };
            let type_index = usize::from(
                *bytes
                    .get(section.transition_type_indices_start + index)
                    .ok_or("truncated TZif transition type indices")?,
            );
            if type_index >= local_time_types.len() {
                return Err("invalid TZif transition type index");
            }
            transitions.push(RawTransition {
                unix_seconds,
                type_index,
            });
        }
        let default_type_index = infer_default_type_index(&local_time_types, &transitions);

        Ok(Self {
            bytes,
            local_time_types,
            transitions,
            default_type_index,
            designations: section.designations,
        })
    }

    #[cfg(test)]
    #[must_use]
    pub(crate) fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Finds the first matching character-table index, even when its spelling
    /// appears again later in the TZif data.
    #[must_use]
    pub(crate) fn first_matching_designation_index(&self, query: &str) -> Option<usize> {
        let mut index = 0;
        while index < self.designations.len() {
            if self.designation_matches(index, query) {
                return Some(index);
            }
            index = self.next_designation_index(index)?;
        }
        None
    }

    #[must_use]
    pub(crate) fn first_matching_designation_index_with_checks<E>(
        &self,
        query: &str,
        check: &mut impl FnMut() -> Result<(), E>,
    ) -> Result<Option<usize>, E> {
        let mut index = 0;
        while index < self.designations.len() {
            if index % 128 == 127 {
                check()?;
            }
            if self.designation_matches(index, query) {
                return Ok(Some(index));
            }
            let Some(next) = self.next_designation_index(index) else {
                return Ok(None);
            };
            index = next;
        }
        Ok(None)
    }

    fn next_designation_index(&self, index: usize) -> Option<usize> {
        let start = self.designations.start.checked_add(index)?;
        let rest = self.bytes.get(start..self.designations.end)?;
        rest.iter()
            .position(|&byte| byte == 0)
            .and_then(|length| index.checked_add(length + 1))
    }

    pub(crate) fn designation_matches(&self, index: usize, query: &str) -> bool {
        let Some(start) = self.designations.start.checked_add(index) else {
            return false;
        };
        let Some(rest) = self.bytes.get(start..self.designations.end) else {
            return false;
        };
        let Some(end) = rest.iter().position(|&byte| byte == 0) else {
            return false;
        };
        let raw = &rest[..end];
        raw.len() == query.len()
            && raw
                .iter()
                .zip(query.as_bytes())
                .all(|(&left, &right)| left.eq_ignore_ascii_case(&right))
    }
}

fn read_header(bytes: &[u8], start: usize) -> Result<Header, &'static str> {
    let end = start.checked_add(HEADER_LEN).ok_or("invalid TZif header")?;
    let header = bytes.get(start..end).ok_or("truncated TZif header")?;
    if &header[..4] != b"TZif" {
        return Err("invalid TZif magic");
    }
    let version = header[4];
    if !matches!(version, 0 | b'2' | b'3' | b'4') {
        return Err("unsupported TZif version");
    }
    let count = |index: usize| {
        let offset = 20 + index * 4;
        u32::from_be_bytes([
            header[offset],
            header[offset + 1],
            header[offset + 2],
            header[offset + 3],
        ])
    };
    Ok(Header {
        version,
        gmt_count: count(0),
        standard_count: count(1),
        leap_count: count(2),
        transition_count: count(3),
        type_count: count(4),
        designation_count: count(5),
    })
}

fn section_layout(
    bytes: &[u8],
    header: Header,
    section_start: usize,
    width: usize,
) -> Result<SectionLayout, &'static str> {
    let count = |value: u32| usize::try_from(value).map_err(|_| "oversized TZif section");
    let transitions = count(header.transition_count)?;
    let types = count(header.type_count)?;
    let designations = count(header.designation_count)?;
    let leaps = count(header.leap_count)?;
    let standard = count(header.standard_count)?;
    let gmt = count(header.gmt_count)?;
    if types > usize::from(u8::MAX) + 1
        || (standard != 0 && standard != types)
        || (gmt != 0 && gmt != types)
    {
        return Err("invalid TZif counts");
    }

    let transition_bytes = transitions
        .checked_mul(width)
        .ok_or("oversized TZif transition table")?;
    let transition_type_indices_start = section_start
        .checked_add(transition_bytes)
        .ok_or("oversized TZif transition table")?;
    let transition_indices_end = transition_type_indices_start
        .checked_add(transitions)
        .ok_or("oversized TZif transition table")?;
    let types_bytes = types
        .checked_mul(TYPE_LEN)
        .ok_or("oversized TZif type table")?;
    let designations_start = transition_indices_end
        .checked_add(types_bytes)
        .ok_or("oversized TZif designation table")?;
    let designations_end = designations_start
        .checked_add(designations)
        .ok_or("oversized TZif designation table")?;
    let leap_bytes = leaps
        .checked_mul(width.checked_add(4).ok_or("oversized TZif leap table")?)
        .ok_or("oversized TZif leap table")?;
    let section_end = designations_end
        .checked_add(leap_bytes)
        .and_then(|value| value.checked_add(standard))
        .and_then(|value| value.checked_add(gmt))
        .ok_or("oversized TZif section")?;
    if section_end > bytes.len() {
        return Err("truncated TZif section");
    }
    Ok(SectionLayout {
        transition_times_start: section_start,
        transition_type_indices_start,
        types_start: transition_indices_end,
        end: section_end,
        designations: designations_start..designations_end,
    })
}

fn infer_default_type_index(
    local_time_types: &[RawLocalTimeType],
    transitions: &[RawTransition],
) -> Option<usize> {
    if local_time_types.is_empty() {
        return None;
    }
    let mut default_type = (!transitions
        .iter()
        .any(|transition| transition.type_index == 0))
    .then_some(0);
    if default_type.is_none()
        && transitions.first().is_some_and(|transition| {
            local_time_types[transition.type_index].is_dst
        })
    {
        let mut index = transitions.first()?.type_index;
        while index > 0 {
            index -= 1;
            if !local_time_types[index].is_dst {
                default_type = Some(index);
                break;
            }
        }
    }
    default_type.or_else(|| {
        local_time_types
            .iter()
            .position(|local_type| !local_type.is_dst)
            .or(Some(0))
    })
}

#[cfg(test)]
fn parse_local_time_types(bytes: Arc<[u8]>) -> Result<RawTzif, &'static str> {
    RawTzif::parse(bytes)
}

#[cfg(test)]
fn first_matching_designation_index(tzif: &RawTzif, query: &str) -> Option<usize> {
    tzif.first_matching_designation_index(query)
}

#[cfg(test)]
mod tests {
    use super::{first_matching_designation_index, parse_local_time_types};
    use std::sync::Arc;

    fn tzif(types: &[(i32, bool, u8)], designations: &[u8]) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"TZif\0");
        bytes.extend_from_slice(&[0; 15]);
        let type_count = u32::try_from(types.len()).expect("small TZif fixture");
        let designation_count = u32::try_from(designations.len()).expect("small TZif fixture");
        for count in [0_u32, 0, 0, 0, type_count, designation_count] {
            bytes.extend_from_slice(&count.to_be_bytes());
        }
        for (offset, is_dst, designation_index) in types {
            bytes.extend_from_slice(&offset.to_be_bytes());
            bytes.push(u8::from(*is_dst));
            bytes.push(*designation_index);
        }
        bytes.extend_from_slice(designations);
        bytes
    }

    #[test]
    fn should_retain_shared_raw_designation_identity_for_dynamic_types() {
        // Arrange
        let bytes = tzif(&[(0, false, 0), (3_600, true, 0)], b"AAA\0");

        // Act
        let parsed = parse_local_time_types(Arc::from(bytes)).expect("valid TZif type table");
        let selected = first_matching_designation_index(&parsed, "aaa");

        // Assert
        assert_eq!(selected, Some(0));
        assert_eq!(parsed.local_time_types.len(), 2);
        assert_eq!(
            parsed.local_time_types[0].designation_index,
            parsed.local_time_types[1].designation_index
        );
        assert_ne!(
            (
                parsed.local_time_types[0].offset_seconds_east,
                parsed.local_time_types[0].is_dst
            ),
            (
                parsed.local_time_types[1].offset_seconds_east,
                parsed.local_time_types[1].is_dst
            )
        );
    }

    #[test]
    fn should_keep_duplicate_spelling_at_distinct_raw_indices_fixed() {
        // Arrange
        let bytes = tzif(
            &[(0, false, 0), (3_600, true, 4)],
            b"AAA\0AAA\0",
        );

        // Act
        let parsed = parse_local_time_types(Arc::from(bytes)).expect("valid TZif type table");
        let selected = first_matching_designation_index(&parsed, "AAA");
        let selected_types = parsed
            .local_time_types
            .iter()
            .filter(|local_type| Some(local_type.designation_index) == selected)
            .collect::<Vec<_>>();

        // Assert
        assert_eq!(selected, Some(0));
        assert_eq!(parsed.local_time_types[0].designation_index, 0);
        assert_eq!(parsed.local_time_types[1].designation_index, 4);
        assert_eq!(selected_types.len(), 1);
        assert_eq!(selected_types[0].offset_seconds_east, 0);
    }

    #[test]
    fn should_select_the_first_matching_character_index_even_when_unused() {
        // Arrange
        let bytes = tzif(&[(3_600, true, 4)], b"AAA\0AAA\0");

        // Act
        let parsed = parse_local_time_types(Arc::from(bytes)).expect("valid TZif type table");
        let selected = first_matching_designation_index(&parsed, "AAA");

        // Assert
        assert_eq!(selected, Some(0));
        assert_eq!(parsed.local_time_types[0].designation_index, 4);
    }

    #[test]
    fn should_find_unreferenced_local_time_types() {
        // Arrange
        let bytes = tzif(&[(0, false, 0), (-21_600, false, 4)], b"UTC\0CST\0");

        // Act
        let parsed = parse_local_time_types(Arc::from(bytes)).expect("valid TZif type table");
        let selected = first_matching_designation_index(&parsed, "cst");

        // Assert
        assert_eq!(parsed.local_time_types.len(), 2);
        assert_eq!(selected, Some(4));
        assert_eq!(parsed.local_time_types[1].offset_seconds_east, -21_600);
    }

    #[test]
    fn should_reject_truncated_tzif_type_tables() {
        // Arrange
        let mut bytes = tzif(&[(0, false, 0)], b"UTC\0");
        bytes.pop();

        // Act
        let parsed = parse_local_time_types(Arc::from(bytes));

        // Assert
        assert!(parsed.is_err());
    }
}
