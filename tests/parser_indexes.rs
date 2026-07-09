#![allow(unused_imports)]

use cassie::app::Cassie;
use cassie::catalog::{IndexKind, IndexMeta};
use cassie::sql::ast::{
    BinaryOp, CteQuery, Expr, InsertSource, JoinKind, QuerySource, QueryStatement, SelectItem,
    SetOperator, SortDirection,
};
use cassie::sql::parse_statement;
use cassie::types::{DataType, FieldSchema, Schema};
use std::collections::BTreeMap;
use uuid::Uuid;

#[test]
fn should_parse_create_index_statement() {
    // Arrange
    let sql = "CREATE UNIQUE INDEX idx_users_email ON users USING btree (email) WITH (fillfactor = 90, case_sensitive = 'false')";

    // Act
    let parsed = parse_statement(sql).expect("parse should succeed");

    // Assert
    let QueryStatement::CreateIndex(statement) = parsed.statement else {
        panic!("expected create index statement");
    };

    assert_eq!(statement.name, "idx_users_email");
    assert_eq!(statement.table, "users");
    assert_eq!(statement.fields, vec!["email".to_string()]);
    assert!(statement.unique);
    assert!(matches!(statement.kind, cassie::catalog::IndexKind::Scalar));
    assert_eq!(statement.options.get("fillfactor"), Some(&"90".to_string()));
    assert_eq!(
        statement.options.get("case_sensitive"),
        Some(&"false".to_string())
    );
}

#[test]
fn should_parse_composite_create_index_statement() {
    // Arrange
    let sql = "CREATE INDEX idx_docs_title_score ON docs USING btree (title, score)";

    // Act
    let parsed = parse_statement(sql).expect("parse should succeed");

    // Assert
    let QueryStatement::CreateIndex(statement) = parsed.statement else {
        panic!("expected create index statement");
    };

    assert_eq!(statement.name, "idx_docs_title_score");
    assert_eq!(statement.table, "docs");
    assert_eq!(
        statement.fields,
        vec!["title".to_string(), "score".to_string()]
    );
    assert!(!statement.unique);
    assert!(matches!(statement.kind, IndexKind::Scalar));
}

#[test]
fn should_parse_create_index_include_columns() {
    // Arrange
    let sql = "CREATE INDEX idx_docs_title_include ON docs USING btree (title) INCLUDE (body, score) WITH (fillfactor = 90)";

    // Act
    let parsed = parse_statement(sql).expect("parse should succeed");

    // Assert
    let QueryStatement::CreateIndex(statement) = parsed.statement else {
        panic!("expected create index statement");
    };
    assert_eq!(statement.name, "idx_docs_title_include");
    assert_eq!(statement.fields, vec!["title".to_string()]);
    assert_eq!(
        statement.include_fields,
        vec!["body".to_string(), "score".to_string()]
    );
    assert_eq!(statement.options.get("fillfactor"), Some(&"90".to_string()));
}

#[test]
fn should_parse_create_partial_index_statement() {
    // Arrange
    let sql = "CREATE INDEX idx_docs_active ON docs USING btree (title) WHERE status = 'active'";

    // Act
    let parsed = parse_statement(sql).expect("parse should succeed");

    // Assert
    let QueryStatement::CreateIndex(statement) = parsed.statement else {
        panic!("expected create index statement");
    };
    assert_eq!(statement.name, "idx_docs_active");
    assert_eq!(statement.fields, vec!["title".to_string()]);
    assert!(statement.predicate.is_some());
}

#[test]
fn should_parse_create_expression_index_statement() {
    // Arrange
    let sql = "CREATE INDEX idx_docs_lower_title ON docs USING btree (lower(title))";

    // Act
    let parsed = parse_statement(sql).expect("parse should succeed");

    // Assert
    let QueryStatement::CreateIndex(statement) = parsed.statement else {
        panic!("expected create index statement");
    };
    assert_eq!(statement.name, "idx_docs_lower_title");
    assert_eq!(statement.table, "docs");
    assert!(statement.fields.is_empty());
    assert_eq!(statement.expressions.len(), 1);
}

