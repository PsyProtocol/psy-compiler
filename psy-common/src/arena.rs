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
