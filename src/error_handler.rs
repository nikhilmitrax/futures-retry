use RetryPolicy;

/// An error handler trait.
///
/// Please note that this trait is implemented for any `FnMut` closure with a compatible signature,
/// so for some simple cases you could simply use a closure instead of creating your own type and
/// implementing this trait for it.
///
/// Here's an example of an error handler that counts error attempts.
///
/// ```
/// extern crate futures_retry;
/// use futures_retry::{ErrorHandler, RetryPolicy};
/// use std::io;
/// use std::time::Duration;
///
/// pub struct CustomHandler {
///     attempts_left: usize,
/// }
///
/// impl CustomHandler {
///     pub fn new(attempts: usize) -> Self {
///         Self {
///             attempts_left: attempts,
///         }
///     }
/// }
///
/// impl ErrorHandler<io::Error> for CustomHandler {
///     type OutError = io::Error;
///
///     fn handle(&mut self, e: io::Error) -> RetryPolicy<io::Error> {
///         if self.attempts_left == 0 {
///             eprintln!("No attempts left");
///             return RetryPolicy::ForwardError(e);
///         }
///         self.attempts_left -= 1;
///         match e.kind() {
///             io::ErrorKind::ConnectionRefused => RetryPolicy::WaitRetry(Duration::from_secs(1)),
///             io::ErrorKind::TimedOut => RetryPolicy::Repeat,
///             _ => RetryPolicy::ForwardError(e),
///         }
///     }
/// }
/// #
/// # fn main() {}
/// ```
pub trait ErrorHandler<InError> {
    /// An error that the `handle` function will produce.
    type OutError;

    /// Handles an error.
    ///
    /// Refer to the [`RetryPolicy`](enum.RetryPolicy.html) type to understand what this method
    /// might return.
    fn handle(&mut self, InError) -> RetryPolicy<Self::OutError>;
}

impl<InError, F, OutError> ErrorHandler<InError> for F
where
    F: FnMut(InError) -> RetryPolicy<OutError>,
{
    type OutError = OutError;

    fn handle(&mut self, e: InError) -> RetryPolicy<OutError> {
        (self)(e)
    }
}
