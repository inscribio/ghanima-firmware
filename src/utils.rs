use core::convert::Infallible;

/// Helper trait to resolve Infallible Results
pub trait InfallibleResult<T> {
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
