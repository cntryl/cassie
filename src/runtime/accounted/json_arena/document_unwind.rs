//! Binary-COPY-style checked end checks after a JSON visitor failure.
use super::{Document, DocumentError, DocumentProgress, Phase};
use crate::runtime::accounted::json_arena::cursor::CursorError;

impl Document<'_> {
    pub(super) fn unwind_binary(
        &mut self,
        mut error: DocumentError,
        observer: &mut impl FnMut(DocumentProgress),
    ) -> DocumentError {
        if matches!(&error, DocumentError::Control(_)) {
            return error;
        }
        while let Some(phase) = self.frames.top().map(|frame| frame.phase) {
            if matches!(phase, Phase::RawWaiting) {
                error = error.wrap_nested();
                if let Err(control) = self.sources.pop() {
                    return DocumentError::Control(control);
                }
            }
            let frame = self.frames.pop();
            self.sources.leave_container();
            let already_checked = matches!(phase, Phase::NumberEndChecked | Phase::RawEndChecked);
            if !already_checked {
                let array = matches!(
                    phase,
                    Phase::ArrayFirst | Phase::ArrayValue | Phase::ArrayAfter
                );
                if let Err(control) = self.unwind_end(array, observer) {
                    return DocumentError::from(control);
                }
            }
            drop(frame);
        }
        debug_assert!(!self.sources.has_nested());
        error
    }

    fn unwind_end(
        &mut self,
        array: bool,
        observer: &mut impl FnMut(DocumentProgress),
    ) -> Result<(), CursorError> {
        let next = self.unwind_whitespace(observer)?;
        if next == Some(if array { b']' } else { b'}' }) {
            self.unwind_consume(observer)?;
        } else if array && next == Some(b',') {
            self.unwind_consume(observer)?;
            let _ = self.unwind_whitespace(observer)?;
        }
        Ok(())
    }

    fn unwind_whitespace(
        &mut self,
        observer: &mut impl FnMut(DocumentProgress),
    ) -> Result<Option<u8>, CursorError> {
        let controls = self.controls.clone();
        self.sources.step(|cursor| {
            controls
                .check_interruption()
                .map_err(CursorError::Interrupted)?;
            while matches!(cursor.peek(), Some(b' ' | b'\n' | b'\t' | b'\r')) {
                let _ = cursor.next()?;
                observer(DocumentProgress::UnwindByte(cursor.position()));
                controls
                    .check_interruption()
                    .map_err(CursorError::Interrupted)?;
            }
            Ok(cursor.peek())
        })
    }

    fn unwind_consume(
        &mut self,
        observer: &mut impl FnMut(DocumentProgress),
    ) -> Result<(), CursorError> {
        let controls = self.controls.clone();
        self.sources.step(|cursor| {
            let _ = cursor.next()?;
            observer(DocumentProgress::UnwindByte(cursor.position()));
            controls
                .check_interruption()
                .map_err(CursorError::Interrupted)
        })
    }
}
