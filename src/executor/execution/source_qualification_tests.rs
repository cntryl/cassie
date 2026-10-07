use super::*;
use std::mem::size_of;
use std::sync::Arc;
use std::time::Instant;

fn with_fixture(run: impl FnOnce(&Cassie)) {
    let path =
        std::env::temp_dir().join(format!("cassie-857-qualification-{}", uuid::Uuid::new_v4()));
    let cassie = Cassie::new_with_data_dir(&path).expect("fixture");
    run(&cassie);
    drop(cassie);
    std::fs::remove_dir_all(path).expect("strict fixture cleanup");
}

#[test]
fn should_retain_fresh_derived_qualification_backing_with_parent_roots() {
    // Arrange
    with_fixture(|cassie| {
        let limits = crate::config::CassieRuntimeLimits {
            query_memory_budget_bytes: 1024 * 1024,
            ..crate::config::CassieRuntimeLimits::default()
        };
        let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
        let parent = Arc::new(
            controls
                .reserve_query_memory(65_536)
                .expect("source parent"),
        );
        let row = BatchRow::new(vec![("n".into(), Value::Int64(1))])
            .with_query_memory(Some(Arc::clone(&parent)))
            .retain_operator_memory(&controls, Arc::clone(&parent))
            .expect("operator root");
        let before = controls.current_query_memory_bytes();
        let functions = HashMap::new();
        let env = SourceExecutionEnv {
            cassie,
            session: None,
            user_functions: &functions,
            params: &[],
            controls: &controls,
        };
        let qualifier = "q".repeat(4096);
        // Act
        let (output, _) =
            finalize_source_batches(&env, vec![vec![row]], Vec::new(), true, &qualifier)
                .expect("qualified derived output");
        // Assert
        assert!(
            controls.current_query_memory_bytes() > before + qualifier.len(),
            "new aliases and eager lookup must retain independent charge"
        );
        assert_eq!(
            output[0][0].get(&format!("{qualifier}.n")),
            Some(&Value::Int64(1))
        );
        drop(parent);
        assert!(controls.current_query_memory_bytes() > 65_536);
        drop(output);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    });
}

#[test]
fn should_deny_derived_qualification_before_constructing_long_aliases() {
    // Arrange
    with_fixture(|cassie| {
        let limits = crate::config::CassieRuntimeLimits {
            query_memory_budget_bytes: 65_536 + 1024,
            ..crate::config::CassieRuntimeLimits::default()
        };
        let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
        let parent = Arc::new(
            controls
                .reserve_query_memory(65_536)
                .expect("source parent"),
        );
        let row = BatchRow::new(vec![("n".into(), Value::Int64(1))])
            .with_query_memory(Some(Arc::clone(&parent)))
            .retain_operator_memory(&controls, Arc::clone(&parent))
            .expect("operator root");
        let functions = HashMap::new();
        let env = SourceExecutionEnv {
            cassie,
            session: None,
            user_functions: &functions,
            params: &[],
            controls: &controls,
        };
        // Act
        let result =
            finalize_source_batches(&env, vec![vec![row]], Vec::new(), true, &"q".repeat(4096));
        // Assert
        assert!(
            matches!(
                result.map_err(crate::app::CassieError::from),
                Err(crate::app::CassieError::ResourceLimit(_))
            ),
            "old roots cannot admit newly constructed aliases"
        );
        assert_eq!(controls.current_query_memory_bytes(), 65_536);
        drop(parent);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    });
}

#[test]
fn should_bound_actual_non_power_of_two_alias_growth() {
    // Arrange
    let aliases = (0..10)
        .map(|index| (format!("a{index}"), 0))
        .collect::<Vec<_>>();
    assert_eq!(aliases.capacity(), 10);
    let row = BatchRow::with_aliases(vec![("n".into(), Value::Int64(1))], aliases);
    let admitted =
        super::source_collection::row_qualification_bytes("q", &row).expect("qualification bound");
    // Act
    let qualified = qualify_row(row, "q");
    let (entries, aliases) = qualified.into_parts();
    // Assert
    assert_eq!(aliases.len(), 11);
    assert_eq!(aliases.capacity(), 20);
    let names = entries.iter().map(|(name, _)| name.len()).sum::<usize>()
        + aliases.iter().map(|(name, _)| name.len()).sum::<usize>();
    let lookup =
        crate::executor::retained_memory::lookup_bytes(entries.len() + aliases.len(), names)
            .expect("eager lookup authority");
    let minimum = aliases.capacity() * size_of::<(String, usize)>()
        + lookup
        + aliases.last().expect("fresh alias").0.capacity();
    assert!(
        admitted >= minimum,
        "actual capacity20 must be bounded, not rounded16: admitted={admitted} minimum={minimum}"
    );
}

