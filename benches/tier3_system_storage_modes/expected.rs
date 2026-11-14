//! Independent relational expectations for the deterministic 100k fixture.
use cassie::types::Value;
use std::collections::BTreeMap;
use std::hash::{DefaultHasher, Hash, Hasher};

pub fn fingerprint(rows: &[Vec<Value>]) -> (usize, u64, u64) {
    let mut xor = 0;
    let mut sum = 0_u64;
    for row in rows {
        let mut hash = DefaultHasher::new();
        for value in row {
            match value {
                Value::Int64(value) => value.hash(&mut hash),
                Value::Float64(value) => value.to_bits().hash(&mut hash),
                Value::String(value) => value.hash(&mut hash),
                Value::Bool(value) => value.hash(&mut hash),
                Value::Null => 0xff_u8.hash(&mut hash),
                other => panic!("unexpected benchmark result type: {other:?}"),
            }
        }
        let value = hash.finish();
        xor ^= value;
        sum = sum.wrapping_add(value);
    }
    (rows.len(), xor, sum)
}

pub fn rows(workload: &str, scale: usize) -> Vec<Vec<Value>> {
    if workload.contains("join") {
        return join(scale);
    }
    if workload.contains("aggregate") {
        return aggregate(scale);
    }
    (0..scale)
        .map(|id| {
            let qty = i64::try_from((id * 7) % 1000).expect("qty");
            let group = i64::try_from(id % 100).expect("group");
            if workload.contains("narrow") {
                return vec![Value::Int64(group), Value::Int64(qty)];
            }
            vec![
                Value::Int64(i64::try_from(id).expect("id")),
                Value::Int64(group),
                Value::String(format!("cat{}", id % 8)),
                if id.is_multiple_of(7) {
                    Value::Null
                } else {
                    Value::Float64(f64::from(u32::try_from(id % 1000).expect("amount")) / 4.0)
                },
                Value::Int64(qty),
                Value::Bool(id.is_multiple_of(3)),
                if id.is_multiple_of(5) {
                    Value::Null
                } else {
                    Value::String(format!("note-{id}"))
                },
                Value::Int64(i64::try_from(id % 1000).expect("dimension id")),
            ]
        })
        .collect()
}

fn aggregate(scale: usize) -> Vec<Vec<Value>> {
    let mut groups = BTreeMap::<usize, (i64, i64, f64)>::new();
    for id in 0..scale {
        if id % 100 >= 50 {
            continue;
        }
        let entry = groups.entry(id % 8).or_default();
        entry.0 += 1;
        entry.1 += i64::try_from((id * 7) % 1000).expect("qty");
        if !id.is_multiple_of(7) {
            entry.2 += f64::from(u32::try_from(id % 1000).expect("amount")) / 4.0;
        }
    }
    groups
        .into_iter()
        .map(|(category, (count, qty, amount))| {
            vec![
                Value::String(format!("cat{category}")),
                Value::Int64(count),
                Value::Int64(qty),
                Value::Float64(amount),
            ]
        })
        .collect()
}

fn join(scale: usize) -> Vec<Vec<Value>> {
    let mut regions = BTreeMap::<usize, (i64, i64, i64)>::new();
    for id in 0..scale {
        if id % 100 >= 50 {
            continue;
        }
        let entry = regions.entry((id % 1000) % 10).or_default();
        entry.0 += 1;
        entry.1 += i64::try_from((id * 7) % 1000).expect("qty");
        entry.2 += i64::try_from((id % 1000) % 50).expect("weight");
    }
    regions
        .into_iter()
        .map(|(region, (count, qty, weight))| {
            vec![
                Value::String(format!("region{region}")),
                Value::Int64(count),
                Value::Int64(qty),
                Value::Int64(weight),
            ]
        })
        .collect()
}
