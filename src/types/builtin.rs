//! Shared identities for existing scalar runtime types.
//!
//! Legacy vector/array identities remain in schema's v2 adapter. This registry
//! does not admit new named types or change the durable representation.
use super::{DataType, ObjectIdKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BuiltinKind {
    Unknown,
    Bool,
    Int2,
    Int4,
    Int8,
    Float8,
    Text,
    Bpchar,
    Varchar,
    Bytea,
    Uuid,
    Date,
    Time,
    Timestamp,
    Json,
    Jsonb,
    JsonPath,
    ObjectId(super::ObjectIdKind),
    Extended(super::extended::ExtendedType),
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct BuiltinType {
    pub(crate) kind: BuiltinKind,
    pub(crate) oid: u32,
    pub(crate) name: &'static str,
    pub(crate) typlen: i16,
}

impl BuiltinType {
    const fn new(kind: BuiltinKind, oid: u32, name: &'static str, typlen: i16) -> Self {
        Self {
            kind,
            oid,
            name,
            typlen,
        }
    }
}

pub(crate) const BUILTIN_TYPES: &[BuiltinType] = &[
    BuiltinType::new(BuiltinKind::Unknown, 705, "unknown", -2),
    BuiltinType::new(BuiltinKind::Bool, 16, "bool", 1),
    BuiltinType::new(BuiltinKind::Int2, 21, "int2", 2),
    BuiltinType::new(BuiltinKind::Int4, 23, "int4", 4),
    BuiltinType::new(BuiltinKind::Int8, 20, "int8", 8),
    BuiltinType::new(BuiltinKind::Float8, 701, "float8", 8),
    BuiltinType::new(BuiltinKind::Text, 25, "text", -1),
    BuiltinType::new(BuiltinKind::Bpchar, 1042, "bpchar", -1),
    BuiltinType::new(BuiltinKind::Varchar, 1043, "varchar", -1),
    BuiltinType::new(BuiltinKind::Bytea, 17, "bytea", -1),
    BuiltinType::new(BuiltinKind::Uuid, 2950, "uuid", 16),
    BuiltinType::new(BuiltinKind::Date, 1082, "date", 4),
    BuiltinType::new(BuiltinKind::Time, 1083, "time", 8),
    BuiltinType::new(BuiltinKind::Timestamp, 1114, "timestamp", 8),
    BuiltinType::new(BuiltinKind::Json, 114, "json", -1),
    BuiltinType::new(BuiltinKind::Jsonb, 3802, "jsonb", -1),
    BuiltinType::new(BuiltinKind::JsonPath, 4072, "jsonpath", -1),
    object_id_type(ObjectIdKind::Oid),
    object_id_type(ObjectIdKind::RegProc),
    object_id_type(ObjectIdKind::RegProcedure),
    object_id_type(ObjectIdKind::RegOper),
    object_id_type(ObjectIdKind::RegOperator),
    object_id_type(ObjectIdKind::RegClass),
    object_id_type(ObjectIdKind::RegType),
    object_id_type(ObjectIdKind::RegConfig),
    object_id_type(ObjectIdKind::RegDictionary),
    object_id_type(ObjectIdKind::RegNamespace),
    object_id_type(ObjectIdKind::RegRole),
    object_id_type(ObjectIdKind::RegCollation),
    extended_type(super::extended::ExtendedType::Real),
    extended_type(super::extended::ExtendedType::Inet),
    extended_type(super::extended::ExtendedType::Cidr),
    extended_type(super::extended::ExtendedType::Macaddr),
    extended_type(super::extended::ExtendedType::Macaddr8),
    extended_type(super::extended::ExtendedType::PgLsn),
    extended_type(super::extended::ExtendedType::Bit { length: None }),
    extended_type(super::extended::ExtendedType::VarBit { length: None }),
    extended_type(super::extended::ExtendedType::Numeric(None)),
    extended_type(super::extended::ExtendedType::TimeTz { precision: None }),
    extended_type(super::extended::ExtendedType::TimestampTz { precision: None }),
    extended_type(super::extended::ExtendedType::Interval { typmod: -1 }),
];

/// Natively carried `date`/`time`/`timestamp` share the existing identities;
/// typmods do not change a type's catalog entry.
const fn extended_builtin_kind(kind: super::extended::ExtendedType) -> BuiltinKind {
    use super::extended::ExtendedType;
    match kind.base() {
        ExtendedType::Date => BuiltinKind::Date,
        ExtendedType::Time { .. } => BuiltinKind::Time,
        ExtendedType::Timestamp { .. } => BuiltinKind::Timestamp,
        base => BuiltinKind::Extended(base),
    }
}

const fn extended_type(kind: super::extended::ExtendedType) -> BuiltinType {
    BuiltinType::new(
        BuiltinKind::Extended(kind),
        kind.type_oid(),
        kind.typname(),
        kind.typlen(),
    )
}

const fn object_id_type(kind: ObjectIdKind) -> BuiltinType {
    BuiltinType::new(BuiltinKind::ObjectId(kind), kind.type_oid(), kind.name(), 4)
}

pub(crate) fn builtin_for_oid(oid: u32) -> Option<&'static BuiltinType> {
    BUILTIN_TYPES.iter().find(|metadata| metadata.oid == oid)
}

/// The type a declared parameter OID binds as. An extended family whose
/// operators are incomplete keeps its parameters on the numeric adapter path.
pub(crate) fn parameter_builtin_for_oid(oid: u32) -> Option<&'static BuiltinType> {
    builtin_for_oid(oid).filter(|metadata| match metadata.kind {
        BuiltinKind::Unknown => false,
        BuiltinKind::Extended(kind) => kind.binds_parameters(),
        _ => true,
    })
}