#[test]
fn should_retain_cte_array_source_descriptor_replacement_until_output_drop() {
    // Arrange
    with_fixture(|cassie| {
        let controls = QueryExecutionControls::from_limits(
            &crate::config::CassieRuntimeLimits::default(),
            Instant::now(),
        );
        let parent = Arc::new(controls.reserve_query_memory(65_536).expect("source owner"));
        let context = CteContext::unleased(HashMap::from([(
            "c".into(),
            super::super::cte::CteRelation {
                rows: vec![],
                fields: vec![crate::types::FieldSchema {
                    name: "n".into(),
                    data_type: crate::types::DataType::Array(Box::new(
                        crate::types::DataType::Text,
                    )),
                    nullable: true,
                }],
                _rows_memory: None,
                _fields_memory: None,
            },
        )]));
        let functions = HashMap::new();
        let env = SourceExecutionEnv {
            cassie,
            session: None,
            user_functions: &functions,
            params: &[],
            controls: &controls,
        };
        let mut batches = vec![vec![BatchRow::new(vec![("n".into(), Value::Null)])
            .with_query_memory(Some(Arc::clone(&parent)))]];
        // Act
        source_shape::attach_types(&env, &QuerySource::Cte("c".into()), &context, &mut batches)
            .expect("existing ARRAY descriptor");
        // Assert
        assert!(
            controls.current_query_memory_bytes() > 65_536,
            "new shared descriptors must retain replacement charge"
        );
        let types = batches[0][0]
            .shared_data_types()
            .expect("shared descriptor");
        let root = batches[0][0].operator_memory().expect("descriptor root");
        let nodes = 2 * (std::mem::size_of_val(root.as_ref()) + 2 * size_of::<usize>());
        let heap = crate::executor::retained_memory::data_type_clone_bytes(&types[0])
            .expect("nested descriptor heap");
        let minimum = nodes
            + size_of::<Vec<crate::types::DataType>>()
            + 2 * size_of::<usize>()
            + types.capacity() * size_of::<crate::types::DataType>()
            + heap
            + size_of::<crate::runtime::QueryMemoryReservation>()
            + 2 * size_of::<usize>();
        assert!(
            controls.current_query_memory_bytes() - 65_536 >= minimum,
            "retained type capacity{} must be charged: delta{} minimum{minimum}",
            types.capacity(),
            controls.current_query_memory_bytes() - 65_536
        );
        drop(root);
        drop(types);

        assert_eq!(
            batches[0][0].data_types()[0],
            crate::types::DataType::Array(Box::new(crate::types::DataType::Text))
        );
        drop(parent);
        assert!(controls.current_query_memory_bytes() > 65_536);
        drop(batches);
        drop(context);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    });
}

#[test]
fn should_retain_known_cte_null_template_copy_until_template_drop() {
    // Arrange
    with_fixture(|cassie| {
        let controls = QueryExecutionControls::from_limits(
            &crate::config::CassieRuntimeLimits::default(),
            Instant::now(),
        );
        let context = CteContext::unleased(HashMap::from([(
            "c".into(),
            super::super::cte::CteRelation {
                rows: vec![],
                fields: vec![crate::types::FieldSchema {
                    name: "n".repeat(4096),
                    data_type: crate::types::DataType::Array(Box::new(
                        crate::types::DataType::Text,
                    )),
                    nullable: true,
                }],
                _rows_memory: None,
                _fields_memory: None,
            },
        )]));
        let functions = HashMap::new();
        let env = SourceExecutionEnv {
            cassie,
            session: None,
            user_functions: &functions,
            params: &[],
            controls: &controls,
        };
        // Act
        let template = source_shape::null_row(&env, &QuerySource::Cte("c".into()), &context)
            .expect("known CTE NULL template");
        // Assert
        assert_eq!(template.entries()[0].1, Value::Null);
        assert_eq!(
            template.data_types()[0],
            crate::types::DataType::Array(Box::new(crate::types::DataType::Text))
        );
        assert!(
            controls.current_query_memory_bytes() >= 4096,
            "actual live template must own copied names and ARRAY metadata"
        );
        drop(context);
        assert!(controls.current_query_memory_bytes() >= 4096);
        drop(template);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    });
}

