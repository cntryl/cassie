use crate::types::composite::CompositeType;

/// The reserved schema that owns the graph types and functions.
pub const GRAPH_SCHEMA: &str = "cassie_graph";

/// The ownership marker stored on the reserved namespace.
pub const GRAPH_SCHEMA_OWNER: &str = "cassie:graph-types:v1";

/// Local name of the typed key component.
pub const KEY_PART_TYPE: &str = "graph_key_part";
/// Local name of the durable element reference.
pub const ELEMENT_REF_TYPE: &str = "graph_element_ref";
/// Local name of the ordered path.
pub const PATH_TYPE: &str = "graph_path";

/// Which reserved composite type a descriptor names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphTypeKind {
    KeyPart,
    ElementRef,
    Path,
}

impl GraphTypeKind {
    /// Every reserved type in dependency order (a type follows the types its
    /// fields use).
    pub const ALL: [Self; 3] = [Self::KeyPart, Self::ElementRef, Self::Path];

    /// The schema-local type name.
    #[must_use]
    pub const fn type_name(self) -> &'static str {
        match self {
            Self::KeyPart => KEY_PART_TYPE,
            Self::ElementRef => ELEMENT_REF_TYPE,
            Self::Path => PATH_TYPE,
        }
    }

    /// The reserved type a descriptor identifies, by qualified name. Only the
    /// bootstrap can create a type in the reserved schema, so the name is the
    /// identity.
    #[must_use]
    pub fn of(descriptor: &CompositeType) -> Option<Self> {
        // The cheap local-name test runs first: this sits on every record cast.
        let kind = Self::ALL
            .into_iter()
            .find(|kind| kind.type_name() == descriptor.name)?;
        (!descriptor.qualified_name.is_empty()
            && crate::catalog::scope::relation_schema_name(&descriptor.qualified_name)
                .eq_ignore_ascii_case(GRAPH_SCHEMA))
        .then_some(kind)
    }
}
