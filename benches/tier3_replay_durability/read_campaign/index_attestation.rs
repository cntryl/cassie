//! Match logical definitions while checking native identities independently.
use cassie::catalog::IndexMeta;
use serde::Serialize;

const RELATION_ID: &str = "__cassie_relation_id";
const STORAGE_ID: &str = "__cassie_storage_id";

#[derive(Debug, Serialize, PartialEq, Eq)]
pub(crate) struct NativeIdentity {
    pub(crate) collection: String,
    pub(crate) name: String,
    pub(crate) relation_id: u64,
    pub(crate) storage_id: u64,
}

fn positive_identity(index: &IndexMeta, key: &str) -> Result<u64, String> {
    index
        .options
        .get(key)
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value != 0)
        .ok_or_else(|| {
            format!(
                "missing or invalid native identity {key} for {}",
                index.name
            )
        })
}

pub(crate) fn verify_definitions(
    catalog: &[IndexMeta],
    persisted: &[IndexMeta],
    catalog_relation: u64,
    native_relation: u64,
) -> Result<Vec<NativeIdentity>, String> {
    if catalog_relation == 0 || native_relation == 0 || catalog_relation != native_relation {
        return Err("catalog/native collection identities must match and be nonzero".into());
    }
    if catalog.len() != persisted.len() {
        return Err("catalog/persisted index definition count differs".into());
    }
    let mut observed = Vec::with_capacity(persisted.len());
    for (logical, native) in catalog.iter().zip(persisted) {
        let relation_id = positive_identity(native, RELATION_ID)?;
        let storage_id = positive_identity(native, STORAGE_ID)?;
        if relation_id != catalog_relation || relation_id != native_relation {
            return Err(format!(
                "native index relation identity does not match collection: {}",
                native.name
            ));
        }
        // Hydration removes precisely these two options. Every other field/option
        // (including catalog OID and access method) remains in full equality.
        let mut normalized = native.clone();
        normalized.options.remove(RELATION_ID);
        normalized.options.remove(STORAGE_ID);
        if *logical != normalized {
            return Err(format!(
                "catalog/persisted index definition differs: {}",
                native.name
            ));
        }
        observed.push(NativeIdentity {
            collection: native.collection.clone(),
            name: native.name.clone(),
            relation_id,
            storage_id,
        });
    }
    Ok(observed)
}