#[test]
fn should_deny_known_cte_null_template_copy_without_partial_publication() {
    // Arrange
    with_fixture(|cassie| {
        let controls = QueryExecutionControls::from_limits(
            &crate::config::CassieRuntimeLimits {
                query_memory_budget_bytes: 128,
                ..crate::config::CassieRuntimeLimits::default()
            },
            Instant::now(),
        );
        let context = CteContext::unleased(HashMap::from([(
            "c".into(),
            super::super::cte::CteRelation {
                rows: vec![],
                fields: vec![crate::types::FieldSchema {
                    name: "n".repeat(4096),
                    data_type: crate::types::DataType::Array(Box::new(
                        crate::types::DataType::Text,
                    )),
                    nullable: true,
                }],
                _rows_memory: None,
                _fields_memory: None,
            },
        )]));
        let functions = HashMap::new();
        let env = SourceExecutionEnv {
            cassie,
            session: None,
            user_functions: &functions,
            params: &[],
            controls: &controls,
        };
        // Act
        let result = source_shape::null_row(&env, &QuerySource::Cte("c".into()), &context);
        // Assert
        assert!(matches!(
            result.map_err(crate::app::CassieError::from),
            Err(crate::app::CassieError::ResourceLimit(_))
        ));
        assert_eq!(context["c"].fields[0].name.len(), 4096);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    });
}

#[test]
fn should_admit_combined_known_cte_templates_before_copying_backing() {
    // Arrange
    with_fixture(|cassie| {
        let fields = |name: &str| {
            vec![crate::types::FieldSchema {
                name: name.into(),
                data_type: crate::types::DataType::Array(Box::new(crate::types::DataType::Text)),
                nullable: true,
            }]
        };
        let context = CteContext::unleased(HashMap::from([
            (
                "c".into(),
                super::super::cte::CteRelation {
                    rows: vec![],
                    fields: fields("n"),
                    _rows_memory: None,
                    _fields_memory: None,
                },
            ),
            (
                "d".into(),
                super::super::cte::CteRelation {
                    rows: vec![],
                    fields: fields("m"),
                    _rows_memory: None,
                    _fields_memory: None,
                },
            ),
        ]));
        let source = QuerySource::Join {
            left: Box::new(QuerySource::Cte("c".into())),
            right: Box::new(QuerySource::Cte("d".into())),
            kind: JoinKind::Inner,
            on: Expr::BoolLiteral(true),
        };
        let functions = HashMap::new();
        let limits = crate::config::CassieRuntimeLimits::default();
        let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
        let env = SourceExecutionEnv {
            cassie,
            session: None,
            user_functions: &functions,
            params: &[],
            controls: &controls,
        };
        let left = source_shape::null_row(&env, &QuerySource::Cte("c".into()), &context)
            .expect("left template");
        let right = source_shape::null_row(&env, &QuerySource::Cte("d".into()), &context)
            .expect("right template");
        let minimum =
            source_join::combined_row_bytes(&left, &right).expect("borrowed combined authority");
        drop(left);
        drop(right);
        assert_eq!(controls.current_query_memory_bytes(), 0);
        // Act
        let template = source_shape::null_row(&env, &source, &context).expect("combined template");
        // Assert
        assert!(
            template
                .query_memory()
                .expect("independent copied output owner")
                .bytes()
                >= minimum
        );
        assert_eq!(template.get("c.n"), Some(&Value::Null));
        assert_eq!(template.get("d.m"), Some(&Value::Null));
        assert_eq!(template.data_types().len(), 2);
        let peak = controls.peak_query_memory_bytes();
        drop(template);
        assert_eq!(controls.current_query_memory_bytes(), 0);
        let denied = QueryExecutionControls::from_limits(
            &crate::config::CassieRuntimeLimits {
                query_memory_budget_bytes: peak - 1,
                ..limits
            },
            Instant::now(),
        );
        let env = SourceExecutionEnv {
            cassie,
            session: None,
            user_functions: &functions,
            params: &[],
            controls: &denied,
        };
        let result = source_shape::null_row(&env, &source, &context);
        assert!(matches!(
            result.map_err(crate::app::CassieError::from),
            Err(crate::app::CassieError::ResourceLimit(_))
        ));
        assert_eq!(denied.current_query_memory_bytes(), 0);
    });
}