#[test]
fn should_reject_non_scalar_expression_index() {
    // Arrange
    let sql = "CREATE INDEX idx_docs_lower_title ON docs USING fulltext (lower(title))";

    // Act
    let err = parse_statement(sql).expect_err("parse should reject expression fulltext index");

    // Assert
    assert_eq!(err.kind(), cassie::sql::SqlErrorKind::Unsupported);
    assert!(err
        .message()
        .contains("expression indexes are only supported for scalar index methods"));
}

#[test]
fn should_reject_non_immutable_function_expression_index() {
    // Arrange
    let cassie = Cassie::new_with_data_dir(format!(
        "/tmp/cassie-expression-index-volatile-{}",
        Uuid::new_v4()
    ))
    .unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        cassie.register_collection(
            "expression_index_volatile_docs".to_string(),
            Schema {
                fields: vec![FieldSchema {
                    name: "title".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                }],
            },
        );
        let parsed = parse_statement(
            "CREATE INDEX idx_expression_volatile ON expression_index_volatile_docs USING btree (current_user())",
        )
        .expect("parse should succeed");

        // Act
        let err = cassie::sql::binder::bind(parsed, &cassie.catalog)
            .expect_err("bind should reject volatile function");

        // Assert
        assert!(err
            .to_string()
            .contains("function 'current_user' is not immutable for index expressions"));
    });
}

#[test]
fn should_reject_duplicate_include_columns() {
    // Arrange
    let cassie =
        Cassie::new_with_data_dir(format!("/tmp/cassie-include-duplicate-{}", Uuid::new_v4()))
            .unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        cassie.register_collection(
            "include_duplicate_docs".to_string(),
            Schema {
                fields: vec![
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
                ],
            },
        );

        // Act
        let parsed = parse_statement(
            "CREATE INDEX idx_include_duplicate ON include_duplicate_docs USING btree (title) INCLUDE (body, body)",
        )
        .expect("parse should succeed");
        let bound = cassie::sql::binder::bind(parsed, &cassie.catalog);

        // Assert
        assert!(bound.is_err());
    });
}

#[test]
fn should_reject_include_key_overlap() {
    // Arrange
    let cassie =
        Cassie::new_with_data_dir(format!("/tmp/cassie-include-overlap-{}", Uuid::new_v4()))
            .unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        cassie.register_collection(
            "include_overlap_docs".to_string(),
            Schema {
                fields: vec![FieldSchema {
                    name: "title".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                }],
            },
        );

        // Act
        let parsed = parse_statement(
            "CREATE INDEX idx_include_overlap ON include_overlap_docs USING btree (title) INCLUDE (title)",
        )
        .expect("parse should succeed");
        let bound = cassie::sql::binder::bind(parsed, &cassie.catalog);

        // Assert
        assert!(bound.is_err());
    });
}

#[test]
fn should_reject_unknown_include_column() {
    // Arrange
    let cassie =
        Cassie::new_with_data_dir(format!("/tmp/cassie-include-unknown-{}", Uuid::new_v4()))
            .unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        cassie.register_collection(
            "include_unknown_docs".to_string(),
            Schema {
                fields: vec![FieldSchema {
                    name: "title".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                }],
            },
        );

        // Act
        let parsed = parse_statement(
            "CREATE INDEX idx_include_unknown ON include_unknown_docs USING btree (title) INCLUDE (body)",
        )
        .expect("parse should succeed");
        let bound = cassie::sql::binder::bind(parsed, &cassie.catalog);

        // Assert
        assert!(bound.is_err());
    });
}

