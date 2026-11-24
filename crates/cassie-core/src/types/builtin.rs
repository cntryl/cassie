//! Shared identities for existing scalar runtime types.
//!
//! Legacy vector/array identities remain in schema's v2 adapter. This registry
//! does not admit new named types or change the durable representation.
use super::{DataType, ObjectIdKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[doc(hidden)]
pub enum BuiltinKind {
    Unknown,
    Bool,
    Int2,
    Int4,
    Int8,
    Float8,
    Name,
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
#[doc(hidden)]
pub struct BuiltinType {
    pub kind: BuiltinKind,
    pub oid: u32,
    pub name: &'static str,
    pub typlen: i16,
    pub array_oid: u32,
}

impl BuiltinType {
    const fn new(
        kind: BuiltinKind,
        oid: u32,
        name: &'static str,
        typlen: i16,
        array_oid: u32,
    ) -> Self {
        Self {
            kind,
            oid,
            name,
            typlen,
            array_oid,
        }
    }
}

pub const BUILTIN_TYPES: &[BuiltinType] = &[
    BuiltinType::new(BuiltinKind::Unknown, 705, "unknown", -2, 0),
    BuiltinType::new(BuiltinKind::Bool, 16, "bool", 1, 1000),
    BuiltinType::new(BuiltinKind::Int2, 21, "int2", 2, 1005),
    BuiltinType::new(BuiltinKind::Int4, 23, "int4", 4, 1007),
    BuiltinType::new(BuiltinKind::Int8, 20, "int8", 8, 1016),
    BuiltinType::new(BuiltinKind::Float8, 701, "float8", 8, 1022),
    BuiltinType::new(BuiltinKind::Name, 19, "name", 64, 1003),
    BuiltinType::new(BuiltinKind::Text, 25, "text", -1, 1009),
    BuiltinType::new(BuiltinKind::Bpchar, 1042, "bpchar", -1, 1014),
    BuiltinType::new(BuiltinKind::Varchar, 1043, "varchar", -1, 1015),
    BuiltinType::new(BuiltinKind::Bytea, 17, "bytea", -1, 1001),
    BuiltinType::new(BuiltinKind::Uuid, 2950, "uuid", 16, 2951),
    BuiltinType::new(BuiltinKind::Date, 1082, "date", 4, 1182),
    BuiltinType::new(BuiltinKind::Time, 1083, "time", 8, 1183),
    BuiltinType::new(BuiltinKind::Timestamp, 1114, "timestamp", 8, 1115),
    BuiltinType::new(BuiltinKind::Json, 114, "json", -1, 199),
    BuiltinType::new(BuiltinKind::Jsonb, 3802, "jsonb", -1, 3807),
    BuiltinType::new(BuiltinKind::JsonPath, 4072, "jsonpath", -1, 4073),
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
        extended_array_oid(kind),
    )
}

const fn object_id_type(kind: ObjectIdKind) -> BuiltinType {
    BuiltinType::new(
        BuiltinKind::ObjectId(kind),
        kind.type_oid(),
        kind.name(),
        4,
        object_id_array_oid(kind),
    )
}

const fn extended_array_oid(kind: super::extended::ExtendedType) -> u32 {
    use super::extended::ExtendedType;
    match kind.base() {
        ExtendedType::Real => 1021,
        ExtendedType::Numeric(_) => 1231,
        ExtendedType::Inet => 1041,
        ExtendedType::Cidr => 651,
        ExtendedType::Macaddr => 1040,
        ExtendedType::Macaddr8 => 775,
        ExtendedType::PgLsn => 3221,
        ExtendedType::Bit { .. } => 1561,
        ExtendedType::VarBit { .. } => 1563,
        ExtendedType::TimeTz { .. } => 1270,
        ExtendedType::TimestampTz { .. } => 1185,
        ExtendedType::Interval { .. } => 1187,
        ExtendedType::Date | ExtendedType::Time { .. } | ExtendedType::Timestamp { .. } => 0,
    }
}

const fn object_id_array_oid(kind: ObjectIdKind) -> u32 {
    use ObjectIdKind as Kind;
    match kind {
        Kind::Oid => 1028,
        Kind::RegProc => 1008,
        Kind::RegProcedure => 2207,
        Kind::RegOper => 2208,
        Kind::RegOperator => 2209,
        Kind::RegClass => 2210,
        Kind::RegType => 2211,
        Kind::RegConfig => 3735,
        Kind::RegDictionary => 3770,
        Kind::RegNamespace => 4090,
        Kind::RegRole => 4097,
        Kind::RegCollation => 4192,
    }
}

#[doc(hidden)]
pub fn builtin_for_oid(oid: u32) -> Option<&'static BuiltinType> {
    BUILTIN_TYPES.iter().find(|metadata| metadata.oid == oid)
}

