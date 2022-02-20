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
