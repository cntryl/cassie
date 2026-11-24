//! Small, admitted name snapshots for phased catalog scans.
use crate::app::CassieError;
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};

pub(super) struct IndexName {
    pub(super) collection: String,
    pub(super) name: String,
}

pub(super) struct Names<T> {
    pub(super) values: Vec<T>,
    memory: Option<QueryMemoryReservation>,
}

impl<T> Names<T> {
    pub(super) fn new(controls: Option<&QueryExecutionControls>) -> Result<Self, CassieError> {
        Ok(Self {
            values: Vec::new(),
            memory: controls
                .map(|controls| controls.reserve_query_memory(0))
                .transpose()?,
        })
    }

    fn grow(&mut self, controls: Option<&QueryExecutionControls>) -> Result<(), CassieError> {
        if let (Some(controls), Some(memory)) = (controls, self.memory.as_mut()) {
            crate::app::copy_decode::grow_text_vec(&mut self.values, memory, controls)?;
        }
        Ok(())
    }

    fn copy(
        &mut self,
        value: &str,
        controls: Option<&QueryExecutionControls>,
    ) -> Result<String, CassieError> {
        if let (Some(controls), Some(memory)) = (controls, self.memory.as_mut()) {
            let (value, additional) = crate::app::copy_decode::copy_text(value, controls)?;
            memory.absorb(additional);
            Ok(value)
        } else {
            Ok(value.to_owned())
        }
    }
}

impl Names<String> {
    pub(super) fn push_name(
        &mut self,
        value: &str,
        controls: Option<&QueryExecutionControls>,
    ) -> Result<(), CassieError> {
        self.grow(controls)?;
        let value = self.copy(value, controls)?;
        self.values.push(value);
        Ok(())
    }
}

impl Names<IndexName> {
    pub(super) fn push_index(
        &mut self,
        collection: &str,
        name: &str,
        controls: Option<&QueryExecutionControls>,
    ) -> Result<(), CassieError> {
        self.grow(controls)?;
        let collection = self.copy(collection, controls)?;
        let name = self.copy(name, controls)?;
        self.values.push(IndexName { collection, name });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::Names;
    use crate::config::CassieRuntimeLimits;
    use crate::runtime::QueryExecutionControls;
    use std::time::Instant;

    #[test]
    fn should_keep_index_name_snapshot_admitted_until_its_owner_drops() {
        // Arrange
        let controls =
            QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
        let mut names = Names::new(Some(&controls)).expect("owner");

        // Act
        names
            .push_index(
                "postgres.public.rows",
                "postgres.public.idx",
                Some(&controls),
            )
            .expect("capture");
        assert!(controls.current_query_memory_bytes() > 0);
        drop(names);

        // Assert
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }

    #[test]
    fn should_release_failed_index_name_snapshot_admission() {
        // Arrange
        let limits = CassieRuntimeLimits {
            query_memory_budget_bytes: 1,
            ..CassieRuntimeLimits::default()
        };
        let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
        let mut names = Names::new(Some(&controls)).expect("owner");

        // Act
        let result = names.push_index(
            "postgres.public.rows",
            "postgres.public.idx",
            Some(&controls),
        );
        drop(names);

        // Assert
        assert!(matches!(
            result,
            Err(crate::app::CassieError::ResourceLimit(_))
        ));
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}