fn derived_cte_template_source(alias: &str, lateral: bool) -> QuerySource {
    let parsed =
        crate::sql::parse_statement("SELECT n FROM c").expect("existing derived SELECT syntax");
    let crate::sql::ast::QueryStatement::Select(mut select) = parsed.statement else {
        panic!("SELECT fixture");
    };
    select.source = QuerySource::Cte("c".into());
    QuerySource::Subquery {
        alias: alias.into(),
        select: Box::new(select),
        lateral,
    }
}

#[test]
fn should_retain_derived_and_lateral_cte_array_templates_until_drop() {
    // Arrange
    with_fixture(|cassie| {
        let mut missing = Vec::new();
        for lateral in [false, true] {
            let controls = QueryExecutionControls::from_limits(
                &crate::config::CassieRuntimeLimits::default(),
                Instant::now(),
            );
            let context = CteContext::unleased(HashMap::from([(
                "c".into(),
                super::super::cte::CteRelation {
                    rows: vec![],
                    fields: vec![crate::types::FieldSchema {
                        name: "n".into(),
                        data_type: crate::types::DataType::Array(Box::new(
                            crate::types::DataType::Text,
                        )),
                        nullable: true,
                    }],
                    _rows_memory: None,
                    _fields_memory: None,
                },
            )]));
            let functions = HashMap::new();
            let env = SourceExecutionEnv {
                cassie,
                session: None,
                user_functions: &functions,
                params: &[],
                controls: &controls,
            };
            // Act
            let template =
                source_shape::null_row(&env, &derived_cte_template_source("q", lateral), &context)
                    .expect("existing CTE-derived template");
            // Assert
            assert_eq!(
                template.data_types()[0],
                crate::types::DataType::Array(Box::new(crate::types::DataType::Text))
            );
            assert_eq!(template.get("q.n"), Some(&Value::Null));
            println!(
                "derived template lateral={lateral} live_bytes={}",
                controls.current_query_memory_bytes()
            );
            if controls.current_query_memory_bytes() == 0 {
                missing.push(lateral);
            }
            drop(context);
            if controls.current_query_memory_bytes() == 0 && !missing.contains(&lateral) {
                missing.push(lateral);
            }
            drop(template);
            assert_eq!(controls.current_query_memory_bytes(), 0);
        }
        assert!(
            missing.is_empty(),
            "missing copied template owners for lateral flags {missing:?}"
        );
    });
}

#[test]
fn should_deny_derived_and_lateral_cte_template_aliases_before_construction() {
    // Arrange
    with_fixture(|cassie| {
        let mut missing = Vec::new();
        for lateral in [false, true] {
            let controls = QueryExecutionControls::from_limits(
                &crate::config::CassieRuntimeLimits {
                    query_memory_budget_bytes: 4096,
                    ..crate::config::CassieRuntimeLimits::default()
                },
                Instant::now(),
            );
            let context = CteContext::unleased(HashMap::from([(
                "c".into(),
                super::super::cte::CteRelation {
                    rows: vec![],
                    fields: vec![crate::types::FieldSchema {
                        name: "n".into(),
                        data_type: crate::types::DataType::Array(Box::new(
                            crate::types::DataType::Text,
                        )),
                        nullable: true,
                    }],
                    _rows_memory: None,
                    _fields_memory: None,
                },
            )]));
            let functions = HashMap::new();
            let env = SourceExecutionEnv {
                cassie,
                session: None,
                user_functions: &functions,
                params: &[],
                controls: &controls,
            };
            // Act
            let result = source_shape::null_row(
                &env,
                &derived_cte_template_source(&"q".repeat(4096), lateral),
                &context,
            );
            // Assert
            let denied = matches!(
                result.map_err(crate::app::CassieError::from),
                Err(crate::app::CassieError::ResourceLimit(_))
            );
            println!("derived alias admission lateral={lateral} denied={denied}");
            if !denied {
                missing.push(lateral);
            }
            assert_eq!(context["c"].fields.len(), 1);
            assert_eq!(controls.current_query_memory_bytes(), 0);
        }
        assert!(
            missing.is_empty(),
            "missing alias admission for lateral flags {missing:?}"
        );
    });
}
