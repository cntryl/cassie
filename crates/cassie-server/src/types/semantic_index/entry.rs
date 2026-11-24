use super::Bucket;

pub(crate) enum Entry<'a, K, V> {
    Occupied(OccupiedEntry<'a, K, V>),
    Vacant(VacantEntry<'a, K, V>),
}
impl<'a, K, V> Entry<'a, K, V> {
    pub(crate) fn or_default(self) -> &'a mut V
    where
        V: Default,
    {
        match self {
            Self::Occupied(entry) => entry.into_mut(),
            Self::Vacant(entry) => entry.insert(V::default()),
        }
    }
}
pub(crate) struct OccupiedEntry<'a, K, V> {
    pub(super) value: &'a mut (K, V),
}
impl<'a, K, V> OccupiedEntry<'a, K, V> {
    pub(crate) fn into_mut(self) -> &'a mut V {
        &mut self.value.1
    }
    pub(crate) fn get_mut(&mut self) -> &mut V {
        &mut self.value.1
    }
}
pub(crate) struct VacantEntry<'a, K, V> {
    pub(super) bucket: &'a mut Bucket<K, V>,
    pub(super) key: K,
    pub(super) length: &'a mut usize,
}
impl<'a, K, V> VacantEntry<'a, K, V> {
    pub(crate) fn key(&self) -> &K {
        &self.key
    }
    pub(crate) fn insert(self, value: V) -> &'a mut V {
        self.bucket.values.push((self.key, value));
        *self.length += 1;
        &mut self.bucket.values.last_mut().expect("inserted record").1
    }
}
