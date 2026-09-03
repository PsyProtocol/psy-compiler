use std::ops::{Index, IndexMut};

#[derive(Clone, Debug)]
pub struct Arena<I: From<usize> + Into<usize> + Copy, T> {
    pub items: Vec<T>,
    _marker: std::marker::PhantomData<I>,
}

impl<I, T> Arena<I, T>
where
    I: From<usize> + Into<usize> + Copy,
{
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn next_idx(&self) -> I {
        I::from(self.items.len())
    }

    pub fn alloc_item(&mut self, item: T) -> I {
        self.items.push(item);
        I::from(self.items.len() - 1)
    }

    pub fn replace_item(&mut self, item_idx: I, new_item: T) -> T {
        std::mem::replace(&mut self.items[item_idx.into()], new_item)
    }

    pub fn modify_item(&mut self, item_idx: I, f: &impl Fn(&mut T)) {
        f(&mut self[item_idx]);
    }

    pub fn alloc_items(&mut self, items: impl IntoIterator<Item = T>) -> Vec<I> {
        let mut result = Vec::new();
        for item in items {
            result.push(self.alloc_item(item));
        }
        result
    }

    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.items.iter()
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut T> {
        self.items.iter_mut()
    }
}

impl<I, T> Default for Arena<I, T>
where
    I: From<usize> + Into<usize> + Copy,
{
    fn default() -> Self {
        Self {
            items: Vec::new(),
            _marker: std::marker::PhantomData,
        }
    }
}

impl<I, T> Index<I> for Arena<I, T>
where
    I: From<usize> + Into<usize> + Copy,
{
    type Output = T;
    fn index(&self, index: I) -> &Self::Output {
        &self.items[index.into()]
    }
}

impl<I, T> IndexMut<I> for Arena<I, T>
where
    I: From<usize> + Into<usize> + Copy,
{
    fn index_mut(&mut self, index: I) -> &mut Self::Output {
        &mut self.items[index.into()]
    }
}

impl<I: From<usize> + Into<usize> + Copy, T> IntoIterator for Arena<I, T> {
    type Item = T;
    type IntoIter = std::vec::IntoIter<T>;

    fn into_iter(self) -> Self::IntoIter {
        self.items.into_iter()
    }
}

impl<'a, I: From<usize> + Into<usize> + Copy, T> IntoIterator for &'a Arena<I, T> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.items.iter()
    }
}

impl<'a, I: From<usize> + Into<usize> + Copy, T> IntoIterator for &'a mut Arena<I, T> {
    type Item = &'a mut T;
    type IntoIter = std::slice::IterMut<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.items.iter_mut()
    }
}

#[macro_export]
macro_rules! define_arena_id {
    ($name:ident) => {
        #[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
        pub struct $name(pub usize);

        impl From<usize> for $name {
            fn from(i: usize) -> Self {
                $name(i)
            }
        }

        impl From<$name> for usize {
            fn from(i: $name) -> Self {
                i.0
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    crate::define_arena_id!(TestId);

    #[test]
    fn empty_arena_reports_zero_length_and_first_index() {
        let arena = Arena::<TestId, i32>::new();

        assert_eq!(arena.len(), 0);
        assert_eq!(arena.next_idx(), TestId(0));
        assert_eq!(arena.iter().next(), None);
    }

    #[test]
    fn allocation_replacement_and_mutation_preserve_indices() {
        let mut arena = Arena::<TestId, i32>::new();
        let ids = arena.alloc_items([10, 20, 30]);

        assert_eq!(ids, vec![TestId(0), TestId(1), TestId(2)]);
        assert_eq!(arena.next_idx(), TestId(3));
        assert_eq!(arena.replace_item(TestId(1), 21), 20);
        arena.modify_item(TestId(2), &|value| *value += 1);
        arena[TestId(0)] = 11;

        assert_eq!(arena.iter().copied().collect::<Vec<_>>(), vec![11, 21, 31]);
    }

    #[test]
    fn owned_shared_and_mutable_iteration_cover_every_item_in_order() {
        let mut arena = Arena::<TestId, i32>::default();
        arena.alloc_items([1, 2, 3]);

        for value in &mut arena {
            *value *= 2;
        }
        assert_eq!((&arena).into_iter().copied().collect::<Vec<_>>(), vec![2, 4, 6]);
        assert_eq!(arena.into_iter().collect::<Vec<_>>(), vec![2, 4, 6]);
    }

    #[test]
    #[should_panic]
    fn invalid_index_panics_instead_of_aliasing_an_item() {
        let arena = Arena::<TestId, i32>::new();
        let _ = arena[TestId(usize::MAX)];
    }

    #[test]
    fn allocating_an_empty_batch_leaves_the_arena_untouched() {
        let mut arena = Arena::<TestId, i32>::new();

        assert!(arena.alloc_items([]).is_empty());
        assert_eq!(arena.len(), 0);
        assert_eq!(arena.next_idx(), TestId(0));
    }

    #[test]
    fn cloning_preserves_items_and_continues_indexing_from_the_copy() {
        let mut arena = Arena::<TestId, &str>::new();
        arena.alloc_items(["a", "b"]);

        let clone = arena.clone();
        arena.alloc_item("c");

        assert_eq!(clone.iter().copied().collect::<Vec<_>>(), vec!["a", "b"]);
        assert_eq!(clone.len(), 2);
        assert_eq!(arena.iter().copied().collect::<Vec<_>>(), vec!["a", "b", "c"]);
    }
}
