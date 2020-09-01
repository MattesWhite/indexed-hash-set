//! This crate provides a bidirectional set with the following properties:
//! - Each entry is distinct (no double elements).
//! - Entries can be accessed by reference, like in a standard `HashSet`.
//! - Entries can be accessed via indices avoiding the cost of hashing the element.
//! - Indices are reference-counted. This means when no external index is around
//!   an entry is considered _unused_ and will be dropped on [`drop_unused()`].
//! - Internal a [generational arena] is used to allow for effective mutation
//!   of the set.
//!
//! [`drop_unused()`]: ./struct.IndexedHashSet.html#method.drop_unused
//! [generational arena]: https://docs.rs/generational-arena/latest/

#![deny(missing_docs)]

use generational_arena::{Arena, Index as AIndex};
use std::borrow::Borrow;
use std::cell::RefCell;
use std::collections::HashMap;
use std::hash::Hash;
use std::rc::Rc;

mod internal_ref;
use self::internal_ref::{InternalRef, Wrap as _};

/// An entry in the set.
#[derive(Debug)]
struct Entry<T> {
    /// Elements are boxed to allow correct self-references in the
    /// element-to-index-map. Otherwise a re-allocation of the arena due to
    /// growth could invalidate the supporting map.
    elem: Box<T>,
    /// Count of existing indices referencing this entry. If this is zero the
    /// entry can be dropped.
    usage_cnt: Rc<RefCell<usize>>,
}

impl<T> Entry<T> {
    /// A new entry with a `usage_cnt` of zero.
    fn new(elem: T) -> Self {
        Entry {
            elem: Box::new(elem),
            usage_cnt: Default::default(),
        }
    }
    fn cnt_handle(&self) -> Rc<RefCell<usize>> {
        self.usage_cnt.clone()
    }
    fn cnt(&self) -> usize {
        *(*self.usage_cnt).borrow()
    }
    fn elem(&self) -> &T {
        self.elem.as_ref()
    }
}

/// An indexed hash set. Can be accessed either by index of hashing.
#[derive(Debug)]
pub struct IndexedHashSet<T>
where
    T: 'static,
{
    /// The actual store of entries.
    arena: Arena<Entry<T>>,
    /// Map from elements to indices that ultimately retrieve the entries.
    ///
    /// The keys are fake `'static`. Actually they **self-reference** the
    /// entries in the arena.
    map: HashMap<InternalRef<T>, AIndex>,
}

impl<T> IndexedHashSet<T>
where
    T: 'static + Eq + Hash,
{
    /// A new, empty set.
    pub fn new() -> Self {
        Default::default()
    }
    /// Number of elements in the set, including the unused ones.
    pub fn len(&self) -> usize {
        self.arena.len()
    }
    /// Get the usage count of an element by hash.
    pub fn get_cnt<Q>(&self, elem: &Q) -> Option<usize>
    where
        T: Borrow<Q>,
        Q: ?Sized + Hash + Eq,
    {
        let idx = self.map.get_key_value(elem.wrap()).map(|(_, idx)| *idx)?;
        let entry = &self.arena[idx];
        Some(entry.cnt())
    }
    /// Get a reference to the stored element by hash.
    pub fn get_ref_by_hash<'a, Q>(&'a self, elem: &Q) -> Option<&'a T>
    where
        T: Borrow<Q>,
        Q: ?Sized + Hash + Eq,
    {
        // points to the same entry, no need for a lookup in the arena
        self.map.get_key_value(elem.wrap()).map(|(k, _)| k.as_ref())
    }
    /// Get the index of the stored element by hash.
    pub fn get_index_by_hash<'a, Q>(&'a self, elem: &Q) -> Option<RcIndex>
    where
        T: Borrow<Q>,
        Q: ?Sized + Hash + Eq,
    {
        let a_idx = self.map.get(elem.wrap())?;
        Some(self.aidx_to_rcidx(*a_idx))
    }
    /// Get a reference to the stored element by index.
    ///
    /// As the index can be from another `IndexedHashSet` this operation is
    /// fallible.
    ///
    /// Alternatively, the [index notation](struct.IndexedHashSet.html#impl-Index<%26'a RcIndex>)
    /// can be used, e.g. `set[&rc_idx]`. However, this may panic with a
    /// foreign `RcIndex`.
    //#impl-Index<%26'a RcIndex>
    pub fn get_ref_by_index<'a>(&'a self, idx: &RcIndex) -> Option<&'a T> {
        let entry = self.arena.get(idx.inner)?;
        Some(entry.elem.as_ref())
    }
    /// Insert a new element into the set.
    ///
    /// If the element is already in the set `None` is returned else the index
    /// of the new entry is returned.
    ///
    /// _Note:_ The returned `RcIndex` is the initial usage of the entry. If it
    /// is dropped without cloning the `usage_cnt` goes to zero and the new
    /// element is dropped on the next [`drop_unused()`](#method.drop_unused)!
    #[must_use = "If not stored usage count of the new element goes to zero."]
    pub fn insert(&mut self, elem: T) -> Option<RcIndex> {
        if self.map.get(elem.wrap()).is_some() {
            return None;
        }

        Some(self.insert_unchecked(elem))
    }
    /// Gets the index of the element in the set if present. If not the element
    /// is inserted and the new index is returned.
    pub fn get_or_insert(&mut self, elem: &T) -> RcIndex
    where
        T: Clone,
    {
        if let Some(a_idx) = self.map.get(elem.wrap()) {
            self.aidx_to_rcidx(*a_idx)
        } else {
            self.insert_unchecked(elem.clone())
        }
    }
    /// Unconditionally inserts the element.
    ///
    /// If not checked carefully this may violate the `IndexedHashSet`'s
    /// contract that elements are distinct as the arena doesn't have the
    /// properties of a set.
    fn insert_unchecked(&mut self, elem: T) -> RcIndex {
        let entry = Entry::new(elem);
        let cnt_handle = entry.cnt_handle();
        let inner_ref = InternalRef::from_ref(entry.elem());

        let a_idx = self.arena.insert(entry);
        self.map.insert(inner_ref, a_idx);

        RcIndex::new(a_idx, cnt_handle)
    }
    /// Drop all entries whose `usage_cnt` is zero.
    pub fn drop_unused(&mut self) -> usize {
        // tell Rust that both mutable borrows are distinct.
        let arena = &mut self.arena;
        let map = &mut self.map;

        let before = arena.len();

        arena.retain(|_, entry| {
            if entry.cnt() == 0 {
                map.remove(entry.elem().wrap());
                false
            } else {
                true
            }
        });

        before - arena.len()
    }
    /// Iterates over all elements in the set with `usage_cnt != 0`.
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.arena
            .iter()
            .filter_map(|(_, e)| if e.cnt() != 0 { Some(e.elem()) } else { None })
    }
    /// Returns the respective `RcIndex` for an index of the arena.
    ///
    /// # Panics
    ///
    /// This panics if the arena index is not present. However, since these
    /// kind of indices are only used internally this should never be the case.
    fn aidx_to_rcidx(&self, a_idx: AIndex) -> RcIndex {
        let entry = &self.arena[a_idx];
        let handle = entry.cnt_handle();
        RcIndex::new(a_idx, handle)
    }
}

