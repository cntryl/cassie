//! Borrow one visible function signature without cloning catalog programs or alias maps.

use std::cmp::Ordering;
use std::mem::size_of;
use std::time::Duration;

use parking_lot::{RwLock, RwLockReadGuard};

use crate::app::CassieError;
use crate::catalog::{
    canonical_schema_name, name_matches, parse_name, FunctionArgMeta, FunctionMeta, ParsedName,
    DEFAULT_SCHEMA, INFORMATION_SCHEMA, PG_CATALOG_SCHEMA,
};
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};
use crate::types::DataType;

use super::Catalog;

/// Raw session scope, borrowed only for the synchronous parameter-inference visit.
pub(crate) struct FunctionInputScope<'a> {
    pub(crate) database: &'a str,
    pub(crate) search_path: &'a [String],
    pub(crate) user: &'a str,
    pub(crate) enforce_database_scope: bool,
}

struct SearchPath<'a> {
    schemas: Vec<&'a str>,
    _memory: Option<QueryMemoryReservation>,
}

impl Catalog {
    /// The callback must not perform another catalog lookup while the signature is borrowed.
    pub(crate) fn with_function_signature<R>(
        &self,
        name: &str,
        scope: Option<&FunctionInputScope<'_>>,
        controls: Option<&QueryExecutionControls>,
        inspect: impl FnOnce(Option<(&[FunctionArgMeta], &DataType)>) -> R,
    ) -> Result<R, CassieError> {
        check(controls)?;
        // Resolve namespaces before taking the function lock. Only borrowed schema names are
        // retained, and neither namespace guards nor statement controls escape this operation.
        let path = match scope.filter(|scope| scope.enforce_database_scope) {
            Some(scope) => self.function_input_search_path(scope, controls)?,
            None => SearchPath {
                schemas: Vec::new(),
                _memory: None,
            },
        };
        let functions = read_catalog(&self.functions, controls)?;
        let mut selected: Option<(&FunctionMeta, (u8, usize))> = None;
        for function in functions.values() {
            check(controls)?;
            let rank = with_parsed_name(&function.name, controls, |parsed| {
                function_rank(function, parsed, name, scope, &path.schemas, controls)
            })??;
            let Some(rank) = rank else { continue };
            if selected.is_none_or(|(previous, previous_rank)| {
                rank < previous_rank
                    || (rank == previous_rank
                        && ascii_lowercase_cmp(&function.name, &previous.name) == Ordering::Less)
            }) {
                selected = Some((function, rank));
            }
        }
        check(controls)?;
        let result = inspect(
            selected.map(|(function, _)| (function.args.as_slice(), &function.return_type)),
        );
        check(controls)?;
        Ok(result)
    }

    fn function_input_search_path<'a>(
        &self,
        scope: &FunctionInputScope<'a>,
        controls: Option<&QueryExecutionControls>,
    ) -> Result<SearchPath<'a>, CassieError> {
        let capacity = scope.search_path.len().max(1);
        let memory = reserve_scratch(controls, checked_mul(capacity, size_of::<&str>())?)?;
        let mut schemas = Vec::new();
        schemas.try_reserve_exact(capacity).map_err(|error| {
            CassieError::ResourceLimit(format!("unable to allocate function search path: {error}"))
        })?;
        if schemas.capacity() > capacity {
            return Err(CassieError::ResourceLimit(
                "allocator exceeded admitted function search path capacity".to_owned(),
            ));
        }
        for entry in scope.search_path {
            check(controls)?;
            let schema = if entry == "$user" {
                scope.user
            } else {
                entry.as_str()
            };
            // Existing session resolution checks existence before BindingContext trims entries.
            if self.function_input_schema_exists(scope.database, schema, controls)? {
                let schema = schema.trim();
                if !schema.is_empty() {
                    schemas.push(schema);
                }
            }
        }
        // Both an empty raw path and a fully filtered path normalize to the current public default.
        if schemas.is_empty() {
            schemas.push(DEFAULT_SCHEMA);
        }
        Ok(SearchPath {
            schemas,
            _memory: memory,
        })
    }

    fn function_input_schema_exists(
        &self,
        database: &str,
        schema: &str,
        controls: Option<&QueryExecutionControls>,
    ) -> Result<bool, CassieError> {
        if [INFORMATION_SCHEMA, PG_CATALOG_SCHEMA, DEFAULT_SCHEMA]
            .iter()
            .any(|known| known.eq_ignore_ascii_case(schema))
        {
            return Ok(true);
        }
        // canonical_schema_name can own escaped components and its combined output together.
        let size = checked_add(database.len(), schema.len())?;
        let _memory = reserve_scratch(controls, scratch_bytes(size)?)?;
        let canonical = canonical_schema_name(database, schema);
        let namespaces = read_catalog(&self.namespaces, controls)?;
        if namespaces.contains_key(&canonical) {
            return Ok(true);
        }
        for stored in namespaces.keys() {
            check(controls)?;
            let _matching_memory = reserve_scratch(
                controls,
                scratch_bytes(checked_add(stored.len(), canonical.len())?)?,
            )?;
            if name_matches(stored, &canonical) {
                return Ok(true);
            }
        }
        Ok(false)
    }
}

