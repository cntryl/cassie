//! Test-only reachable-interval abbreviation classification prototype.
use super::Zone;

#[derive(Debug, PartialEq, Eq)]
enum Classification {
    Fixed { east: i32, dst: bool },
    Dynamic,
}

fn classify<E>(
    zone: &Zone,
    name: &str,
    check: &mut impl FnMut() -> Result<(), E>,
) -> Result<Option<Classification>, E> {
    check()?;
    let Zone::Rules { tz, .. } = zone else {
        return Ok(None);
    };
    let initial = tz.to_offset_info(jiff::Timestamp::MIN);
    let mut found = initial
        .abbreviation()
        .eq_ignore_ascii_case(name)
        .then(|| (initial.offset().seconds(), initial.dst().is_dst()));
    for (index, transition) in tz.following(jiff::Timestamp::MIN).enumerate() {
        if index % 128 == 127 {
            check()?;
        }
        if transition.abbreviation().eq_ignore_ascii_case(name) {
            let current = (transition.offset().seconds(), transition.dst().is_dst());
            match found {
                Some(first) if first != current => {
                    check()?;
                    return Ok(Some(Classification::Dynamic));
                }
                None => found = Some(current),
                Some(_) => {}
            }
        }
    }
    check()?;
    Ok(found.map(|(east, dst)| Classification::Fixed { east, dst }))
}

#[cfg(test)]
#[path = "zone_classification_tests.rs"]
mod tests;
