#![allow(unused_imports, dead_code)]
use cassie::app::CassieError;
use cassie::catalog::{Catalog, IndexKind, IndexMeta};
use cassie::planner::{logical, optimizer, physical, physical::Operator};
use cassie::sql::ast::{
    BinaryOp, Expr, InsertSource, JoinKind, ParsedStatement, QuerySource, QueryStatement,
    SelectItem, SelectStatement, SortDirection,
};
use cassie::sql::binder::BoundStatement;
use cassie::sql::{binder, parser};
use cassie::types::{DataType, FieldSchema};
use std::collections::BTreeMap;

fn register_test_collection(catalog: &Catalog, name: &str) {
    let schema = vec![
        FieldSchema {
            name: "title".to_string(),
            data_type: DataType::Text,
            nullable: true,
        },
        FieldSchema {
            name: "body".to_string(),
            data_type: DataType::Text,
            nullable: true,
        },
    ];

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        catalog.register_collection(
            name,
            schema
                .into_iter()
                .map(|field| (field.name, field.data_type))
                .collect(),
        );
    });
}

fn register_scalar_index(catalog: &Catalog, collection: &str, name: &str, fields: Vec<&str>) {
    let fields = fields
        .into_iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    catalog.register_index(IndexMeta {
        collection: collection.to_string(),
        name: name.to_string(),
        field: fields.first().cloned().unwrap_or_default(),
        fields,
        expressions: Vec::new(),
        include_fields: Vec::new(),
        predicate: None,
        kind: IndexKind::Scalar,
        unique: false,
        options: BTreeMap::new(),
    });
}

#[test]
fn should_plan_create_table_as_command() {
    // Arrange
    let catalog = Catalog::new();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        // Act
        let parsed =
            parser::parse_statement("CREATE TABLE planner_create (id INT, title TEXT, body TEXT)")
                .unwrap();
        let bound = binder::bind(parsed, &catalog).unwrap();
        let plan = logical::plan(&bound).unwrap();

        // Assert
        assert_eq!(plan.collection, "planner_create");
        assert!(plan.command.is_some());
        assert!(plan.projection.is_empty());
        assert!(plan.filter.is_none());
    });
}

#[test]
fn should_plan_drop_table_as_command() {
    // Arrange
    let catalog = Catalog::new();
    register_test_collection(&catalog, "planner_drop");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        // Act
        let parsed = parser::parse_statement("DROP TABLE planner_drop").unwrap();
        let bound = binder::bind(parsed, &catalog).unwrap();
        let plan = logical::plan(&bound).unwrap();

        // Assert
        assert_eq!(plan.collection, "planner_drop");
        assert!(plan.command.is_some());
    });
}

#[test]
fn should_plan_insert_values_as_command() {
    // Arrange
    let catalog = Catalog::new();
    register_test_collection(&catalog, "planner_insert_values");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        // Act
        let parsed =
            parser::parse_statement("INSERT INTO planner_insert_values (title) VALUES ('alpha')")
                .unwrap();
        let bound = binder::bind(parsed, &catalog).unwrap();
        let plan = logical::plan(&bound).unwrap();

        // Assert
        assert_eq!(plan.collection, "planner_insert_values");
        match plan.command.as_ref().expect("insert command") {
            logical::LogicalCommand::Insert(statement) => {
                assert_eq!(statement.table, "planner_insert_values");
                assert_eq!(statement.columns, vec!["title".to_string()]);
                assert!(matches!(statement.source, InsertSource::Values(_)));
            }
            _ => panic!("expected insert command"),
        }
    });
}

#[test]
fn should_plan_insert_select_as_command() {
    // Arrange
    let catalog = Catalog::new();
    register_test_collection(&catalog, "planner_insert_select_target");
    register_test_collection(&catalog, "planner_insert_select_source");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        // Act
        let parsed = parser::parse_statement(
            "INSERT INTO planner_insert_select_target (title) SELECT title FROM planner_insert_select_source",
        )
        .unwrap();
        let bound = binder::bind(parsed, &catalog).unwrap();
        let plan = logical::plan(&bound).unwrap();

        // Assert
        assert_eq!(plan.collection, "planner_insert_select_target");
        match plan.command.as_ref().expect("insert command") {
            logical::LogicalCommand::Insert(statement) => {
                assert_eq!(statement.table, "planner_insert_select_target");
                assert_eq!(statement.columns, vec!["title".to_string()]);
                assert!(matches!(statement.source, InsertSource::Select(_)));
            }
            _ => panic!("expected insert command"),
        }
    });
}