fn function_rank(
    function: &FunctionMeta,
    parsed: Option<&ParsedName>,
    requested: &str,
    scope: Option<&FunctionInputScope<'_>>,
    search_path: &[&str],
    controls: Option<&QueryExecutionControls>,
) -> Result<Option<(u8, usize)>, CassieError> {
    let enforced = scope.is_some_and(|scope| scope.enforce_database_scope);
    let Some(ParsedName::DatabaseQualified {
        database,
        schema,
        name,
    }) = parsed
    else {
        return Ok((!enforced && function.name.eq_ignore_ascii_case(requested)).then_some((0, 0)));
    };
    if enforced && scope.is_some_and(|scope| !database.eq_ignore_ascii_case(scope.database)) {
        return Ok(None);
    }
    // functions_for_scope populates canonical keys before schema aliases, then unqualified
    // aliases. Sorted canonical names choose the first candidate for each alias collision.
    if function.name.eq_ignore_ascii_case(requested) {
        return Ok(Some((0, 0)));
    }
    if schema_alias_matches(schema, name, requested) {
        return Ok(Some((1, 0)));
    }
    if !name.eq_ignore_ascii_case(requested) {
        return Ok(None);
    }
    if enforced {
        for (position, candidate) in search_path.iter().enumerate() {
            check(controls)?;
            if candidate.eq_ignore_ascii_case(schema) {
                return Ok(Some((2, position)));
            }
        }
        return Ok(None);
    }
    Ok(Some((2, 0)))
}

fn schema_alias_matches(schema: &str, name: &str, requested: &str) -> bool {
    schema
        .bytes()
        .chain(std::iter::once(b'.'))
        .chain(name.bytes())
        .map(|byte| byte.to_ascii_lowercase())
        .eq(requested.bytes().map(|byte| byte.to_ascii_lowercase()))
}

fn ascii_lowercase_cmp(left: &str, right: &str) -> Ordering {
    left.bytes()
        .map(|byte| byte.to_ascii_lowercase())
        .cmp(right.bytes().map(|byte| byte.to_ascii_lowercase()))
}

fn with_parsed_name<R>(
    raw: &str,
    controls: Option<&QueryExecutionControls>,
    inspect: impl FnOnce(Option<&ParsedName>) -> R,
) -> Result<R, CassieError> {
    check(controls)?;
    let _memory = reserve_scratch(controls, scratch_bytes(raw.len())?)?;
    // Reuse the central quoted-identifier lexer. Its temporary component directory, component
    // buffers, final ParsedName copies, and diagnostic String all fit the conservative admission.
    let parsed = parse_name(raw).ok();
    check(controls)?;
    Ok(inspect(parsed.as_ref()))
}

fn scratch_bytes(length: usize) -> Result<usize, CassieError> {
    // At most length+1 components, a geometrically grown String directory, two copies of
    // component bytes and the lexer buffer coexist. This also covers the fixed error prefix.
    checked_add(checked_mul(checked_add(length, 1)?, 128)?, 256)
}

fn checked_add(left: usize, right: usize) -> Result<usize, CassieError> {
    left.checked_add(right).ok_or_else(scratch_overflow)
}

fn checked_mul(left: usize, right: usize) -> Result<usize, CassieError> {
    left.checked_mul(right).ok_or_else(scratch_overflow)
}

fn scratch_overflow() -> CassieError {
    CassieError::ResourceLimit("function lookup scratch capacity overflow".to_owned())
}

fn reserve_scratch(
    controls: Option<&QueryExecutionControls>,
    bytes: usize,
) -> Result<Option<QueryMemoryReservation>, CassieError> {
    check(controls)?;
    controls
        .map(|controls| controls.reserve_query_memory(bytes))
        .transpose()
}

fn check(controls: Option<&QueryExecutionControls>) -> Result<(), CassieError> {
    controls.map_or(Ok(()), QueryExecutionControls::check_interruption)
}

fn read_catalog<'a, T>(
    catalog: &'a RwLock<T>,
    controls: Option<&QueryExecutionControls>,
) -> Result<RwLockReadGuard<'a, T>, CassieError> {
    if controls.is_none() {
        return Ok(catalog.read());
    }
    loop {
        check(controls)?;
        if let Some(guard) = catalog.try_read_for(Duration::from_millis(1)) {
            check(controls)?;
            return Ok(guard);
        }
    }
}
