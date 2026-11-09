//! Isolated ordinary JSON grammar; sentinel and consumer semantics are pending.
use crate::app::CassieError;
use crate::runtime::QueryExecutionControls;

use super::cursor::{
    literal::Literal, number::NumberDiagnostic, string::DecodedText, Cursor, CursorError, Position,
};
use super::directory::Directory;
use super::forest::Forest;
use super::slots::NodeId;
use super::sources::SourceStack;

#[path = "sentinel_type.rs"]
mod sentinel_type;
use sentinel_type::{Expected, TypeDiagnostic};
#[path = "document_binary.rs"]
pub(super) mod binary;
#[path = "document_diagnostic.rs"]
pub(super) mod diagnostic;
#[path = "document_jsonb.rs"]
pub(super) mod jsonb;
#[path = "document_unwind.rs"]
mod unwind;

#[path = "document_text_array.rs"]
pub(super) mod text_array;

#[path = "document_comparison.rs"]
pub(super) mod comparison;

#[path = "document_exists.rs"]
pub(super) mod exists;

#[cfg(test)]
#[path = "document_tests.rs"]
mod tests;

#[derive(Debug, Clone, Copy)]
enum SyntaxKind {
    EofValue,
    EofList,
    EofObject,
    ExpectedValue,
    ExpectedListEnd,
    ExpectedObjectEnd,
    ExpectedKey,
    ExpectedColon,
    TrailingComma,
    TrailingCharacters,
    Recursion,
}

#[derive(Debug)]
enum DocumentError {
    Control(CassieError),
    Token(CursorError),
    Syntax(SyntaxKind, Position),
    NumberSentinel(NumberDiagnostic),
    SentinelType(TypeDiagnostic),
    Wrapped(WrappedCause),
}

#[derive(Debug)]
enum WrappedCause {
    Syntax(SyntaxKind, Position),
    Token(CursorError),
    Number(NumberDiagnostic),
    Type(TypeDiagnostic),
}

impl DocumentError {
    fn wrap_nested(self) -> Self {
        match self {
            Self::Syntax(kind, position) => Self::Wrapped(WrappedCause::Syntax(kind, position)),
            Self::Token(error) => Self::Wrapped(WrappedCause::Token(error)),
            Self::NumberSentinel(error) => Self::Wrapped(WrappedCause::Number(error)),
            Self::SentinelType(error) => Self::Wrapped(WrappedCause::Type(error)),
            other @ (Self::Control(_) | Self::Wrapped(_)) => other,
        }
    }
}

impl From<CassieError> for DocumentError {
    fn from(error: CassieError) -> Self {
        Self::Control(error)
    }
}

impl From<CursorError> for DocumentError {
    fn from(error: CursorError) -> Self {
        match error {
            CursorError::Interrupted(error) => Self::Control(error),
            other => Self::Token(other),
        }
    }
}

#[cfg(test)]
impl DocumentError {
    fn test_diagnostic(&self) -> Option<(serde_json::error::Category, usize, usize)> {
        let (category, position) = match self {
            Self::Token(error) => error.test_info()?,
            Self::NumberSentinel(error) => {
                let (_, position) = error.test_info();
                (serde_json::error::Category::Data, position)
            }
            Self::SentinelType(error) => error.test_info(),
            Self::Wrapped(cause) => (serde_json::error::Category::Data, cause.test_position()?),
            Self::Syntax(kind, position) => {
                let category = if matches!(
                    kind,
                    SyntaxKind::EofValue | SyntaxKind::EofList | SyntaxKind::EofObject
                ) {
                    serde_json::error::Category::Eof
                } else {
                    serde_json::error::Category::Syntax
                };
                (category, *position)
            }
            Self::Control(_) => return None,
        };
        Some((category, position.line, position.column))
    }

    fn test_number_message(&self) -> Option<String> {
        match self {
            Self::NumberSentinel(error) => Some(error.test_message()),
            Self::SentinelType(error) => Some(error.test_message()),
            _ => None,
        }
    }

    fn test_message(&self) -> Option<String> {
        match self {
            Self::Wrapped(cause) => cause.test_message(),
            Self::NumberSentinel(error) => Some(error.test_message()),
            Self::SentinelType(error) => Some(error.test_message()),
            _ => None,
        }
    }
}