#[test]
fn should_plan_update_as_command() {
    // Arrange
    let catalog = Catalog::new();
    register_test_collection(&catalog, "planner_update");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        // Act
        let parsed = parser::parse_statement(
            "UPDATE planner_update SET title = 'alpha' WHERE body = 'old' RETURNING title",
        )
        .unwrap();
        let bound = binder::bind(parsed, &catalog).unwrap();
        let plan = logical::plan(&bound).unwrap();

        // Assert
        assert_eq!(plan.collection, "planner_update");
        match plan.command.as_ref().expect("update command") {
            logical::LogicalCommand::Update(statement) => {
                assert_eq!(statement.table, "planner_update");
                assert_eq!(statement.assignments.len(), 1);
                assert!(statement.filter.is_some());
                assert_eq!(statement.returning.len(), 1);
            }
            _ => panic!("expected update command"),
        }
    });
}

#[test]
fn should_plan_delete_as_command() {
    // Arrange
    let catalog = Catalog::new();
    register_test_collection(&catalog, "planner_delete");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        // Act
        let parsed = parser::parse_statement(
            "DELETE FROM planner_delete WHERE title = 'alpha' RETURNING title",
        )
        .unwrap();
        let bound = binder::bind(parsed, &catalog).unwrap();
        let plan = logical::plan(&bound).unwrap();

        // Assert
        assert_eq!(plan.collection, "planner_delete");
        match plan.command.as_ref().expect("delete command") {
            logical::LogicalCommand::Delete(statement) => {
                assert_eq!(statement.table, "planner_delete");
                assert!(statement.filter.is_some());
                assert_eq!(statement.returning.len(), 1);
            }
            _ => panic!("expected delete command"),
        }
    });
}

#[test]
fn should_plan_alter_table_command() {
    // Arrange
    let catalog = Catalog::new();
    register_test_collection(&catalog, "planner_alter");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        // Act
        let parsed =
            parser::parse_statement("ALTER TABLE planner_alter ADD COLUMN score FLOAT").unwrap();
        let bound = binder::bind(parsed, &catalog).unwrap();
        let plan = logical::plan(&bound).unwrap();

        // Assert
        assert_eq!(plan.collection, "planner_alter");
        assert!(plan.command.is_some());
        match plan.command.as_ref().unwrap() {
            logical::LogicalCommand::AlterTable(statement) => {
                assert_eq!(statement.table, "planner_alter");
            }
            _ => panic!("expected alter table command"),
        }
    });
}

#[test]
fn should_plan_create_index_as_command() {
    // Arrange
    let catalog = Catalog::new();
    register_test_collection(&catalog, "planner_create_index");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        // Act
        let parsed = parser::parse_statement(
            "CREATE UNIQUE INDEX planner_idx_title ON planner_create_index USING btree (title)",
        )
        .unwrap();
        let bound = binder::bind(parsed, &catalog).unwrap();
        let plan = logical::plan(&bound).unwrap();

        // Assert
        assert_eq!(plan.collection, "planner_create_index");
        assert!(plan.command.is_some());
        match plan.command.as_ref().unwrap() {
            logical::LogicalCommand::CreateIndex(statement) => {
                assert_eq!(statement.name, "planner_idx_title");
                assert!(statement.unique);
            }
            _ => panic!("expected create index command"),
        }
    });
}

#[test]
fn should_plan_drop_index_as_command() {
    // Arrange
    let catalog = Catalog::new();
    register_test_collection(&catalog, "planner_drop_index");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        catalog.register_index(cassie::catalog::IndexMeta {
            collection: "planner_drop_index".to_string(),
            name: "planner_idx_title".to_string(),
            field: "title".to_string(),
            fields: vec!["title".to_string()],
            expressions: Vec::new(),
            include_fields: Vec::new(),
            predicate: None,
            kind: cassie::catalog::IndexKind::Scalar,
            unique: false,
            options: std::collections::BTreeMap::new(),
        });

        // Act
        let parsed =
            parser::parse_statement("DROP INDEX planner_idx_title ON planner_drop_index").unwrap();
        let bound = binder::bind(parsed, &catalog).unwrap();
        let plan = logical::plan(&bound).unwrap();

        // Assert
        assert_eq!(plan.collection, "planner_drop_index");
        assert!(plan.command.is_some());
        match plan.command.as_ref().unwrap() {
            logical::LogicalCommand::DropIndex(statement) => {
                assert_eq!(statement.name, "planner_idx_title");
                assert_eq!(statement.table, "planner_drop_index");
            }
            _ => panic!("expected drop index command"),
        }
    });
}
