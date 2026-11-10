//! Borrowed-root and owned-nested sources for the isolated parser foundation.
use crate::app::CassieError;
use crate::runtime::QueryExecutionControls;

use super::cursor::{string::DecodedText, Cursor, CursorError, Position};
use super::directory::Directory;

#[cfg(test)]
#[path = "sources_tests.rs"]
mod tests;

enum Source<'a> {
    Borrowed(&'a [u8]),
    Owned(DecodedText),
}

impl Source<'_> {
    fn bytes(&self) -> &[u8] {
        match self {
            Self::Borrowed(source) => source,
            Self::Owned(source) => source.as_str().as_bytes(),
        }
    }
}

struct SourceFrame<'a> {
    source: Source<'a>,
    position: Position,
    remaining_depth: usize,
}

impl<'a> SourceFrame<'a> {
    fn new(source: Source<'a>) -> Self {
        Self {
            source,
            position: Position {
                offset: 0,
                line: 1,
                column: 0,
            },
            remaining_depth: 128,
        }
    }

    fn step<'cursor, T>(
        &'cursor mut self,
        controls: &QueryExecutionControls,
        action: impl FnOnce(&mut Cursor<'cursor>) -> Result<T, CursorError>,
    ) -> Result<T, CursorError> {
        let mut cursor = Cursor::restore_validated(self.source.bytes(), self.position, controls);
        let result = action(&mut cursor);
        self.position = cursor.position();
        result
    }
}

#[derive(Clone, Copy)]
enum SourceProgress {
    Moved,
}

pub(super) struct SourceStack<'a> {
    frames: Directory<SourceFrame<'a>>,
    controls: QueryExecutionControls,
    length: usize,
    allocated: usize,
}

impl<'a> SourceStack<'a> {
    #[cfg(test)]
    pub(super) fn from_str(
        source: &'a str,
        controls: &QueryExecutionControls,
    ) -> Result<Self, CassieError> {
        Self::from_validated(source.as_bytes(), controls)
    }

    pub(super) fn from_bytes(
        source: &'a [u8],
        controls: &QueryExecutionControls,
    ) -> Result<Self, CursorError> {
        let _ = Cursor::from_bytes(source, controls)?;
        Self::from_validated(source, controls).map_err(CursorError::Interrupted)
    }

    fn from_validated(
        source: &'a [u8],
        controls: &QueryExecutionControls,
    ) -> Result<Self, CassieError> {
        let mut frames = Directory::new(controls)?;
        frames.push(SourceFrame::new(Source::Borrowed(source)))?;
        Ok(Self {
            frames,
            controls: controls.clone(),
            length: 1,
            allocated: 1,
        })
    }

    pub(super) fn push_owned(&mut self, source: DecodedText) -> Result<(), CassieError> {
        self.push_owned_with_observer(source, |_| {})
    }

    #[cfg(test)]
    fn push_owned_observed(
        &mut self,
        source: DecodedText,
        observer: impl FnMut(SourceProgress),
    ) -> Result<(), CassieError> {
        self.push_owned_with_observer(source, observer)
    }

    fn push_owned_with_observer(
        &mut self,
        source: DecodedText,
        mut observer: impl FnMut(SourceProgress),
    ) -> Result<(), CassieError> {
        self.controls.check_interruption()?;
        if !source.shares_tracker(&self.controls) {
            return Err(CassieError::Execution(
                "JSON source uses a different query tracker".into(),
            ));
        }
        let next_length = self
            .length
            .checked_add(1)
            .ok_or_else(|| CassieError::ResourceLimit("JSON source stack overflow".into()))?;
        let frame = SourceFrame::new(Source::Owned(source));
        if self.length < self.allocated {
            self.frames
                .put_empty(self.length, frame)
                .map_err(|_| CassieError::Execution("occupied JSON source frame".into()))?;
        } else {
            self.frames.push_observed(frame, |progress| {
                if progress.is_moved() {
                    observer(SourceProgress::Moved);
                }
                Ok(())
            })?;
            self.allocated = next_length;
        }
        self.length = next_length;
        Ok(())
    }

    pub(super) fn step<'cursor, T>(
        &'cursor mut self,
        action: impl FnOnce(&mut Cursor<'cursor>) -> Result<T, CursorError>,
    ) -> Result<T, CursorError> {
        self.frames
            .get_mut(self.length - 1)
            .expect("active source")
            .step(&self.controls, action)
    }

    pub(super) fn enter_container(&mut self) -> Result<bool, CassieError> {
        self.controls.check_interruption()?;
        let frame = self.frames.get_mut(self.length - 1).expect("active source");
        if frame.remaining_depth <= 1 {
            return Ok(false);
        }
        frame.remaining_depth -= 1;
        Ok(true)
    }

    pub(super) fn leave_container(&mut self) {
        let frame = self.frames.get_mut(self.length - 1).expect("active source");
        frame.remaining_depth += 1;
    }

    pub(super) fn pop(&mut self) -> Result<(), CassieError> {
        self.controls.check_interruption()?;
        if self.length == 1 {
            return Err(CassieError::Execution("cannot pop root JSON source".into()));
        }
        let frame = self.frames.take(self.length - 1).expect("active source");
        self.length -= 1;
        drop(frame);
        Ok(())
    }

    pub(super) fn has_nested(&self) -> bool {
        self.length > 1
    }
}

#[cfg(test)]
impl SourceStack<'_> {
    fn test_position(&self) -> Position {
        self.frames
            .get(self.length - 1)
            .expect("active source")
            .position
    }

    fn test_set_remaining(&mut self, remaining: usize) {
        self.frames
            .get_mut(self.length - 1)
            .expect("active source")
            .remaining_depth = remaining;
    }

    fn test_remaining(&self) -> usize {
        self.frames
            .get(self.length - 1)
            .expect("active source")
            .remaining_depth
    }

    fn test_len(&self) -> usize {
        self.length
    }

    fn test_owned_text(&self, index: usize) -> Option<&str> {
        match &self.frames.get(index)?.source {
            Source::Borrowed(_) => None,
            Source::Owned(text) => Some(text.as_str()),
        }
    }
}
