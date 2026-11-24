pub use cassie_core::catalog::sequence::{SequenceCursor, SequenceError, SequenceParameters};
pub use cassie_core::catalog::declarations::SequenceOptions;

#[cfg(test)]
mod tests {
    use super::{SequenceCursor, SequenceOptions, SequenceParameters};
    use crate::types::DataType;

    fn parameters(options: &SequenceOptions) -> SequenceParameters {
        SequenceParameters::resolve(None, options, &DataType::BigInt).expect("valid options")
    }

    #[test]
    fn should_stop_a_smallint_sequence_at_its_type_maximum() {
        // Arrange
        let parameters = parameters(&SequenceOptions {
            data_type: Some(DataType::SmallInt),
            start: Some(32_766),
            ..SequenceOptions::default()
        });
        let mut cursor = SequenceCursor::starting_at(parameters.start);

        // Act
        let values = [
            cursor.next("s1", &parameters),
            cursor.next("s1", &parameters),
        ];
        let exhausted = cursor
            .next("s1", &parameters)
            .map_err(|error| error.sqlstate);

        // Assert
        assert_eq!(values, [Ok(32_766), Ok(32_767)]);
        assert_eq!(exhausted, Err("2200H"));
    }

    #[test]
    fn should_default_descending_sequence_bounds_like_postgresql() {
        // Arrange
        let options = SequenceOptions {
            increment: Some(-1),
            ..SequenceOptions::default()
        };

        // Act
        let parameters = parameters(&options);

        // Assert
        assert_eq!(
            (parameters.start, parameters.max_value, parameters.min_value),
            (-1, -1, i64::MIN)
        );
    }

    #[test]
    fn should_cycle_back_to_the_minimum() {
        // Arrange
        let parameters = parameters(&SequenceOptions {
            max_value: Some(Some(2)),
            cycle: Some(true),
            ..SequenceOptions::default()
        });
        let mut cursor = SequenceCursor::starting_at(parameters.start);

        // Act
        let values = [(); 3].map(|()| cursor.next("s3", &parameters));

        // Assert
        assert_eq!(values, [Ok(1), Ok(2), Ok(1)]);
    }

    #[test]
    fn should_return_the_set_value_next_when_setval_is_not_called() {
        // Arrange
        let parameters = parameters(&SequenceOptions::default());
        let mut cursor = SequenceCursor::starting_at(parameters.start);

        // Act
        let set = cursor.set("s4", &parameters, 10, false);
        let next = cursor.next("s4", &parameters);

        // Assert
        assert_eq!((set, next), (Ok(10), Ok(10)));
    }

    #[test]
    fn should_reject_invalid_options_with_postgresql_messages() {
        // Arrange
        let cases = [
            SequenceOptions {
                start: Some(5),
                min_value: Some(Some(10)),
                ..SequenceOptions::default()
            },
            SequenceOptions {
                data_type: Some(DataType::Int),
                max_value: Some(Some(3_000_000_000)),
                ..SequenceOptions::default()
            },
        ];

        // Act
        let messages = cases.map(|options| {
            SequenceParameters::resolve(None, &options, &DataType::BigInt)
                .map_err(|error| error.message)
        });

        // Assert
        assert_eq!(
            messages,
            [
                Err("START value (5) cannot be less than MINVALUE (10)".to_string()),
                Err(
                    "MAXVALUE (3000000000) is out of range for sequence data type integer"
                        .to_string()
                ),
            ]
        );
    }
}