impl<T: 'static> Default for IndexedHashSet<T> {
    fn default() -> Self {
        Self {
            arena: Default::default(),
            map: Default::default(),
        }
    }
}

/// Allows to access the set like `set[&rc_idx]`.
///
/// This panics if the `RcIndex` used is not from this `IndexedHashSet`.
impl<'a, T> std::ops::Index<&'a RcIndex> for IndexedHashSet<T>
where
    T: 'static + Eq + Hash,
{
    type Output = T;

    fn index(&self, index: &'a RcIndex) -> &Self::Output {
        self.get_ref_by_index(index).unwrap()
    }
}

/// The `!Send` internal references are only used internally. Therefore, this
/// type is safe to be `Send`.
unsafe impl<T> Send for IndexedHashSet<T> {}

/// A reference-counted index to an entry of the set.
#[derive(Debug)]
pub struct RcIndex {
    /// Original index into the arena.
    inner: AIndex,
    /// Usage count. Incremented at index construction and decremented at drop.
    cnt: Rc<RefCell<usize>>,
}

impl RcIndex {
    /// Creates a new reference-counted index.
    ///
    /// On creation the `usage_cnt` is incremented.
    fn new(idx: AIndex, cnt_handle: Rc<RefCell<usize>>) -> Self {
        {
            let mut cnt = cnt_handle.borrow_mut();
            *cnt += 1;
        }
        Self {
            inner: idx,
            cnt: cnt_handle,
        }
    }
    /// Get the usage count of the element.
    pub fn cnt(&self) -> usize {
        *(*self.cnt).borrow()
    }
}

impl Clone for RcIndex {
    fn clone(&self) -> Self {
        let mut cnt = self.cnt.borrow_mut();
        *cnt += 1;
        Self {
            inner: self.inner,
            cnt: self.cnt.clone(),
        }
    }
}

impl Drop for RcIndex {
    fn drop(&mut self) {
        let mut cnt = self.cnt.borrow_mut();
        *cnt -= 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Set with three entries with each usage count equal to zero.
    fn standard_set() -> IndexedHashSet<String> {
        let mut set = IndexedHashSet::new();
        set.insert("Olaf".to_owned()).unwrap();
        set.insert("Eijnar".to_owned()).unwrap();
        set.insert("Harald".to_owned()).unwrap();
        set
    }

    #[test]
    fn unused_entries() {
        let mut set = standard_set();
        assert_eq!(set.len(), 3);
        assert_eq!(set.drop_unused(), 3);
        assert_eq!(set.len(), 0);
    }

    #[test]
    fn usage_cnt() {
        let mut set = IndexedHashSet::new();
        let o1 = set.insert("Olaf".to_owned()).unwrap();
        assert_eq!(o1.cnt(), 1);
        let _o2 = set.get_index_by_hash("Olaf").unwrap();
        assert_eq!(o1.cnt(), 2);
        {
            let _o3 = o1.clone();
            assert_eq!(o1.cnt(), 3);
        }
        assert_eq!(o1.cnt(), 2);
    }
}
