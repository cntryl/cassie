//! Isolated ownership forest; tokenizer, subtree retirement and consumers are pending.
use crate::app::CassieError;
use crate::runtime::{accounted::Accounted, QueryExecutionControls};

use super::cursor::number::NumberToken;
use super::cursor::string::DecodedText;
use super::object::{Key, ObjectIndex};
use super::slots::{NodeId, ValueSlots};

#[path = "array.rs"]
mod array;
use array::ArrayIndex;
#[path = "scalar.rs"]
mod scalar;
use scalar::OwnedText;
#[path = "jsonb_comparison.rs"]
pub(super) mod comparison;
#[path = "jsonb_number.rs"]
pub(super) mod jsonb_number;
#[path = "jsonb_order.rs"]
mod jsonb_order;
#[path = "jsonb_output.rs"]
pub(super) mod jsonb_output;
#[path = "json_object_keys.rs"]
pub(super) mod object_keys;
#[path = "output.rs"]
pub(super) mod output;

#[path = "retirement.rs"]
mod retirement;
#[cfg(test)]
#[path = "forest_tests.rs"]
mod tests;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Owner {
    Unattached,
    Root,
    Parent(NodeId),
}

enum Kind {
    Scalar,
    Boolean(bool),
    Text(OwnedText),
    Number(OwnedText),
    Object(ObjectIndex<NodeId>),
    Array(ArrayIndex),
}

struct Node {
    kind: Kind,
    owner: Owner,
}

#[derive(Clone, Copy)]
enum Progress {
    ParentWalk,
    #[cfg(test)]
    BeforeInsert,
    RetirementVisit,
    RetirementMove,
    ArrayMove,
    #[cfg(test)]
    TextCopy,
}

#[derive(Clone, Copy)]
pub(crate) enum InspectionRoot {
    Null,
    Boolean,
    String,
    Number,
    Array(usize),
    Object,
}

pub(super) struct Forest {
    nodes: ValueSlots<Node>,
    controls: QueryExecutionControls,
}

pub(super) enum TextArrayElement<'a> {
    Text(&'a str),
    Null,
    Other,
}

impl Forest {
    pub(super) fn inspection_root(&self, root: NodeId) -> Result<InspectionRoot, CassieError> {
        Ok(match &self.nodes.get(root)?.kind {
            Kind::Scalar => InspectionRoot::Null,
            Kind::Boolean(_) => InspectionRoot::Boolean,
            Kind::Text(_) => InspectionRoot::String,
            Kind::Number(_) => InspectionRoot::Number,
            Kind::Array(array) => InspectionRoot::Array(array.len()),
            Kind::Object(_) => InspectionRoot::Object,
        })
    }
    pub(super) fn key_exists(&self, root: NodeId, key: &str) -> Result<bool, CassieError> {
        self.key_exists_observing(root, key, || {})
    }

    pub(super) fn key_exists_observing(
        &self,
        root: NodeId,
        key: &str,
        mut observe: impl FnMut(),
    ) -> Result<bool, CassieError> {
        self.controls.check_interruption()?;
        match &self.nodes.get(root)?.kind {
            Kind::Object(index) => Ok(index.find(key)?.is_some()),
            Kind::Array(index) => {
                for position in 0..index.len() {
                    self.controls.check_interruption()?;
                    let child = index.get(position).ok_or_else(invalid_owner)?;
                    if let Kind::Text(text) = &self.nodes.get(child)?.kind {
                        let equal = self.equal_text(text.as_str(), key)?;
                        observe();
                        self.controls.check_interruption()?;
                        if equal {
                            return Ok(true);
                        }
                    } else {
                        observe();
                        self.controls.check_interruption()?;
                    }
                }
                Ok(false)
            }
            Kind::Text(text) => self.equal_text(text.as_str(), key),
            Kind::Scalar | Kind::Boolean(_) | Kind::Number(_) => Ok(false),
        }
    }