#[test]
fn should_reject_fulltext_include_columns() {
    // Arrange
    let cassie =
        Cassie::new_with_data_dir(format!("/tmp/cassie-include-fulltext-{}", Uuid::new_v4()))
            .unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        cassie.register_collection(
            "include_fulltext_docs".to_string(),
            Schema {
                fields: vec![
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
                ],
            },
        );

        // Act
        let parsed = parse_statement(
            "CREATE INDEX idx_include_fulltext ON include_fulltext_docs USING fulltext (body) INCLUDE (title)",
        )
        .expect("parse should succeed");
        let bound = cassie::sql::binder::bind(parsed, &cassie.catalog);

        // Assert
        assert!(bound.is_err());
    });
}

#[test]
fn should_reject_composite_vector_create_index_statement() {
    // Arrange
    let sql = "CREATE INDEX idx_docs_embedding ON docs USING vector (embedding, source)";

    // Act
    let parsed = parse_statement(sql);

    // Assert
    assert!(parsed.is_err());
}

#[test]
fn should_reject_composite_fulltext_create_index_statement() {
    // Arrange
    let sql = "CREATE INDEX idx_docs_body_title ON docs USING fulltext (body, title)";

    // Act
    let parsed = parse_statement(sql);

    // Assert
    assert!(parsed.is_err());
}

#[test]
fn should_parse_vector_create_index_statement() {
    // Arrange
    let sql = "CREATE INDEX idx_docs_embedding ON docs USING vector (embedding) WITH (source_field = content, metric = 'l2')";

    // Act
    let parsed = parse_statement(sql).expect("parse should succeed");

    // Assert
    let QueryStatement::CreateIndex(statement) = parsed.statement else {
        panic!("expected create index statement");
    };

    assert_eq!(statement.name, "idx_docs_embedding");
    assert_eq!(statement.table, "docs");
    assert_eq!(statement.fields, vec!["embedding".to_string()]);
    assert!(!statement.unique);
    assert!(matches!(statement.kind, IndexKind::Vector));
    assert_eq!(
        statement.options.get("source_field"),
        Some(&"content".to_string())
    );
    assert_eq!(statement.options.get("metric"), Some(&"l2".to_string()));
}

#[test]
fn should_parse_fulltext_create_index_statement_with_options() {
    // Arrange
    let cassie = Cassie::new_with_data_dir(format!(
        "/tmp/cassie-fulltext-index-options-{}",
        Uuid::new_v4()
    ))
    .unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        cassie
            .register_collection(
                "ft_docs_options".to_string(),
                Schema {
                    fields: vec![
                        FieldSchema {
                            name: "id".to_string(),
                            data_type: DataType::Text,
                            nullable: true,
                        },
                        FieldSchema {
                            name: "body".to_string(),
                            data_type: DataType::Text,
                            nullable: true,
                        },
                    ],
                },
            )
            ;

        // Act
        let parsed = parse_statement(
            "CREATE INDEX idx_ft_docs_body ON ft_docs_options USING fulltext (body) WITH (boost = 2.5, k1 = 0.8, b = 0.1)",
        )
        .expect("parse should succeed");
        let bound = cassie::sql::binder::bind(parsed, &cassie.catalog)
            .expect("bind should succeed");

        // Assert
        let QueryStatement::CreateIndex(statement) = bound.statement.statement else {
            panic!("expected create index statement");
        };
        assert!(matches!(statement.kind, IndexKind::FullText));
        assert_eq!(statement.options.get("boost"), Some(&"2.5".to_string()));
        assert_eq!(statement.options.get("k1"), Some(&"0.8".to_string()));
        assert_eq!(statement.options.get("b"), Some(&"0.1".to_string()));
    });
}

