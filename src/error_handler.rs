use crate::RetryPolicy;
use std::pin::Pin;

/// An error handler trait.
///
/// Please note that this trait is implemented for any `FnMut` closure with a compatible signature,
/// so for some simple cases you might simply use a closure instead of creating your own type and
/// implementing this trait for it.
///
/// Here's an example of an error handler that counts *consecutive* error attempts.
///
/// ```
/// use futures_retry::{ErrorHandler, RetryPolicy};
/// use std::io;
/// use std::time::Duration;
///
/// pub struct CustomHandler {
///     attempt: usize,
///     max_attempts: usize,
/// }
///
/// impl CustomHandler {
///     pub fn new(attempts: usize) -> Self {
///         Self {
///             attempt: 0,
///             max_attempts: attempts,
///         }
///     }
/// }
///
/// impl ErrorHandler<io::Error> for CustomHandler {
///     type OutError = io::Error;
///
///     fn handle(&mut self, e: io::Error) -> RetryPolicy<io::Error> {
///         if self.attempt == self.max_attempts {
///             eprintln!("No attempts left");
///             return RetryPolicy::ForwardError(e);
///         }
///         self.attempt += 1;
///         match e.kind() {
///             io::ErrorKind::ConnectionRefused => RetryPolicy::WaitRetry(Duration::from_secs(1)),
///             io::ErrorKind::TimedOut => RetryPolicy::Repeat,
///             _ => RetryPolicy::ForwardError(e),
///         }
///     }
///
///     fn ok(&mut self) {
///         self.attempt = 0;
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
    fn handle(self: Pin<&mut Self>, _: InError) -> RetryPolicy<Self::OutError>;

    /// This method is called on a successful execution (before returning an item) of the underlying
    /// future/stream.
    ///
    /// One can use this method to reset an internal state, like a consecutive errors counter for
    /// example.
    ///
    /// By default the method is a no-op.
    fn ok(self: Pin<&mut Self>) {}
}

impl<InError, F, OutError> ErrorHandler<InError> for F
where
    F: Unpin + FnMut(InError) -> RetryPolicy<OutError>,
{
    type OutError = OutError;

    fn handle(self: Pin<&mut Self>, e: InError) -> RetryPolicy<OutError> {
        (self.get_mut())(e)
    }
}