#[cfg(test)]
impl WrappedCause {
    fn test_position(&self) -> Option<Position> {
        match self {
            Self::Syntax(_, position) => Some(*position),
            Self::Token(error) => error.test_info().map(|(_, position)| position),
            Self::Number(error) => Some(error.test_info().1),
            Self::Type(error) => Some(error.test_info().1),
        }
    }

    fn test_message(&self) -> Option<String> {
        match self {
            Self::Syntax(kind, position) => {
                let message = match kind {
                    SyntaxKind::EofValue => "EOF while parsing a value",
                    SyntaxKind::EofList => "EOF while parsing a list",
                    SyntaxKind::EofObject => "EOF while parsing an object",
                    SyntaxKind::ExpectedValue => "expected value",
                    SyntaxKind::ExpectedListEnd => "expected `,` or `]`",
                    SyntaxKind::ExpectedObjectEnd => "expected `,` or `}`",
                    SyntaxKind::ExpectedKey => "key must be a string",
                    SyntaxKind::ExpectedColon => "expected `:`",
                    SyntaxKind::TrailingComma => "trailing comma",
                    SyntaxKind::TrailingCharacters => "trailing characters",
                    SyntaxKind::Recursion => "recursion limit exceeded",
                };
                Some(format!(
                    "{message} at line {} column {}",
                    position.line, position.column
                ))
            }
            Self::Token(error) => error.test_message(),
            Self::Number(error) => Some(error.test_message()),
            Self::Type(error) => Some(error.test_message()),
        }
    }
}

#[derive(Clone, Copy)]
enum Phase {
    ArrayFirst,
    ArrayValue,
    ArrayAfter,
    ObjectFirst,
    ObjectKey,
    ObjectValue,
    ObjectAfter,
    NumberSentinel,
    RawSentinel,
    RawWaiting,
    NumberEndChecked,
    RawEndChecked,
}

struct Frame {
    node: NodeId,
    phase: Phase,
    key: Option<DecodedText>,
}

struct Frames {
    entries: Directory<Frame>,
    length: usize,
    allocated: usize,
}

impl Frames {
    fn new(controls: &QueryExecutionControls) -> Result<Self, CassieError> {
        Ok(Self {
            entries: Directory::new(controls)?,
            length: 0,
            allocated: 0,
        })
    }

    fn push(&mut self, frame: Frame) -> Result<(), CassieError> {
        let next = self
            .length
            .checked_add(1)
            .ok_or_else(|| CassieError::ResourceLimit("JSON grammar stack overflow".into()))?;
        if self.length < self.allocated {
            self.entries
                .put_empty(self.length, frame)
                .map_err(|_| CassieError::Execution("occupied JSON grammar frame".into()))?;
        } else {
            self.entries.push(frame)?;
            self.allocated = next;
        }
        self.length = next;
        Ok(())
    }

    fn top(&self) -> Option<&Frame> {
        self.length
            .checked_sub(1)
            .and_then(|index| self.entries.get(index))
    }

    fn top_mut(&mut self) -> Option<&mut Frame> {
        self.length
            .checked_sub(1)
            .and_then(|index| self.entries.get_mut(index))
    }

    fn pop(&mut self) -> Frame {
        self.length -= 1;
        self.entries
            .take(self.length)
            .expect("active grammar frame")
    }
}

#[derive(Clone, Copy)]
enum DocumentProgress {
    FramePushed,
    NumberSourceReady,
    NumberDecoded,
    NumberTypeScanned,
    RawSourcePushed,
    RawValueReady,
    UnwindByte(Position),
}

pub(super) struct Parsed {
    pub(super) forest: Forest,
    pub(super) root: NodeId,
}

pub(super) struct Document<'a> {
    sources: SourceStack<'a>,
    frames: Frames,
    forest: Forest,
    controls: QueryExecutionControls,
}

impl<'a> Document<'a> {
    #[cfg(test)]
    fn parse(input: &'a str, controls: &QueryExecutionControls) -> Result<Parsed, DocumentError> {
        Self::parse_with_observer(input, controls, |_| {})
    }