#[test]
fn should_apply_fulltext_create_index_defaults() {
    // Arrange
    let cassie = Cassie::new_with_data_dir(format!(
        "/tmp/cassie-fulltext-index-defaults-{}",
        Uuid::new_v4()
    ))
    .unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        cassie.register_collection(
            "ft_docs_defaults".to_string(),
            Schema {
                fields: vec![
                    FieldSchema {
                        name: "id".to_string(),
                        data_type: DataType::Text,
                        nullable: true,
                    },
                    FieldSchema {
                        name: "body".to_string(),
                        data_type: DataType::Text,
                        nullable: true,
                    },
                ],
            },
        );

        // Act
        let parsed = parse_statement(
            "CREATE INDEX idx_ft_docs_defaults ON ft_docs_defaults USING fulltext (body)",
        )
        .expect("parse should succeed");
        let bound =
            cassie::sql::binder::bind(parsed, &cassie.catalog).expect("bind should succeed");

        // Assert
        let QueryStatement::CreateIndex(statement) = bound.statement.statement else {
            panic!("expected create index statement");
        };
        assert_eq!(statement.options.get("boost"), Some(&"1".to_string()));
        assert_eq!(statement.options.get("k1"), Some(&"1.2".to_string()));
        assert_eq!(statement.options.get("b"), Some(&"0.75".to_string()));
    });
}

#[test]
fn should_reject_fulltext_create_index_with_non_finite_boost() {
    // Arrange
    let cassie = Cassie::new_with_data_dir(format!(
        "/tmp/cassie-fulltext-index-non-finite-{}",
        Uuid::new_v4()
    ))
    .unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        cassie
            .register_collection(
                "ft_docs_non_finite".to_string(),
                Schema {
                    fields: vec![
                        FieldSchema {
                            name: "id".to_string(),
                            data_type: DataType::Text,
                            nullable: true,
                        },
                        FieldSchema {
                            name: "body".to_string(),
                            data_type: DataType::Text,
                            nullable: true,
                        },
                    ],
                },
            )
            ;

        // Act
        let parsed = parse_statement(
            "CREATE INDEX idx_ft_docs_non_finite ON ft_docs_non_finite USING fulltext (body) WITH (boost = inf)",
        )
        .expect("parse should succeed");
        let bound = cassie::sql::binder::bind(parsed, &cassie.catalog);

        // Assert
        assert!(bound.is_err());
    });
}

#[test]
fn should_reject_duplicate_fulltext_index_on_same_field() {
    // Arrange
    let cassie = Cassie::new_with_data_dir(format!(
        "/tmp/cassie-fulltext-index-duplicate-{}",
        Uuid::new_v4()
    ))
    .unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        cassie
            .register_collection(
                "ft_docs_duplicate".to_string(),
                Schema {
                    fields: vec![
                        FieldSchema {
                            name: "id".to_string(),
                            data_type: DataType::Text,
                            nullable: true,
                        },
                        FieldSchema {
                            name: "body".to_string(),
                            data_type: DataType::Text,
                            nullable: true,
                        },
                    ],
                },
            )
            ;

        cassie
            .catalog
            .register_index(IndexMeta {
                collection: "ft_docs_duplicate".to_string(),
                name: "idx_ft_docs_duplicate_primary".to_string(),
                field: "body".to_string(),
                fields: vec!["body".to_string()],
                expressions: Vec::new(),
                include_fields: Vec::new(),
                predicate: None,
                kind: IndexKind::FullText,
                unique: false,
                options: BTreeMap::from_iter(vec![
                    ("boost".to_string(), "1.0".to_string()),
                    ("k1".to_string(), "1.2".to_string()),
                    ("b".to_string(), "0.75".to_string()),
                ]),
            });

        // Act
        let parsed = parse_statement(
            "CREATE INDEX idx_ft_docs_duplicate_secondary ON ft_docs_duplicate USING fulltext (body)",
        )
        .expect("parse should succeed");
        let bound = cassie::sql::binder::bind(parsed, &cassie.catalog);

        // Assert
        assert!(bound.is_err());
    });
}

