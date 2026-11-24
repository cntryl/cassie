//! Engine-owned, query-controlled JSONB input.

use super::extended::TypeInputError;
use crate::app::CassieError;
use crate::config::CassieRuntimeLimits;
use crate::runtime::QueryExecutionControls;

pub use cassie_core::types::jsonb_format::{format, input_text_len, text_len};

#[doc(hidden)]
pub fn canonical(input: &str) -> Result<String, TypeInputError> {
    canonical_with_controls(input, None).map_err(|error| match error {
        CassieError::TypeInput(error) => error,
        error => TypeInputError::new("54000", error.to_string()),
    })
}

pub(crate) fn canonical_with_controls(
    input: &str,
    controls: Option<&QueryExecutionControls>,
) -> Result<String, CassieError> {
    let local_controls = controls.is_none().then(|| {
        QueryExecutionControls::from_limits(
            &CassieRuntimeLimits::default(),
            std::time::Instant::now(),
        )
    });
    let controls = controls
        .or(local_controls.as_ref())
        .expect("canonical controls");
    let output =
        crate::runtime::accounted::json_arena::canonical_jsonb_text(input.as_bytes(), controls)
            .map_err(|error| match error {
                crate::runtime::accounted::json_arena::JsonbTextError::Invalid => {
                    CassieError::TypeInput(invalid_json_detail(input))
                }
                crate::runtime::accounted::json_arena::JsonbTextError::NumericOverflow => {
                    CassieError::TypeInput(TypeInputError::new(
                        "22003",
                        "value overflows numeric format".into(),
                    ))
                }
                crate::runtime::accounted::json_arena::JsonbTextError::Nul => {
                    CassieError::TypeInput(TypeInputError::new(
                        "22P05",
                        "unsupported Unicode escape sequence: \\u0000".into(),
                    ))
                }
                crate::runtime::accounted::json_arena::JsonbTextError::Control(error) => error,
            })?;
    let (text, memory) = output.into_parts();
    drop(memory);
    Ok(text)
}

fn invalid_json_detail(input: &str) -> TypeInputError {
    TypeInputError::new(
        "22P02",
        format!(
            "invalid input syntax for type jsonb: {}",
            crate::runtime::accounted::json_arena::syntax_error(input.as_bytes())
        ),
    )
}