impl BuiltinKind {
    pub(crate) fn metadata(self) -> &'static BuiltinType {
        BUILTIN_TYPES
            .iter()
            .find(|metadata| metadata.kind == self)
            .expect("every current scalar kind has registered metadata")
    }

    pub(crate) const fn data_type(self) -> DataType {
        match self {
            Self::Unknown => DataType::Null,
            Self::Bool => DataType::Boolean,
            Self::Int2 => DataType::SmallInt,
            Self::Int4 => DataType::Int,
            Self::Int8 => DataType::BigInt,
            Self::Float8 => DataType::Float,
            Self::Text => DataType::Text,
            Self::Bpchar => DataType::Char { length: None },
            Self::Varchar => DataType::Varchar { length: None },
            Self::Bytea => DataType::Bytea,
            Self::Uuid => DataType::Uuid,
            Self::Date => DataType::Date,
            Self::Time => DataType::Time,
            Self::Timestamp => DataType::Timestamp,
            Self::Json => DataType::Json,
            Self::Jsonb => DataType::Jsonb,
            Self::JsonPath => DataType::JsonPath,
            Self::ObjectId(kind) => DataType::ObjectId(kind),
            Self::Extended(kind) => DataType::Extended(kind),
        }
    }
}

/// The current container adapter is explicit; every scalar must resolve metadata.
pub(crate) enum TypeMetadata<'a> {
    Scalar(&'static BuiltinType),
    Vector(usize),
    Array(&'a DataType),
}

pub(crate) fn type_metadata(data_type: &DataType) -> TypeMetadata<'_> {
    let kind = match data_type {
        DataType::Null => BuiltinKind::Unknown,
        DataType::Boolean => BuiltinKind::Bool,
        DataType::SmallInt => BuiltinKind::Int2,
        DataType::Int => BuiltinKind::Int4,
        DataType::BigInt => BuiltinKind::Int8,
        DataType::Float => BuiltinKind::Float8,
        DataType::Text => BuiltinKind::Text,
        DataType::Char { .. } => BuiltinKind::Bpchar,
        DataType::Varchar { .. } => BuiltinKind::Varchar,
        DataType::Bytea => BuiltinKind::Bytea,
        DataType::Uuid => BuiltinKind::Uuid,
        DataType::Date => BuiltinKind::Date,
        DataType::Time => BuiltinKind::Time,
        DataType::Timestamp => BuiltinKind::Timestamp,
        DataType::Json => BuiltinKind::Json,
        DataType::Jsonb => BuiltinKind::Jsonb,
        DataType::JsonPath => BuiltinKind::JsonPath,
        DataType::ObjectId(kind) => BuiltinKind::ObjectId(*kind),
        DataType::Extended(kind) => extended_builtin_kind(*kind),
        DataType::Vector(dimensions) => return TypeMetadata::Vector(*dimensions),
        DataType::Array(inner) => return TypeMetadata::Array(inner),
    };
    TypeMetadata::Scalar(kind.metadata())
}