#[test]
fn should_reject_fulltext_index_on_non_text_field() {
    // Arrange
    let cassie = Cassie::new_with_data_dir(format!(
        "/tmp/cassie-fulltext-index-non-text-{}",
        Uuid::new_v4()
    ))
    .unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        cassie.register_collection(
            "ft_docs_bad_field".to_string(),
            Schema {
                fields: vec![FieldSchema {
                    name: "score".to_string(),
                    data_type: DataType::Int,
                    nullable: true,
                }],
            },
        );

        // Act
        let parsed = parse_statement(
            "CREATE INDEX idx_ft_docs_bad_field ON ft_docs_bad_field USING fulltext (score)",
        )
        .expect("parse should succeed");
        let bound = cassie::sql::binder::bind(parsed, &cassie.catalog);

        // Assert
        assert!(bound.is_err());
    });
}

#[test]
fn should_reject_fulltext_create_index_with_unsupported_option() {
    // Arrange
    let cassie = Cassie::new_with_data_dir(format!(
        "/tmp/cassie-fulltext-index-unsupported-option-{}",
        Uuid::new_v4()
    ))
    .unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        cassie
            .register_collection(
                "ft_docs_unsupported".to_string(),
                Schema {
                    fields: vec![
                        FieldSchema {
                            name: "id".to_string(),
                            data_type: DataType::Text,
                            nullable: true,
                        },
                        FieldSchema {
                            name: "body".to_string(),
                            data_type: DataType::Text,
                            nullable: true,
                        },
                    ],
                },
            )
            ;

        // Act
        let parsed = parse_statement(
            "CREATE INDEX idx_ft_docs_unsupported ON ft_docs_unsupported USING fulltext (body) WITH (alpha = 0.5)",
        )
        .expect("parse should succeed");
        let bound = cassie::sql::binder::bind(parsed, &cassie.catalog);

        // Assert
        assert!(bound.is_err());
    });
}

#[test]
fn should_reject_fulltext_create_index_with_invalid_fulltext_k1() {
    // Arrange
    let cassie = Cassie::new_with_data_dir(format!(
        "/tmp/cassie-fulltext-index-bad-k1-{}",
        Uuid::new_v4()
    ))
    .unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        cassie
            .register_collection(
                "ft_docs_bad_k1".to_string(),
                Schema {
                    fields: vec![
                        FieldSchema {
                            name: "id".to_string(),
                            data_type: DataType::Text,
                            nullable: true,
                        },
                        FieldSchema {
                            name: "body".to_string(),
                            data_type: DataType::Text,
                            nullable: true,
                        },
                    ],
                },
            )
            ;

        // Act
        let parsed = parse_statement(
            "CREATE INDEX idx_ft_docs_bad_k1 ON ft_docs_bad_k1 USING fulltext (body) WITH (k1 = -1)",
        )
        .expect("parse should succeed");
        let bound = cassie::sql::binder::bind(parsed, &cassie.catalog);

        // Assert
        assert!(bound.is_err());
    });
}

#[test]
fn should_reject_vector_create_index_without_source_field() {
    // Arrange
    let cassie = Cassie::new_with_data_dir(format!(
        "/tmp/cassie-vector-index-no-source-{}",
        Uuid::new_v4()
    ))
    .unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        cassie.register_collection(
            "vec_docs_no_source".to_string(),
            Schema {
                fields: vec![
                    FieldSchema {
                        name: "id".to_string(),
                        data_type: DataType::Text,
                        nullable: true,
                    },
                    FieldSchema {
                        name: "content".to_string(),
                        data_type: DataType::Text,
                        nullable: true,
                    },
                    FieldSchema {
                        name: "embedding".to_string(),
                        data_type: DataType::Vector(3),
                        nullable: true,
                    },
                ],
            },
        );

        // Act
        let parsed = parse_statement(
            "CREATE INDEX idx_docs_embedding ON vec_docs_no_source USING vector (embedding)",
        )
        .expect("parse should succeed");
        let bound = cassie::sql::binder::bind(parsed, &cassie.catalog);

        // Assert
        assert!(bound.is_err());
    });
}

