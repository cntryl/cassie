//! Checked selection of borrowed spans from a syntax-validated raw document.
use crate::app::CassieError;
use crate::runtime::accounted::compact_json::{OwnedJsonText, Progress as SinkProgress, Sink};
use crate::runtime::accounted::json_arena::raw_decode;
use crate::runtime::QueryExecutionControls;

use super::{object, render, scan};
#[cfg(test)]
use super::{scan_observing, Progress};

#[cfg(test)]
#[path = "json_extract_raw_tests.rs"]
mod tests;

#[derive(Clone, Copy)]
enum Selector<'a> {
    Key(&'a str),
    Index(i64),
    Path(&'a str),
}

enum Output {
    Null,
    Text(OwnedJsonText),
}

pub(in crate::executor::filter) fn evaluate(
    document: &crate::executor::filter::ScalarValue,
    selector: &crate::executor::filter::ScalarValue,
    path: bool,
    as_text: bool,
    jsonb: bool,
    controls: &QueryExecutionControls,
    owner: &crate::executor::filter::EvaluationMemory,
) -> Result<Option<crate::executor::filter::ScalarValue>, crate::executor::QueryError> {
    use crate::executor::filter::ScalarValue;
    use crate::types::Value;
    if matches!(document, ScalarValue::Null) || matches!(selector, ScalarValue::Null) {
        return Ok(Some(ScalarValue::Null));
    }
    let ScalarValue::Json(document) = document else {
        return Ok(None);
    };
    if path {
        if let ScalarValue::Str(text) | ScalarValue::Json(text) = selector {
            let range = scan(document, controls)
                .map_err(render::RenderError::from)
                .map_err(render::RenderError::into_query_error)?;
            let current = &document[range];
            let parsed = match super::super::owned_path::parse(text, controls) {
                Ok(path) => path,
                Err(crate::executor::QueryError::Cassie(CassieError::ArrayInput(_))) => {
                    return Ok(None)
                }
                Err(error) => return Err(error),
            };
            if let Some(path) = parsed {
                super::super::after_owned_path(controls)?;
                let values = path.values();
                let selected = follow(
                    current,
                    values.len(),
                    |index| Ok(values[index].as_deref()),
                    jsonb,
                    controls,
                )?;
                return match selected {
                    Some(value) => output(value, as_text, jsonb, controls, owner).map(Some),
                    None => Ok(Some(ScalarValue::Null)),
                };
            }
            if let Some(path) =
                crate::runtime::accounted::json_arena::parse_text_array_candidate(text, controls)?
            {
                super::super::after_owned_path(controls)?;
                let selected = follow(
                    current,
                    path.len(),
                    |index| path.get(index),
                    jsonb,
                    controls,
                )?;
                return match selected {
                    Some(value) => output(value, as_text, jsonb, controls, owner).map(Some),
                    None => Ok(Some(ScalarValue::Null)),
                };
            }
            return Ok(None);
        }
        let ScalarValue::Array(segments) = selector else {
            return Ok(None);
        };
        for value in segments.elements() {
            controls.check_interruption()?;
            if !matches!(value, Value::String(_) | Value::Null) {
                return Ok(None);
            }
        }
        let range = scan(document, controls)
            .map_err(render::RenderError::from)
            .map_err(render::RenderError::into_query_error)?;
        let values = segments.elements();
        let selected = follow(
            &document[range],
            values.len(),
            |index| {
                Ok(match &values[index] {
                    Value::String(text) => Some(text.as_str()),
                    _ => None,
                })
            },
            jsonb,
            controls,
        )?;
        return match selected {
            Some(value) => output(value, as_text, jsonb, controls, owner).map(Some),
            None => Ok(Some(ScalarValue::Null)),
        };
    }
    let selector = match selector {
        ScalarValue::Str(key) => Selector::Key(key),
        ScalarValue::Int(index) => Selector::Index(*index),
        _ => return Ok(None),
    };
    let range = scan(document, controls)
        .map_err(render::RenderError::from)
        .map_err(render::RenderError::into_query_error)?;
    let Some(value) = select_validated(&document[range], selector, jsonb, controls)
        .map_err(render::RenderError::into_query_error)?
    else {
        return Ok(Some(ScalarValue::Null));
    };
    output(value, as_text, jsonb, controls, owner).map(Some)
}

fn follow<'a, 'b>(
    mut current: &'a str,
    length: usize,
    mut segment: impl FnMut(usize) -> Result<Option<&'b str>, CassieError>,
    jsonb: bool,
    controls: &QueryExecutionControls,
) -> Result<Option<&'a str>, crate::executor::QueryError> {
    for index in 0..length {
        controls.check_interruption()?;
        let Some(segment) = segment(index)? else {
            return Ok(None);
        };
        let Some(next) = select_validated(current, Selector::Path(segment), jsonb, controls)
            .map_err(render::RenderError::into_query_error)?
        else {
            return Ok(None);
        };
        current = next;
    }
    Ok(Some(current))
}

fn output(
    value: &str,
    as_text: bool,
    jsonb: bool,
    controls: &QueryExecutionControls,
    owner: &crate::executor::filter::EvaluationMemory,
) -> Result<crate::executor::filter::ScalarValue, crate::executor::QueryError> {
    use crate::executor::filter::ScalarValue;
    match render_selected(value, as_text, jsonb, controls)
        .map_err(render::RenderError::into_query_error)?
    {
        Output::Null => Ok(ScalarValue::Null),
        Output::Text(output) => {
            let (text, memory) = output.into_parts();
            if !memory.belongs_to(controls) || !owner.belongs_to(controls) {
                drop(text);
                drop(memory);
                return Err(CassieError::ResourceLimit(
                    "JSON extraction owner uses a different query tracker".into(),
                )
                .into());
            }
            owner.adopt(memory);
            Ok(if as_text {
                ScalarValue::Str(text)
            } else {
                ScalarValue::Json(text)
            })
        }
    }
}

