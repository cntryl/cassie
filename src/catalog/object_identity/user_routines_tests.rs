use super::stable_routine_oid;
use crate::catalog::object_identity::ObjectScope;
use crate::catalog::{Catalog, FunctionArgMeta, FunctionMeta, ProcedureMeta, Volatility};
use crate::sql::binder::BindingContext;
use crate::types::{DataType, ObjectIdKind};

const DATABASE: &str = "routine_identity_test";

fn argument(data_type: DataType) -> FunctionArgMeta {
    FunctionArgMeta {
        name: "x".to_string(),
        data_type,
    }
}

fn register_function(catalog: &Catalog, canonical: &str, arguments: Vec<DataType>) {
    catalog.register_function(FunctionMeta {
        name: canonical.to_string(),
        args: arguments.into_iter().map(argument).collect(),
        return_type: DataType::Int,
        volatility: Volatility::Immutable,
        body: "1".to_string(),
    });
}

fn context(search_path: &[&str]) -> BindingContext {
    BindingContext::new(
        DATABASE,
        search_path.iter().map(ToString::to_string).collect(),
    )
}

fn canonical(schema: &str, name: &str) -> String {
    format!("{DATABASE}.{schema}.{name}")
}

#[test]
fn should_keep_routine_oid_stable_across_name_case() {
    // Arrange
    let name = "tenant.public.Rate";

    // Act
    let upper = stable_routine_oid('f', name);
    let lower = stable_routine_oid('f', &name.to_ascii_lowercase());

    // Assert
    assert_eq!(upper, lower);
}

#[test]
fn should_distinguish_function_and_procedure_oids_for_one_name() {
    // Arrange
    let name = "tenant.public.rate";

    // Act
    let function = stable_routine_oid('f', name);
    let procedure = stable_routine_oid('p', name);

    // Assert
    assert_ne!(function, procedure);
}

#[test]
fn should_place_routine_oids_above_the_catalog_allocator_range() {
    // Arrange
    let name = "tenant.public.rate";

    // Act
    let oid = stable_routine_oid('f', name);

    // Assert
    assert!(oid >= 0x8000_0000);
}

#[test]
fn should_resolve_builtin_routine_signatures_by_argument_identity() {
    // Arrange
    let catalog = Catalog::new();
    let context = context(&["public"]);
    let scope = ObjectScope::new(&catalog, &context);
    let spellings = ["pg_catalog.abs(integer)", "abs(int4)", "ABS( INTEGER )"];

    // Act
    let oids = spellings.map(|spelling| scope.resolve(ObjectIdKind::RegProcedure, spelling));

    // Assert
    assert!(oids.iter().all(|oid| matches!(oid, Ok(Some(1397)))));
    assert_eq!(
        scope.label(ObjectIdKind::RegProcedure, 1397),
        "abs(integer)"
    );
}

#[test]
fn should_reject_an_overloaded_bare_routine_name() {
    // Arrange
    let catalog = Catalog::new();
    let context = context(&["public"]);
    let scope = ObjectScope::new(&catalog, &context);

    // Act
    let resolved = scope.resolve(ObjectIdKind::RegProc, "pg_catalog.abs");

    // Assert
    assert!(resolved.is_err());
}

#[test]
fn should_resolve_user_function_by_exact_argument_types() {
    // Arrange
    let catalog = Catalog::new();
    register_function(&catalog, &canonical("public", "rate"), vec![DataType::Int]);
    let context = context(&["public"]);
    let scope = ObjectScope::new(&catalog, &context);

    // Act
    let matching = scope.resolve(ObjectIdKind::RegProcedure, "rate(integer)");
    let other = scope.resolve(ObjectIdKind::RegProcedure, "rate(text)");

    // Assert
    assert!(matches!(matching, Ok(Some(_))));
    assert!(matches!(other, Ok(None)));
}

#[test]
fn should_resolve_user_function_bare_name_to_the_same_identity() {
    // Arrange
    let catalog = Catalog::new();
    register_function(&catalog, &canonical("public", "rate"), vec![DataType::Int]);
    let context = context(&["public"]);
    let scope = ObjectScope::new(&catalog, &context);

    // Act
    let by_signature = scope.resolve(ObjectIdKind::RegProcedure, "rate(int4)");
    let by_name = scope.resolve(ObjectIdKind::RegProc, "rate");

    // Assert
    assert!(matches!(by_signature, Ok(Some(_))));
    assert_eq!(by_signature.ok(), by_name.ok());
}

#[test]
fn should_render_user_function_without_schema_when_the_search_path_finds_it() {
    // Arrange
    let catalog = Catalog::new();
    register_function(&catalog, &canonical("public", "rate"), vec![DataType::Int]);
    let context = context(&["public"]);
    let scope = ObjectScope::new(&catalog, &context);
    let oid = scope
        .resolve(ObjectIdKind::RegProcedure, "rate(integer)")
        .ok()
        .flatten()
        .unwrap_or_default();

    // Act
    let label = scope.label(ObjectIdKind::RegProcedure, oid);

    // Assert
    assert_eq!(label, "rate(integer)");
}

