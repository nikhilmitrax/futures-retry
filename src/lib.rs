//! A tool that helps you retry your future :)
//!
//! It's quite a common task when you need to repeat some action if you've got an error, be it a
//! connection timeout or some temporary OS error.
//!
//! When you do stuff in a synchronous manner it's quite easy to implement the attempts logic, but
//! when it comes to asynchronous programming you suddenly need to write a fool load of a
//! boilerplate code, introduce state machines and everything.
//!
//! This library aims to make your life easier and let you write more straightword and nice code,
//! concentrating on buisness logic rathen than on handling all the mess.
//!
//! I was inspired to write this library after coming over a [`hyper`
//! issue](https://github.com/hyperium/hyper/issues/1358), and I came to an understanding that the
//! problem is more common than I used to think.
//!
//! Suggestions and critiques are welcome!

extern crate futures;
extern crate tokio_timer;

use std::time::Duration;
use futures::Future;

mod future;
mod stream;

pub use future::FutureRetry;
pub use stream::StreamRetry;
pub use stream::StreamRetryExt;

/// A factory trait used to create futures.
///
/// We need a factory for the retry logic because when (and if) a future returns an error, its
/// internal state is undefined and we can't poll on it anymore. Hence we need to create a new one.
///
/// By the way, this trait is implemented for any closure that returns a `Future`, so you don't
/// have to write and implement your own type to handle some simple cases.
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

/// What to do when a future returns an error.
pub enum RetryPolicy<E> {
    /// Create and poll a new future immediately.
    ///
    /// # Pay attention!
    ///
    /// Please be careful when using this variant since it might leed to a high (actually 100%) CPU
    /// usage in case a future instantly resolves into an error.
    Repeat,
    /// Wait for a given duration and make another attempt then.
    WaitRetry(Duration),
    /// Don't give it another try, just pass the error further to the user (probably after some
    /// amendments).
    ForwardError(E),
}