    #[cfg(test)]
    pub(super) fn parse_for_output(
        input: &'a str,
        controls: &QueryExecutionControls,
    ) -> Result<Parsed, CassieError> {
        Self::parse(input, controls)
            .map_err(|_| CassieError::Execution("JSON output fixture failed to parse".into()))
    }

    #[cfg(test)]
    fn parse_observed(
        input: &'a str,
        controls: &QueryExecutionControls,
        observer: impl FnMut(DocumentProgress),
    ) -> Result<Parsed, DocumentError> {
        Self::parse_with_observer(input, controls, observer)
    }

    #[cfg(test)]
    fn parse_with_observer(
        input: &'a str,
        controls: &QueryExecutionControls,
        observer: impl FnMut(DocumentProgress),
    ) -> Result<Parsed, DocumentError> {
        let sources = SourceStack::from_str(input, controls)?;
        Self::parse_sources(sources, controls, observer)
    }

    fn parse_sources(
        sources: SourceStack<'a>,
        controls: &QueryExecutionControls,
        mut observer: impl FnMut(DocumentProgress),
    ) -> Result<Parsed, DocumentError> {
        let mut document = Self {
            sources,
            frames: Frames::new(controls)?,
            forest: Forest::new(controls)?,
            controls: controls.clone(),
        };
        let root = match document.run(&mut observer) {
            Ok(root) => root,
            Err(error) => return Err(document.unwind_binary(error, &mut observer)),
        };
        document.forest.mark_root(root)?;
        Ok(Parsed {
            forest: document.forest,
            root,
        })
    }

    fn run(
        &mut self,
        observer: &mut impl FnMut(DocumentProgress),
    ) -> Result<NodeId, DocumentError> {
        let mut pending = self.begin_value(observer)?;
        loop {
            self.controls.check_interruption()?;
            if let Some(node) = pending.take() {
                if matches!(
                    self.frames.top().map(|frame| frame.phase),
                    Some(Phase::RawWaiting)
                ) {
                    pending = Some(self.finish_raw(node, observer)?);
                    continue;
                }
                let Some(frame) = self.frames.top_mut() else {
                    if self.whitespace()?.is_some() {
                        return Err(self.syntax(SyntaxKind::TrailingCharacters)?);
                    }
                    return Ok(node);
                };
                match frame.phase {
                    Phase::ArrayValue => {
                        self.forest.append(frame.node, node)?;
                        frame.phase = Phase::ArrayAfter;
                    }
                    Phase::ObjectValue => {
                        let key = frame.key.take().expect("object key");
                        if let Some(old) = self.forest.insert_decoded(frame.node, key, node)? {
                            self.forest.retire_subtree(old)?;
                        }
                        frame.phase = Phase::ObjectAfter;
                    }
                    _ => unreachable!("completed child has a waiting parent"),
                }
                continue;
            }
            let phase = self.frames.top().expect("open container").phase;
            match phase {
                Phase::ArrayFirst => match self.whitespace()? {
                    Some(b']') => pending = Some(self.close_container()?),
                    None => return Err(self.syntax(SyntaxKind::EofList)?),
                    Some(_) => self.frames.top_mut().expect("frame").phase = Phase::ArrayValue,
                },
                Phase::ArrayValue | Phase::ObjectValue => pending = self.begin_value(observer)?,
                Phase::NumberSentinel => pending = Some(self.read_number_sentinel(observer)?),
                Phase::RawSentinel => pending = self.start_raw_sentinel(observer)?,
                Phase::RawWaiting => unreachable!("nested value must complete before raw map"),
                Phase::NumberEndChecked | Phase::RawEndChecked => {
                    unreachable!("completed sentinel end check cannot resume")
                }
                Phase::ArrayAfter => match self.whitespace()? {
                    Some(b']') => pending = Some(self.close_container()?),
                    Some(b',') => {
                        self.consume()?;
                        match self.whitespace()? {
                            Some(b']') => return Err(self.syntax(SyntaxKind::TrailingComma)?),
                            None => return Err(self.syntax(SyntaxKind::EofValue)?),
                            Some(_) => {
                                self.frames.top_mut().expect("frame").phase = Phase::ArrayValue
                            }
                        }
                    }
                    None => return Err(self.syntax(SyntaxKind::EofList)?),
                    Some(_) => return Err(self.syntax(SyntaxKind::ExpectedListEnd)?),
                },
                Phase::ObjectFirst => {
                    if self.whitespace()? == Some(b'}') {
                        pending = Some(self.close_container()?);
                    } else {
                        self.read_key(phase)?;
                    }
                }
                Phase::ObjectKey => self.read_key(phase)?,
                Phase::ObjectAfter => match self.whitespace()? {
                    Some(b'}') => pending = Some(self.close_container()?),
                    Some(b',') => {
                        self.consume()?;
                        match self.whitespace()? {
                            Some(b'}') => return Err(self.syntax(SyntaxKind::TrailingComma)?),
                            None => return Err(self.syntax(SyntaxKind::EofValue)?),
                            Some(_) => {
                                self.frames.top_mut().expect("frame").phase = Phase::ObjectKey
                            }
                        }
                    }
                    None => return Err(self.syntax(SyntaxKind::EofObject)?),
                    Some(_) => return Err(self.syntax(SyntaxKind::ExpectedObjectEnd)?),
                },
            }
        }
    }

