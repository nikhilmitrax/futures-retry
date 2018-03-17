extern crate futures;
extern crate tokio_timer;

use std::time::Duration;
use futures::Future;

#[cfg(test)]
mod tests;
mod impl_details;

use impl_details::FutureRetry;

pub trait FutureFactory {
    type FutureItem: Future;

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

pub fn retry<F: FutureFactory>(factory: F, retry_interval: Option<Duration>) -> FutureRetry<F> {
    FutureRetry::new(factory, retry_interval)
}
