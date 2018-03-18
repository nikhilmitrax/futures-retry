extern crate futures;
extern crate tokio_timer;

use std::time::Duration;
use futures::{Future, Stream};

#[cfg(test)]
mod tests;
mod future;
mod stream;
mod stream_propagate;

use future::FutureRetry;
pub use stream::StreamRetry;
pub use stream_propagate::StreamRetryPropagate;

/// A factory trait used to create futures.
///
/// We need a factory for the retry logic because when (and if) a future returns an error, its
/// internal state is undefined and we can't poll on it anymore. Hence we need to create a new one.
///
/// By the way, this trait is implemented for any closure that returns a `Future`.
pub trait FutureFactory {
    /// An future type that is created by the `new` method.
    type FutureItem: Future;

    /// Creates a new future. We don't need the factory to be immutable so we pass `self` as a
    /// mutable reference.
    fn new(&mut self) -> Self::FutureItem;
}

impl<T, F> FutureFactory for T
where
    T: FnMut() -> F,
    F: Future,
{
    type FutureItem = F;

    fn new(&mut self) -> F {
        (*self)()
    }
}

/// What to do when a future return an error.
pub enum RetryPolicy {
    /// Create and poll a new future immediately.
    ///
    /// # Pay attention!
    ///
    /// Please be careful when using this variant since it might leed to a high (actually 100%) CPU
    /// usage in case a future instantly resolved into an error.
    Repeat,
    /// Wait for a given duration and then make another attempt.
    WaitRetry(Duration),
    /// Don't give it another try, just pass the error further to the user.
    ForwardError,
}

/// What to do when a future return an error.
pub enum RetryPropagatePolicy<E> {
    /// Create and poll a new future immediately.
    ///
    /// # Pay attention!
    ///
    /// Please be careful when using this variant since it might leed to a high (actually 100%) CPU
    /// usage in case a future instantly resolved into an error.
    Repeat,
    /// Wait for a given duration and then make another attempt.
    WaitRetry(Duration),
    /// Don't give it another try, just terminate the stream with a given error.
    ForwardError(E),
}

pub fn retry<F, R>(factory: F, error_action: R) -> FutureRetry<F, R>
where
    F: FutureFactory,
    R: FnMut(&<F::FutureItem as Future>::Error) -> RetryPolicy,
{
    FutureRetry::new(factory, error_action)
}

pub fn retry_stream<R, S, T, E>(stream: S, error_action: R) -> StreamRetry<R, S>
where
    S: Stream<Item = Result<T, E>>,
    R: FnMut(&E) -> RetryPolicy,
{
    StreamRetry::new(stream, error_action)
}