    fn begin_value(
        &mut self,
        observer: &mut impl FnMut(DocumentProgress),
    ) -> Result<Option<NodeId>, DocumentError> {
        match self.whitespace()? {
            None => Err(self.syntax(SyntaxKind::EofValue)?),
            Some(b'[' | b'{') => {
                let opener = self
                    .sources
                    .step(|cursor| Ok(cursor.peek()))?
                    .expect("opener");
                if !self.sources.enter_container()? {
                    return Err(self.syntax(SyntaxKind::Recursion)?);
                }
                self.consume()?;
                let (node, phase) = if opener == b'[' {
                    (self.forest.array()?, Phase::ArrayFirst)
                } else {
                    (self.forest.object()?, Phase::ObjectFirst)
                };
                self.frames.push(Frame {
                    node,
                    phase,
                    key: None,
                })?;
                observer(DocumentProgress::FramePushed);
                self.controls.check_interruption()?;
                Ok(None)
            }
            Some(b'"') => {
                let value = self.sources.step(|cursor| cursor.read_string())?;
                Ok(Some(self.forest.adopt_text(value)?))
            }
            Some(b'-' | b'0'..=b'9') => {
                let value = self.sources.step(|cursor| cursor.read_number())?;
                Ok(Some(self.forest.adopt_number(value)?))
            }
            Some(b'n' | b't' | b'f') => {
                let value = self.sources.step(|cursor| cursor.read_literal())?;
                let node = match value {
                    Literal::Null => self.forest.scalar()?,
                    Literal::True => self.forest.boolean(true)?,
                    Literal::False => self.forest.boolean(false)?,
                };
                Ok(Some(node))
            }
            Some(_) => Err(self.syntax(SyntaxKind::ExpectedValue)?),
        }
    }

    fn read_key(&mut self, phase: Phase) -> Result<(), DocumentError> {
        match self.whitespace()? {
            None => return Err(self.syntax(SyntaxKind::EofObject)?),
            Some(b'"') => {}
            Some(_) => return Err(self.syntax(SyntaxKind::ExpectedKey)?),
        }
        let key = self.sources.step(|cursor| cursor.read_string())?;
        let number_sentinel =
            matches!(phase, Phase::ObjectFirst) && key.as_str() == "$serde_json::private::Number";
        let raw_sentinel =
            matches!(phase, Phase::ObjectFirst) && key.as_str() == "$serde_json::private::RawValue";
        match self.whitespace()? {
            Some(b':') => {
                self.consume()?;
            }
            None => return Err(self.syntax(SyntaxKind::EofObject)?),
            Some(_) => return Err(self.syntax(SyntaxKind::ExpectedColon)?),
        }
        let frame = self.frames.top_mut().expect("object frame");
        if number_sentinel {
            frame.phase = Phase::NumberSentinel;
        } else if raw_sentinel {
            frame.phase = Phase::RawSentinel;
        } else {
            frame.key = Some(key);
            frame.phase = Phase::ObjectValue;
        }
        Ok(())
    }