fn render_selected(
    raw: &str,
    as_text: bool,
    jsonb: bool,
    controls: &QueryExecutionControls,
) -> Result<Output, render::RenderError> {
    render_selected_observing(raw, as_text, jsonb, controls, |_| {})
}

fn render_selected_observing(
    raw: &str,
    as_text: bool,
    jsonb: bool,
    controls: &QueryExecutionControls,
    mut observe: impl FnMut(SinkProgress),
) -> Result<Output, render::RenderError> {
    if as_text && raw.starts_with('n') {
        return Ok(Output::Null);
    }
    if as_text && raw.starts_with('"') {
        let decoded =
            raw_decode::decode(raw, controls).map_err(|error| match error.into_control() {
                Ok(error) => render::RenderError::Control(error),
                Err(error) => render::RenderError::Path(crate::executor::QueryError::General(
                    error.to_string(),
                )),
            })?;
        let mut sink = Sink::for_arena(controls)?;
        sink.write(decoded.get(), &mut observe)?;
        return Ok(Output::Text(sink.finish_output()));
    }
    if jsonb {
        return render::render_value(raw, controls).map(Output::Text);
    }
    let mut sink = Sink::for_arena(controls)?;
    sink.write(raw, &mut observe)?;
    Ok(Output::Text(sink.finish_output()))
}

#[cfg(test)]
fn select<'a>(
    source: &'a str,
    selector: Selector<'_>,
    jsonb: bool,
    controls: &QueryExecutionControls,
) -> Result<Option<&'a str>, render::RenderError> {
    select_observing(source, selector, jsonb, controls, |_| {})
}

#[cfg(test)]
fn select_observing<'a>(
    source: &'a str,
    selector: Selector<'_>,
    jsonb: bool,
    controls: &QueryExecutionControls,
    observe: impl FnMut(Progress),
) -> Result<Option<&'a str>, render::RenderError> {
    let range = scan_observing(source, controls, observe)?;
    let token = &source[range];
    select_validated(token, selector, jsonb, controls)
}

fn select_validated<'a>(
    token: &'a str,
    selector: Selector<'_>,
    jsonb: bool,
    controls: &QueryExecutionControls,
) -> Result<Option<&'a str>, render::RenderError> {
    match (token.as_bytes().first(), selector) {
        (Some(b'{'), Selector::Key(key) | Selector::Path(key)) => {
            let fields = object::parse(token, controls)?;
            Ok(fields.last_value(key, controls)?)
        }
        (Some(b'['), Selector::Index(index)) => array_element(token, index, controls),
        (Some(b'['), Selector::Path(index)) => match parse_index(index, controls)? {
            Some(index) => array_element(token, i64::from(index), controls),
            None => Ok(None),
        },
        (Some(byte), Selector::Index(0 | -1)) if jsonb && *byte != b'{' && *byte != b'[' => {
            Ok(Some(token))
        }
        _ => Ok(None),
    }
}

fn array_element<'a>(
    source: &'a str,
    index: i64,
    controls: &QueryExecutionControls,
) -> Result<Option<&'a str>, render::RenderError> {
    let length = render::array_count(source, controls)?;
    let length = i64::try_from(length).unwrap_or(i64::MAX);
    let target = if index < 0 {
        index.checked_add(length)
    } else {
        Some(index)
    };
    let Some(target) = target.and_then(|value| usize::try_from(value).ok()) else {
        return Ok(None);
    };
    if target >= usize::try_from(length).unwrap_or(usize::MAX) {
        return Ok(None);
    }
    let mut scanner = render::array_scanner(source, controls)?;
    let _consumed = scanner.next()?;
    for position in 0..=target {
        scanner.whitespace()?;
        if position > 0 {
            let _consumed = scanner.next()?;
        }
        let range = scanner.value_span()?;
        if position == target {
            return Ok(Some(&source[range]));
        }
    }
    Ok(None)
}

fn parse_index(text: &str, controls: &QueryExecutionControls) -> Result<Option<i32>, CassieError> {
    controls.check_interruption()?;
    let bytes = text.as_bytes();
    let mut cursor = 0;
    while matches!(
        bytes.get(cursor),
        Some(b' ' | b'\t' | b'\n' | b'\r' | b'\x0b' | b'\x0c')
    ) {
        controls.check_interruption()?;
        cursor += 1;
    }
    let negative = match bytes.get(cursor) {
        Some(b'-') => {
            cursor += 1;
            true
        }
        Some(b'+') => {
            cursor += 1;
            false
        }
        _ => false,
    };
    let start = cursor;
    let mut value = 0_i64;
    let maximum = if negative {
        2_147_483_648
    } else {
        2_147_483_647
    };
    while let Some(byte) = bytes.get(cursor) {
        controls.check_interruption()?;
        if !byte.is_ascii_digit() {
            return Ok(None);
        }
        let Some(next) = value
            .checked_mul(10)
            .and_then(|n| n.checked_add(i64::from(*byte - b'0')))
            .filter(|n| *n <= maximum)
        else {
            return Ok(None);
        };
        value = next;
        cursor += 1;
    }
    if cursor == start {
        return Ok(None);
    }
    let signed = if negative { -value } else { value };
    Ok(i32::try_from(signed).ok())
}
