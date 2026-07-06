use std::collections::BTreeMap;

use crate::embeddings::{HnswIndexOptions, IvfFlatIndexOptions, VectorIndexType};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedVectorIndexOptions {
    pub index_type: VectorIndexType,
    pub hnsw: Option<HnswIndexOptions>,
    pub ivfflat: Option<IvfFlatIndexOptions>,
}

/// Normalize SQL or REST vector index options into persisted index settings.
///
/// # Errors
///
/// Returns an error when `index_type` is unsupported or a tuning option cannot be parsed into its
/// supported range.
pub fn normalize_vector_index_options(
    options: &mut BTreeMap<String, String>,
) -> Result<NormalizedVectorIndexOptions, String> {
    let index_type = normalize_vector_index_type(options)?;
    reject_family_specific_options(options, &index_type)?;
    let hnsw = if index_type == VectorIndexType::Hnsw {
        Some(normalize_hnsw_options(options)?)
    } else {
        None
    };
    let ivfflat = if index_type == VectorIndexType::IvfFlat {
        Some(normalize_ivfflat_options(options)?)
    } else {
        None
    };
    Ok(NormalizedVectorIndexOptions {
        index_type,
        hnsw,
        ivfflat,
    })
}

pub fn vector_index_type_from_options(options: &BTreeMap<String, String>) -> VectorIndexType {
    match options
        .get("index_type")
        .map_or("bruteforce", String::as_str)
    {
        "hnsw" => VectorIndexType::Hnsw,
        "ivfflat" => VectorIndexType::IvfFlat,
        _ => VectorIndexType::BruteForce,
    }
}

fn normalize_vector_index_type(
    options: &mut BTreeMap<String, String>,
) -> Result<VectorIndexType, String> {
    let raw = options
        .get("index_type")
        .map_or("bruteforce", String::as_str)
        .trim()
        .to_ascii_lowercase();
    let index_type = match raw.as_str() {
        "bruteforce" => VectorIndexType::BruteForce,
        "hnsw" => VectorIndexType::Hnsw,
        "ivfflat" => VectorIndexType::IvfFlat,
        _ => return Err(format!("unsupported vector index_type '{raw}'")),
    };
    options.insert("index_type".to_string(), raw);
    Ok(index_type)
}

fn reject_family_specific_options(
    options: &BTreeMap<String, String>,
    index_type: &VectorIndexType,
) -> Result<(), String> {
    for key in options.keys() {
        let is_hnsw_key = matches!(key.as_str(), "m" | "ef_construction" | "ef_search");
        let is_ivfflat_key = matches!(
            key.as_str(),
            "lists" | "probes" | "training_sample_size" | "training_seed"
        );
        if is_hnsw_key && *index_type != VectorIndexType::Hnsw {
            return Err(format!(
                "vector index option '{key}' requires index_type 'hnsw'"
            ));
        }
        if is_ivfflat_key && *index_type != VectorIndexType::IvfFlat {
            return Err(format!(
                "vector index option '{key}' requires index_type 'ivfflat'"
            ));
        }
    }
    Ok(())
}

fn normalize_hnsw_options(
    options: &mut BTreeMap<String, String>,
) -> Result<HnswIndexOptions, String> {
    let m = parse_usize_option(options.get("m"), "m", 16, 2, 128)?;
    let ef_construction = parse_usize_option(
        options.get("ef_construction"),
        "ef_construction",
        64,
        m,
        4096,
    )?;
    let ef_search = parse_usize_option(options.get("ef_search"), "ef_search", 40, 1, 4096)?;
    options.insert("m".to_string(), m.to_string());
    options.insert("ef_construction".to_string(), ef_construction.to_string());
    options.insert("ef_search".to_string(), ef_search.to_string());
    Ok(HnswIndexOptions {
        version: 1,
        m,
        ef_construction,
        ef_search,
    })
}

fn normalize_ivfflat_options(
    options: &mut BTreeMap<String, String>,
) -> Result<IvfFlatIndexOptions, String> {
    let lists = parse_usize_option(options.get("lists"), "lists", 64, 1, 65_536)?;
    let probes = parse_usize_option(options.get("probes"), "probes", 1, 1, lists)?;
    let training_sample_size = parse_usize_option(
        options.get("training_sample_size"),
        "training_sample_size",
        lists.saturating_mul(40).max(1),
        lists,
        10_000_000,
    )?;
    let training_seed = parse_usize_option(
        options.get("training_seed"),
        "training_seed",
        1,
        0,
        usize::MAX,
    )?;
    options.insert("lists".to_string(), lists.to_string());
    options.insert("probes".to_string(), probes.to_string());
    options.insert(
        "training_sample_size".to_string(),
        training_sample_size.to_string(),
    );
    options.insert("training_seed".to_string(), training_seed.to_string());
    Ok(IvfFlatIndexOptions {
        version: 1,
        lists,
        probes,
        training_sample_size,
        training_seed: training_seed as u64,
    })
}

fn parse_usize_option(
    value: Option<&String>,
    key: &str,
    default: usize,
    min: usize,
    max: usize,
) -> Result<usize, String> {
    let value = value.map_or("", String::as_str).trim();
    if value.is_empty() {
        return Ok(default);
    }
    let parsed = value
        .parse::<usize>()
        .map_err(|_| format!("invalid vector index option '{key}'"))?;
    if parsed < min || parsed > max {
        return Err(format!(
            "vector index option '{key}' must be in [{min}, {max}]"
        ));
    }
    Ok(parsed)
}