    fn read_number_sentinel(
        &mut self,
        observer: &mut impl FnMut(DocumentProgress),
    ) -> Result<NodeId, DocumentError> {
        match self.whitespace()? {
            Some(b'"') => {}
            None => return Err(self.syntax(SyntaxKind::EofValue)?),
            Some(_) => return Err(self.unexpected_sentinel_type(Expected::NumberString, observer)?),
        }
        let source = self.sources.step(|cursor| cursor.read_string())?;
        observer(DocumentProgress::NumberSourceReady);
        self.controls.check_interruption()?;
        let number = {
            let mut cursor = Cursor::from_str(source.as_str(), &self.controls);
            match cursor.read_strict_number() {
                Ok(number) => number,
                Err(CursorError::Number(error)) => {
                    return Err(DocumentError::NumberSentinel(error))
                }
                Err(error) => return Err(DocumentError::from(error)),
            }
        };
        observer(DocumentProgress::NumberDecoded);
        self.controls.check_interruption()?;
        drop(source);
        let node = self.forest.adopt_number(number)?;
        self.frames.top_mut().expect("number frame").phase = Phase::NumberEndChecked;
        match self.whitespace()? {
            Some(b'}') => {
                let wrapper = self.close_container()?;
                self.forest.retire_subtree(wrapper)?;
                Ok(node)
            }
            Some(b',') => Err(self.syntax(SyntaxKind::TrailingComma)?),
            Some(_) => Err(self.syntax(SyntaxKind::TrailingCharacters)?),
            None => Err(self.syntax(SyntaxKind::EofObject)?),
        }
    }

    fn start_raw_sentinel(
        &mut self,
        observer: &mut impl FnMut(DocumentProgress),
    ) -> Result<Option<NodeId>, DocumentError> {
        match self.whitespace()? {
            Some(b'"') => {}
            None => return Err(self.syntax(SyntaxKind::EofValue)?),
            Some(_) => return Err(self.unexpected_sentinel_type(Expected::RawValue, observer)?),
        }
        let source = self.sources.step(|cursor| cursor.read_string())?;
        self.sources.push_owned(source)?;
        self.frames.top_mut().expect("raw frame").phase = Phase::RawWaiting;
        observer(DocumentProgress::RawSourcePushed);
        self.controls.check_interruption()?;
        self.begin_value(observer)
    }

    fn finish_raw(
        &mut self,
        node: NodeId,
        observer: &mut impl FnMut(DocumentProgress),
    ) -> Result<NodeId, DocumentError> {
        if self.whitespace()?.is_some() {
            return Err(self.syntax(SyntaxKind::TrailingCharacters)?);
        }
        observer(DocumentProgress::RawValueReady);
        self.controls.check_interruption()?;
        self.sources.pop()?;
        self.frames.top_mut().expect("raw frame").phase = Phase::RawEndChecked;
        match self.whitespace()? {
            Some(b'}') => {
                let wrapper = self.close_container()?;
                self.forest.retire_subtree(wrapper)?;
                Ok(node)
            }
            Some(b',') => Err(self.syntax(SyntaxKind::TrailingComma)?),
            Some(_) => Err(self.syntax(SyntaxKind::TrailingCharacters)?),
            None => Err(self.syntax(SyntaxKind::EofObject)?),
        }
    }

    fn close_container(&mut self) -> Result<NodeId, DocumentError> {
        self.consume()?;
        self.sources.leave_container();
        Ok(self.frames.pop().node)
    }

    fn whitespace(&mut self) -> Result<Option<u8>, DocumentError> {
        self.sources
            .step(|cursor| cursor.skip_whitespace())
            .map_err(Into::into)
    }

    fn consume(&mut self) -> Result<(), DocumentError> {
        let _ = self
            .sources
            .step(|cursor| cursor.next())
            .map_err(DocumentError::from)?;
        Ok(())
    }

    fn syntax(&mut self, kind: SyntaxKind) -> Result<DocumentError, DocumentError> {
        let position = self.sources.step(|cursor| cursor.peek_position())?;
        Ok(DocumentError::Syntax(kind, position))
    }
}
