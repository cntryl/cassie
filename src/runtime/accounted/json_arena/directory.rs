//! Isolated recoverable directory migration; no arena consumer is wired yet.
use crate::app::CassieError;
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};

#[cfg(test)]
#[path = "directory_tests.rs"]
mod tests;

#[derive(Clone, Copy)]
pub(super) enum Progress {
    Initialized(usize),
    Moved(usize),
}

struct OwnedDirectory<T> {
    items: Vec<Option<T>>,
    memory: QueryMemoryReservation,
}

struct Migration<T> {
    next: OwnedDirectory<T>,
    frontier: usize,
}

pub(super) struct Directory<T> {
    current: OwnedDirectory<T>,
    migration: Option<Migration<T>>,
    controls: QueryExecutionControls,
}

impl<T> Directory<T> {
    pub(super) fn new(controls: &QueryExecutionControls) -> Result<Self, CassieError> {
        Ok(Self {
            current: OwnedDirectory {
                items: Vec::new(),
                memory: controls.reserve_query_memory(0)?,
            },
            migration: None,
            controls: controls.clone(),
        })
    }

    pub(super) fn push(&mut self, value: T) -> Result<(), CassieError> {
        self.push_with_observer(value, |_| Ok(()))
    }

    pub(super) fn push_observed(
        &mut self,
        value: T,
        observer: impl FnMut(Progress) -> Result<(), CassieError>,
    ) -> Result<(), CassieError> {
        self.push_with_observer(value, observer)
    }

    fn push_with_observer(
        &mut self,
        value: T,
        mut observer: impl FnMut(Progress) -> Result<(), CassieError>,
    ) -> Result<(), CassieError> {
        self.controls.check_interruption()?;
        if self.migration.is_none() && self.current.items.len() == self.current.items.capacity() {
            self.prepare_migration(&mut observer)?;
        }
        self.finish_migration(&mut observer)?;
        self.controls.check_interruption()?;
        self.current.items.push(Some(value));
        Ok(())
    }

    fn prepare_migration(
        &mut self,
        observer: &mut impl FnMut(Progress) -> Result<(), CassieError>,
    ) -> Result<(), CassieError> {
        let length = self.current.items.len();
        let capacity = length
            .max(2)
            .checked_mul(2)
            .ok_or_else(|| CassieError::ResourceLimit("JSON arena directory overflow".into()))?;
        let memory;
        let mut items;
        (items, memory) =
            crate::runtime::owned_memory::try_reserve_vec(capacity, &self.current.memory)?;
        for index in 0..length {
            self.controls.check_interruption()?;
            items.push(None);
            observer(Progress::Initialized(index + 1))?;
        }
        self.controls.check_interruption()?;
        self.migration = Some(Migration {
            next: OwnedDirectory { items, memory },
            frontier: 0,
        });
        Ok(())
    }

    fn finish_migration(
        &mut self,
        observer: &mut impl FnMut(Progress) -> Result<(), CassieError>,
    ) -> Result<(), CassieError> {
        while let Some(migration) = &mut self.migration {
            if migration.frontier == self.current.items.len() {
                let completed = self.migration.take().expect("migration");
                let old = std::mem::replace(&mut self.current, completed.next);
                drop(old);
                break;
            }
            self.controls.check_interruption()?;
            let index = migration.frontier;
            migration.next.items[index] = self.current.items[index].take();
            migration.frontier += 1;
            observer(Progress::Moved(migration.frontier))?;
            self.controls.check_interruption()?;
        }
        Ok(())
    }

    pub(super) fn get(&self, index: usize) -> Option<&T> {
        if let Some(migration) = &self.migration {
            if index < migration.frontier {
                return migration.next.items.get(index)?.as_ref();
            }
        }
        self.current.items.get(index)?.as_ref()
    }

    pub(super) fn take(&mut self, index: usize) -> Option<T> {
        if let Some(migration) = &mut self.migration {
            if index < migration.frontier {
                return migration.next.items.get_mut(index)?.take();
            }
        }
        self.current.items.get_mut(index)?.take()
    }

    pub(super) fn get_mut(&mut self, index: usize) -> Option<&mut T> {
        if let Some(migration) = &mut self.migration {
            if index < migration.frontier {
                return migration.next.items.get_mut(index)?.as_mut();
            }
        }
        self.current.items.get_mut(index)?.as_mut()
    }

    pub(super) fn put_empty(&mut self, index: usize, value: T) -> Result<(), T> {
        let slot = if let Some(migration) = &mut self.migration {
            if index < migration.frontier {
                migration.next.items.get_mut(index)
            } else {
                self.current.items.get_mut(index)
            }
        } else {
            self.current.items.get_mut(index)
        };
        match slot {
            Some(slot @ None) => {
                *slot = Some(value);
                Ok(())
            }
            _ => Err(value),
        }
    }

    #[cfg(test)]
    fn test_frontier(&self) -> Option<usize> {
        self.migration.as_ref().map(|migration| migration.frontier)
    }
}