/// Returns the PostgreSQL array OID for a selected built-in element type.
#[must_use]
pub fn builtin_array_oid(element_oid: u32) -> Option<u32> {
    builtin_for_oid(element_oid)
        .map(|metadata| metadata.array_oid)
        .filter(|oid| *oid != 0)
}

/// The type a declared parameter OID binds as. An extended family whose
/// operators are incomplete keeps its parameters on the numeric adapter path.
pub fn parameter_builtin_for_oid(oid: u32) -> Option<&'static BuiltinType> {
    builtin_for_oid(oid).filter(|metadata| match metadata.kind {
        BuiltinKind::Unknown => false,
        BuiltinKind::Extended(kind) => kind.binds_parameters(),
        _ => true,
    })
}

impl BuiltinKind {
    #[doc(hidden)]
    pub fn metadata(self) -> &'static BuiltinType {
        BUILTIN_TYPES
            .iter()
            .find(|metadata| metadata.kind == self)
            .expect("every current scalar kind has registered metadata")
    }

    #[doc(hidden)]
    pub const fn data_type(self) -> DataType {
        match self {
            Self::Unknown => DataType::Null,
            Self::Bool => DataType::Boolean,
            Self::Int2 => DataType::SmallInt,
            Self::Int4 => DataType::Int,
            Self::Int8 => DataType::BigInt,
            Self::Float8 => DataType::Float,
            Self::Name => DataType::Name,
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
pub enum TypeMetadata<'a> {
    Scalar(&'static BuiltinType),
    Vector,
    Array(&'a DataType),
    /// A named composite or transient record; its identity is in the descriptor.
    Composite(&'a super::CompositeRef),
    /// A named enum type (#827); its identity is in the descriptor.
    Enum(&'a super::EnumRef),
}

#[doc(hidden)]
pub fn type_metadata(data_type: &DataType) -> TypeMetadata<'_> {
    let kind = match data_type {
        DataType::Null => BuiltinKind::Unknown,
        DataType::Boolean => BuiltinKind::Bool,
        DataType::Name => BuiltinKind::Name,
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
        DataType::Vector(_) => return TypeMetadata::Vector,
        DataType::Array(inner) => return TypeMetadata::Array(inner),
        DataType::Composite(descriptor) => return TypeMetadata::Composite(descriptor),
        DataType::Enum(descriptor) => return TypeMetadata::Enum(descriptor),
        // A domain reports its base type's identity on the wire and in SQL.
        DataType::Domain(descriptor) => return type_metadata(&descriptor.base),
    };
    TypeMetadata::Scalar(kind.metadata())
}

#[cfg(test)]
mod tests {
    use super::super::{DataType, ExtendedType, ObjectIdKind};

    #[test]
    fn should_preserve_postgres_array_oids_for_shared_element_types() {
        // Arrange
        let cases = [
            (DataType::Boolean, 1000),
            (DataType::SmallInt, 1005),
            (DataType::Int, 1007),
            (DataType::BigInt, 1016),
            (DataType::Text, 1009),
            (DataType::Uuid, 2951),
            (DataType::Date, 1182),
            (DataType::Extended(ExtendedType::Numeric(None)), 1231),
            (DataType::ObjectId(ObjectIdKind::RegType), 2211),
        ];

        // Act
        let actual = cases.map(|(element, _)| DataType::Array(Box::new(element)).type_oid());

        // Assert
        let expected = cases.map(|(_, array_oid)| i64::from(array_oid));
        assert_eq!(actual, expected);
    }
}
