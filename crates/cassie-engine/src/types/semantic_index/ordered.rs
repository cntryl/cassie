use crate::runtime::QueryMemoryReservation;

pub(crate) struct OrderedEntries<K, V> {
    pub(super) values: Vec<(K, V)>,
    pub(super) memory: Option<QueryMemoryReservation>,
}
pub(crate) struct IntoIter<K, V> {
    values: std::vec::IntoIter<(K, V)>,
    _memory: Option<QueryMemoryReservation>,
}
impl<K, V> IntoIterator for OrderedEntries<K, V> {
    type Item = (K, V);
    type IntoIter = IntoIter<K, V>;
    fn into_iter(self) -> Self::IntoIter {
        IntoIter {
            values: self.values.into_iter(),
            _memory: self.memory,
        }
    }
}
impl<K, V> Iterator for IntoIter<K, V> {
    type Item = (K, V);
    fn next(&mut self) -> Option<Self::Item> {
        self.values.next()
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.values.size_hint()
    }
}
impl<K, V> ExactSizeIterator for IntoIter<K, V> {}