    fn equal_text(&self, left: &str, right: &str) -> Result<bool, CassieError> {
        if left.len() != right.len() {
            return Ok(false);
        }
        for (left, right) in left
            .as_bytes()
            .chunks(1024)
            .zip(right.as_bytes().chunks(1024))
        {
            self.controls.check_interruption()?;
            if left != right {
                return Ok(false);
            }
        }
        Ok(true)
    }

    pub(super) fn text_array_len(&self, root: NodeId) -> Result<Option<usize>, CassieError> {
        Ok(match &self.nodes.get(root)?.kind {
            Kind::Array(index) => Some(index.len()),
            _ => None,
        })
    }

    pub(super) fn text_array_element(
        &self,
        root: NodeId,
        position: usize,
    ) -> Result<TextArrayElement<'_>, CassieError> {
        let Kind::Array(index) = &self.nodes.get(root)?.kind else {
            return Err(invalid_owner());
        };
        let child = index.get(position).ok_or_else(invalid_owner)?;
        Ok(match &self.nodes.get(child)?.kind {
            Kind::Text(text) => TextArrayElement::Text(text.as_str()),
            Kind::Scalar => TextArrayElement::Null,
            _ => TextArrayElement::Other,
        })
    }
    pub(super) fn new(controls: &QueryExecutionControls) -> Result<Self, CassieError> {
        Ok(Self {
            nodes: ValueSlots::new(controls)?,
            controls: controls.clone(),
        })
    }

    pub(super) fn scalar(&mut self) -> Result<NodeId, CassieError> {
        self.new_node(Kind::Scalar)
    }

    pub(super) fn boolean(&mut self, value: bool) -> Result<NodeId, CassieError> {
        self.new_node(Kind::Boolean(value))
    }

    #[cfg(test)]
    fn text_node(&mut self, value: &str) -> Result<NodeId, CassieError> {
        self.text_node_with_observer(value, |_| {})
    }

    pub(super) fn adopt_text(&mut self, token: DecodedText) -> Result<NodeId, CassieError> {
        self.adopt_text_with_observer(token, || {})
    }

    #[cfg(test)]
    fn adopt_text_observed(
        &mut self,
        token: DecodedText,
        observer: impl FnMut(),
    ) -> Result<NodeId, CassieError> {
        self.adopt_text_with_observer(token, observer)
    }

    fn adopt_text_with_observer(
        &mut self,
        token: DecodedText,
        mut observer: impl FnMut(),
    ) -> Result<NodeId, CassieError> {
        self.controls.check_interruption()?;
        let text = OwnedText::adopt(token, &self.controls)?;
        observer();
        self.controls.check_interruption()?;
        self.new_node(Kind::Text(text))
    }

    pub(super) fn adopt_number(&mut self, token: NumberToken) -> Result<NodeId, CassieError> {
        self.adopt_number_with_observer(token, || {})
    }

    #[cfg(test)]
    fn adopt_number_observed(
        &mut self,
        token: NumberToken,
        observer: impl FnMut(),
    ) -> Result<NodeId, CassieError> {
        self.adopt_number_with_observer(token, observer)
    }

    fn adopt_number_with_observer(
        &mut self,
        token: NumberToken,
        mut observer: impl FnMut(),
    ) -> Result<NodeId, CassieError> {
        self.controls.check_interruption()?;
        let text = OwnedText::adopt_number(token, &self.controls)?;
        observer();
        self.controls.check_interruption()?;
        self.new_node(Kind::Number(text))
    }

    #[cfg(test)]
    fn text_node_observed(
        &mut self,
        value: &str,
        observer: impl FnMut(Progress),
    ) -> Result<NodeId, CassieError> {
        self.text_node_with_observer(value, observer)
    }

    #[cfg(test)]
    fn text_node_with_observer(
        &mut self,
        value: &str,
        mut observer: impl FnMut(Progress),
    ) -> Result<NodeId, CassieError> {
        let text = OwnedText::copy(value, &self.controls, &mut observer)?;
        self.new_node(Kind::Text(text))
    }

    #[cfg(test)]
    pub(super) fn boolean_value(&self, node: NodeId) -> Result<Option<bool>, CassieError> {
        Ok(match &self.nodes.get(node)?.kind {
            Kind::Boolean(value) => Some(*value),
            _ => None,
        })
    }

    #[cfg(test)]
    pub(super) fn text_value(&self, node: NodeId) -> Result<Option<&str>, CassieError> {
        Ok(match &self.nodes.get(node)?.kind {
            Kind::Text(value) => Some(value.as_str()),
            _ => None,
        })
    }

    #[cfg(test)]
    pub(super) fn number_value(&self, node: NodeId) -> Result<Option<&str>, CassieError> {
        Ok(match &self.nodes.get(node)?.kind {
            Kind::Number(value) => Some(value.as_str()),
            _ => None,
        })
    }

    pub(super) fn object(&mut self) -> Result<NodeId, CassieError> {
        let index = ObjectIndex::new(&self.controls)?;
        self.new_node(Kind::Object(index))
    }

    pub(super) fn array(&mut self) -> Result<NodeId, CassieError> {
        let index = ArrayIndex::new(&self.controls)?;
        self.new_node(Kind::Array(index))
    }

    fn new_node(&mut self, kind: Kind) -> Result<NodeId, CassieError> {
        let memory = self.controls.reserve_query_memory(0)?;
        self.nodes.insert(Accounted::from_admitted(
            Node {
                kind,
                owner: Owner::Unattached,
            },
            memory,
        ))
    }

    #[cfg(test)]
    fn parent(&self, node: NodeId) -> Result<Owner, CassieError> {
        Ok(self.nodes.get(node)?.owner)
    }

    pub(super) fn mark_root(&mut self, node: NodeId) -> Result<(), CassieError> {
        self.controls.check_interruption()?;
        let value = self.nodes.get_mut(node)?;
        if value.owner != Owner::Unattached {
            return Err(invalid_owner());
        }
        value.owner = Owner::Root;
        Ok(())
    }

    #[cfg(test)]
    fn detach_root(&mut self, node: NodeId) -> Result<(), CassieError> {
        self.controls.check_interruption()?;
        let value = self.nodes.get_mut(node)?;
        if value.owner != Owner::Root {
            return Err(invalid_owner());
        }
        value.owner = Owner::Unattached;
        Ok(())
    }

    pub(super) fn member(&self, parent: NodeId, key: &str) -> Result<Option<NodeId>, CassieError> {
        match &self.nodes.get(parent)?.kind {
            Kind::Object(index) => index.find(key),
            Kind::Scalar | Kind::Boolean(_) | Kind::Text(_) | Kind::Number(_) | Kind::Array(_) => {
                Err(invalid_owner())
            }
        }
    }

    #[cfg(test)]
    pub(super) fn element(
        &self,
        parent: NodeId,
        position: usize,
    ) -> Result<Option<NodeId>, CassieError> {
        match &self.nodes.get(parent)?.kind {
            Kind::Array(index) => Ok(index.get(position)),
            Kind::Scalar | Kind::Boolean(_) | Kind::Text(_) | Kind::Number(_) | Kind::Object(_) => {
                Err(invalid_owner())
            }
        }
    }

    pub(super) fn append(&mut self, parent: NodeId, child: NodeId) -> Result<(), CassieError> {
        self.append_with_observer(parent, child, |_| {})
    }

    #[cfg(test)]
    fn append_observed(
        &mut self,
        parent: NodeId,
        child: NodeId,
        observer: impl FnMut(Progress),
    ) -> Result<(), CassieError> {
        self.append_with_observer(parent, child, observer)
    }

    fn append_with_observer(
        &mut self,
        parent: NodeId,
        child: NodeId,
        mut observer: impl FnMut(Progress),
    ) -> Result<(), CassieError> {
        self.validate_attachment(parent, child, &mut observer)?;
        let parent_node = self.nodes.get_mut(parent)?;
        let Kind::Array(index) = &mut parent_node.kind else {
            return Err(invalid_owner());
        };
        index.append(child, &mut observer)?;
        self.nodes.get_mut(child).expect("validated child").owner = Owner::Parent(parent);
        Ok(())
    }

    #[cfg(test)]
    fn insert(
        &mut self,
        parent: NodeId,
        key: &str,
        child: NodeId,
    ) -> Result<Option<NodeId>, CassieError> {
        self.insert_with_observer(parent, key, child, |_| {})
    }

    #[cfg(test)]
    fn insert_observed(
        &mut self,
        parent: NodeId,
        key: &str,
        child: NodeId,
        observer: impl FnMut(Progress),
    ) -> Result<Option<NodeId>, CassieError> {
        self.insert_with_observer(parent, key, child, observer)
    }

    #[cfg(test)]
    fn insert_with_observer(
        &mut self,
        parent: NodeId,
        key: &str,
        child: NodeId,
        mut observer: impl FnMut(Progress),
    ) -> Result<Option<NodeId>, CassieError> {
        self.validate_attachment(parent, child, &mut observer)?;
        let displaced = self.displaced_for(parent, key)?;
        let owned_key = Key::new(key, &self.controls)?;
        observer(Progress::BeforeInsert);
        self.controls.check_interruption()?;
        self.commit_key(parent, child, displaced, owned_key)
    }

    pub(super) fn insert_decoded(
        &mut self,
        parent: NodeId,
        token: DecodedText,
        child: NodeId,
    ) -> Result<Option<NodeId>, CassieError> {
        self.controls.check_interruption()?;
        let owned_key = Key::adopt(token, &self.controls)?;
        self.validate_attachment(parent, child, &mut |_| {})?;
        let displaced = self.displaced_for(parent, owned_key.as_str())?;
        self.controls.check_interruption()?;
        self.commit_key(parent, child, displaced, owned_key)
    }

    fn displaced_for(&self, parent: NodeId, key: &str) -> Result<Option<NodeId>, CassieError> {
        let displaced = self.member(parent, key)?;
        if let Some(old) = displaced {
            if self.nodes.get(old)?.owner != Owner::Parent(parent) {
                return Err(invalid_owner());
            }
        }
        Ok(displaced)
    }

    fn commit_key(
        &mut self,
        parent: NodeId,
        child: NodeId,
        displaced: Option<NodeId>,
        owned_key: Key,
    ) -> Result<Option<NodeId>, CassieError> {
        let parent_node = self.nodes.get_mut(parent)?;
        let Kind::Object(index) = &mut parent_node.kind else {
            return Err(invalid_owner());
        };
        let actual = index.insert(owned_key, child)?;
        debug_assert_eq!(actual, displaced);
        // All identities were checked under this exclusive borrow; nothing below
        // allocates, checks cancellation, or calls user code before publication.
        self.nodes.get_mut(child).expect("validated child").owner = Owner::Parent(parent);
        if let Some(old) = displaced {
            self.nodes
                .get_mut(old)
                .expect("validated displaced node")
                .owner = Owner::Unattached;
        }
        Ok(displaced)
    }

    fn validate_attachment(
        &self,
        parent: NodeId,
        child: NodeId,
        observer: &mut impl FnMut(Progress),
    ) -> Result<(), CassieError> {
        self.controls.check_interruption()?;
        if self.nodes.get(child)?.owner != Owner::Unattached {
            return Err(invalid_owner());
        }
        let mut current = parent;
        loop {
            self.controls.check_interruption()?;
            if current == child {
                return Err(invalid_owner());
            }
            let owner = self.nodes.get(current)?.owner;
            observer(Progress::ParentWalk);
            self.controls.check_interruption()?;
            match owner {
                Owner::Parent(next) => current = next,
                Owner::Unattached | Owner::Root => break,
            }
        }
        Ok(())
    }

    #[cfg(test)]
    fn retire_empty(&mut self, node: NodeId) -> Result<(), CassieError> {
        let value = self.nodes.get(node)?;
        if value.owner != Owner::Unattached
            || !matches!(
                &value.kind,
                Kind::Scalar | Kind::Boolean(_) | Kind::Text(_) | Kind::Number(_)
            )
        {
            return Err(invalid_owner());
        }
        drop(self.nodes.remove(node)?);
        Ok(())
    }
}

fn invalid_owner() -> CassieError {
    CassieError::Execution("invalid JSON arena ownership".into())
}