#[test]
fn should_reject_vector_create_index_with_invalid_metric() {
    // Arrange
    let cassie = Cassie::new_with_data_dir(format!(
        "/tmp/cassie-vector-index-bad-metric-{}",
        Uuid::new_v4()
    ))
    .unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        cassie
            .register_collection(
                "vec_docs_invalid_metric".to_string(),
                Schema {
                    fields: vec![
                        FieldSchema {
                            name: "content".to_string(),
                            data_type: DataType::Text,
                            nullable: true,
                        },
                        FieldSchema {
                            name: "embedding".to_string(),
                            data_type: DataType::Vector(3),
                            nullable: true,
                        },
                    ],
                },
            )
            ;

        // Act
        let parsed = parse_statement(
            "CREATE INDEX idx_docs_embedding ON vec_docs_invalid_metric USING vector (embedding) WITH (source_field = content, metric = 'unsupported')",
        )
        .expect("parse should succeed");
        let bound = cassie::sql::binder::bind(parsed, &cassie.catalog);

        // Assert
        assert!(bound.is_err());
    });
}

#[test]
fn should_reject_vector_create_index_on_non_vector_field() {
    // Arrange
    let cassie = Cassie::new_with_data_dir(format!(
        "/tmp/cassie-vector-index-non-vector-{}",
        Uuid::new_v4()
    ))
    .unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        cassie
            .register_collection(
                "vec_docs_not_vector".to_string(),
                Schema {
                    fields: vec![
                        FieldSchema {
                            name: "content".to_string(),
                            data_type: DataType::Text,
                            nullable: true,
                        },
                    ],
                },
            );

        // Act
        let parsed = parse_statement(
            "CREATE INDEX idx_docs_embedding ON vec_docs_not_vector USING vector (content) WITH (source_field = content)",
        )
        .expect("parse should succeed");
        let bound = cassie::sql::binder::bind(parsed, &cassie.catalog);

        // Assert
        assert!(bound.is_err());
    });
}

#[test]
fn should_default_vector_metric_to_cosine() {
    // Arrange
    let cassie = Cassie::new_with_data_dir(format!(
        "/tmp/cassie-vector-index-default-metric-{}",
        Uuid::new_v4()
    ))
    .unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        cassie
            .register_collection(
                "vec_docs_default_metric".to_string(),
                Schema {
                    fields: vec![
                        FieldSchema {
                            name: "content".to_string(),
                            data_type: DataType::Text,
                            nullable: true,
                        },
                        FieldSchema {
                            name: "embedding".to_string(),
                            data_type: DataType::Vector(3),
                            nullable: true,
                        },
                    ],
                },
            )
            ;

        // Act
        let parsed = parse_statement(
            "CREATE INDEX idx_docs_embedding ON vec_docs_default_metric USING vector (embedding) WITH (source_field = content)",
        )
        .expect("parse should succeed");
        let bound = cassie::sql::binder::bind(parsed, &cassie.catalog)
            .expect("bind should succeed");

        // Assert
        let QueryStatement::CreateIndex(statement) = bound.statement.statement else {
            panic!("expected create index statement");
        };
        assert_eq!(statement.options.get("metric"), Some(&"cosine".to_string()));
    });
}

#[test]
fn should_parse_drop_index_statement() {
    // Arrange
    let sql = "DROP INDEX IF EXISTS idx_users_email ON users";

    // Act
    let parsed = parse_statement(sql).expect("parse should succeed");

    // Assert
    let QueryStatement::DropIndex(statement) = parsed.statement else {
        panic!("expected drop index statement");
    };

    assert_eq!(statement.name, "idx_users_email");
    assert_eq!(statement.table, "users");
    assert!(statement.if_exists);
}
