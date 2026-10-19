//! Small explicit-storage fixtures for constraint namespace/backing probes.
use cassie::app::Cassie;
use cassie::catalog::FieldConstraint;
use cassie::config::CassieRuntimeConfig;
use cassie::types::{DataType, FieldSchema, Schema};

pub const CANONICAL: &str = "postgres.public.namespace_probe";

pub fn unpublished_fixture() -> (Cassie, FieldConstraint) {
    prepare(Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).unwrap())
}

pub fn local_unpublished_fixture(path: &std::path::Path) -> (Cassie, FieldConstraint) {
    prepare(Cassie::new_with_data_dir_and_config(path, CassieRuntimeConfig::default()).unwrap())
}

fn prepare(cassie: Cassie) -> (Cassie, FieldConstraint) {
    cassie.midge.create_collection(CANONICAL, schema()).unwrap();
    let mut constraint = FieldConstraint::new("value");
    constraint.unique = true;
    constraint.unique_name = Some("namespace_unique".into());
    (cassie, constraint)
}

pub fn fixture(spelling: &str) -> (Cassie, FieldConstraint) {
    let (cassie, constraint) = unpublished_fixture();
    cassie
        .midge
        .save_constraints_with_unique_reservations(spelling, std::slice::from_ref(&constraint))
        .unwrap();
    (cassie, constraint)
}

pub fn schema() -> Schema {
    Schema {
        fields: vec![FieldSchema {
            name: "value".into(),
            data_type: DataType::Text,
            nullable: true,
        }],
    }
}
