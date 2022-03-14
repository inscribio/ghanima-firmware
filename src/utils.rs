//! Miscellaneous utilities

use core::convert::Infallible;

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
#[derive(Clone, Copy, PartialEq)]
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
}
