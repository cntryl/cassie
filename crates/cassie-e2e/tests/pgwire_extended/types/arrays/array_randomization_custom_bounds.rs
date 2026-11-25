//! Random array functions preserve PostgreSQL bounds and element bags (#807).

use super::super::support_pgwire as wire;
use super::super::support_type_metadata_contract as fixture;

fn array_bag(array_text: &str) -> Vec<Option<i32>> {
    let literal = array_text
        .split_once('=')
        .map_or(array_text, |(_, value)| value);
    let elements = literal
        .strip_prefix('{')
        .and_then(|value| value.strip_suffix('}'))
        .expect("one-dimensional array text");
    let mut bag = if elements.is_empty() {
        Vec::new()
    } else {
        elements
            .split(',')
            .map(|element| {
                if element == "NULL" {
                    None
                } else {
                    Some(element.parse().expect("integer array element"))
                }
            })
            .collect()
    };
    bag.sort_unstable();
    bag
}