#[test]
fn should_render_user_function_with_schema_when_the_search_path_misses_it() {
    // Arrange
    let catalog = Catalog::new();
    register_function(&catalog, &canonical("billing", "rate"), vec![DataType::Int]);
    let context = context(&["public"]);
    let scope = ObjectScope::new(&catalog, &context);
    let oid = scope
        .resolve(ObjectIdKind::RegProcedure, "billing.rate(integer)")
        .ok()
        .flatten()
        .unwrap_or_default();

    // Act
    let label = scope.label(ObjectIdKind::RegProcedure, oid);

    // Assert
    assert_eq!(label, "billing.rate(integer)");
}

#[test]
fn should_render_array_argument_types_in_a_user_function_signature() {
    // Arrange
    let catalog = Catalog::new();
    let array = DataType::Array(Box::new(DataType::Int));
    register_function(&catalog, &canonical("public", "total"), vec![array]);
    let context = context(&["public"]);
    let scope = ObjectScope::new(&catalog, &context);
    let oid = scope
        .resolve(ObjectIdKind::RegProcedure, "total(int[])")
        .ok()
        .flatten()
        .unwrap_or_default();

    // Act
    let label = scope.label(ObjectIdKind::RegProcedure, oid);

    // Assert
    assert_eq!(label, "total(integer[])");
}

#[test]
fn should_report_ambiguity_for_a_bare_name_defined_in_two_schemas_with_different_arguments() {
    // Arrange
    let catalog = Catalog::new();
    register_function(&catalog, &canonical("first", "pick"), vec![DataType::Int]);
    register_function(&catalog, &canonical("second", "pick"), vec![DataType::Text]);
    let context = context(&["first", "second"]);
    let scope = ObjectScope::new(&catalog, &context);

    // Act
    let resolved = scope.resolve(ObjectIdKind::RegProc, "pick");

    // Assert
    assert_eq!(resolved.err().map(|error| error.sqlstate()), Some("42725"));
}

#[test]
fn should_prefer_the_earlier_schema_for_identical_signatures() {
    // Arrange
    let catalog = Catalog::new();
    register_function(&catalog, &canonical("first", "pick"), vec![DataType::Int]);
    register_function(&catalog, &canonical("second", "pick"), vec![DataType::Int]);
    let context = context(&["first", "second"]);
    let scope = ObjectScope::new(&catalog, &context);

    // Act
    let bare = scope.resolve(ObjectIdKind::RegProc, "pick");
    let first = scope.resolve(ObjectIdKind::RegProcedure, "first.pick(integer)");

    // Assert
    assert!(matches!(bare, Ok(Some(_))));
    assert_eq!(bare.ok(), first.ok());
}

#[test]
fn should_qualify_a_shadowed_user_function_in_regproc_output() {
    // Arrange
    let catalog = Catalog::new();
    register_function(&catalog, &canonical("first", "pick"), vec![DataType::Int]);
    register_function(&catalog, &canonical("second", "pick"), vec![DataType::Int]);
    let context = context(&["first", "second"]);
    let scope = ObjectScope::new(&catalog, &context);
    let shadowed = scope
        .resolve(ObjectIdKind::RegProcedure, "second.pick(integer)")
        .ok()
        .flatten()
        .unwrap_or_default();

    // Act
    let label = scope.label(ObjectIdKind::RegProcedure, shadowed);

    // Assert
    assert_eq!(label, "second.pick(integer)");
}

#[test]
fn should_not_resolve_a_function_of_another_database() {
    // Arrange
    let catalog = Catalog::new();
    register_function(&catalog, "other_database.public.rate", vec![DataType::Int]);
    let context = context(&["public"]);
    let scope = ObjectScope::new(&catalog, &context);

    // Act
    let resolved = scope.resolve(ObjectIdKind::RegProcedure, "rate(integer)");

    // Assert
    assert!(matches!(resolved, Ok(None)));
}

#[test]
fn should_resolve_a_user_procedure_by_signature() {
    // Arrange
    let catalog = Catalog::new();
    catalog.register_procedure(ProcedureMeta {
        name: canonical("public", "refresh"),
        args: vec![argument(DataType::Text)],
        body: "SELECT 1".to_string(),
    });
    let context = context(&["public"]);
    let scope = ObjectScope::new(&catalog, &context);

    // Act
    let resolved = scope
        .resolve(ObjectIdKind::RegProcedure, "refresh(text)")
        .ok()
        .flatten();

    // Assert
    assert_eq!(
        resolved.map(|oid| scope.label(ObjectIdKind::RegProcedure, oid)),
        Some("refresh(text)".to_string())
    );
}

#[test]
fn should_report_an_undefined_argument_type_in_a_user_signature() {
    // Arrange
    let catalog = Catalog::new();
    register_function(&catalog, &canonical("public", "rate"), vec![DataType::Int]);
    let context = context(&["public"]);
    let scope = ObjectScope::new(&catalog, &context);

    // Act
    let resolved = scope.resolve(ObjectIdKind::RegProcedure, "rate(no_such_type)");

    // Assert
    assert_eq!(resolved.err().map(|error| error.sqlstate()), Some("42704"));
}
