//! Miscellaneous utilities

use core::convert::Infallible;

use serde::{Serialize, Deserialize};

/// Const-evaluation of max(a,b): <https://stackoverflow.com/a/53646925>
pub const fn max(a: usize, b: usize) -> usize {
    [a, b][(a < b) as usize]
}

/// Helper trait to resolve [`Result`]s with unreachable [`Err`] variant
pub trait InfallibleResult<T> {
    /// Take the Ok value of an infallible [`Result`]
    fn infallible(self) -> T;
}

impl<T> InfallibleResult<T> for Result<T, Infallible> {
    fn infallible(self) -> T {
        match self {
            Ok(v) => v,
            Err(e) => match e {},
        }
    }
}

/// Changing value of a variable with integer steps
#[derive(Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Inc {
    /// Up/Increase/Next/Increment
    Up,
    /// Down/Decrease/Previous/Decrement
    Down,
}

impl Inc {
    pub fn update<'a, T>(&self, iter: &mut CircularIter<'a, T>) -> &'a T {
        match self {
            Inc::Up => iter.next().unwrap(),
            Inc::Down => iter.next_back().unwrap(),
        }
    }
}

/// Slice iterator in both directions that wraps around to first/last element
///
/// Often the use case is to use [`CircularIter::current`] to access the current
/// element and only use iterator methods only to advance the "cursor". In any
/// case `next()` returns the new current element, so [`CircularIter::current`]
/// is de facto the element previously returned by the iterator.
pub struct CircularIter<'a, T> {
    slice: &'a [T],
    index: usize,
}

impl<'a, T> CircularIter<'a, T> {
    /// Create new iterator pointing to first element of the slice
    pub fn new(slice: &'a [T]) -> Self {
        Self { slice, index: 0 }
    }

    /// Get the element currently being pointed at
    pub fn current(&self) -> &'a T {
        &self.slice[self.index]
    }
}

impl<'a, T> Iterator for CircularIter<'a, T> {
    type Item = &'a T;

    fn next(&mut self) -> Option<Self::Item> {
        self.index = (self.index + 1) % self.slice.len();
        Some(self.current())
    }
}

impl<'a, T> DoubleEndedIterator for CircularIter<'a, T> {
    fn next_back(&mut self) -> Option<Self::Item> {
        self.index = if self.index == 0 {
            self.slice.len() - 1
        } else {
            self.index - 1
        };
        Some(self.current())
    }
}

/// Extension trait for [`Option`] for tracking if a value changes on updates
pub trait OptionChanges {
    type Item;

    /// Update contained value, return the new one if the value changed (or option was `None`)
    fn if_changed(&mut self, new: &Self::Item) -> Option<&Self::Item>;
}

impl<T> OptionChanges for Option<T>
where
    T: Clone + PartialEq
{
    type Item = T;

    fn if_changed(&mut self, new: &Self::Item) -> Option<&Self::Item> {
        if self.as_ref().map_or(true, |last| last != new) {
            Some(self.insert(new.clone()))
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn circular_iterator() {
        let array = [1, 2, 3];
        let mut iter = CircularIter::new(&array[..]);
        assert_eq!(iter.current(), &1);
        assert_eq!(iter.next(), Some(&2));
        assert_eq!(iter.current(), &2);
        assert_eq!(iter.next(), Some(&3));
        assert_eq!(iter.current(), &3);
        assert_eq!(iter.next(), Some(&1));
        assert_eq!(iter.current(), &1);
        assert_eq!(iter.next(), Some(&2));
        assert_eq!(iter.current(), &2);
        assert_eq!(iter.next_back(), Some(&1));
        assert_eq!(iter.current(), &1);
        assert_eq!(iter.next_back(), Some(&3));
        assert_eq!(iter.current(), &3);
        assert_eq!(iter.next_back(), Some(&2));
        assert_eq!(iter.current(), &2);
    }

    #[test]
    fn option_changes() {
        let mut val: Option<u8> = None;
        assert_eq!(val, None);
        assert_eq!(val.if_changed(&10), Some(&10));
        assert_eq!(val.if_changed(&20), Some(&20));
        assert_eq!(val.if_changed(&20), None);
        assert_eq!(val.if_changed(&30), Some(&30));
        assert_eq!(val.if_changed(&30), None);
        assert_eq!(val.if_changed(&30), None);
    }
}
