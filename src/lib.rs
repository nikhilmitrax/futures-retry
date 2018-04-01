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
#[cfg(test)]
extern crate tokio;
extern crate tokio_timer;

use std::time::Duration;

mod future;
mod stream;

pub use future::FutureFactory;
pub use future::FutureRetry;
pub use stream::StreamRetry;
pub use stream::StreamRetryExt;

/// What to do when a future returns an error. Used in `FutureRetry::new` and `StreamRetry::new`.
pub enum RetryPolicy<E> {
    /// Create and poll a new future immediately.
    ///
    /// # Be careful!
    ///
    /// Please be careful when using this variant since it might lead to a high (actually 100%) CPU
    /// usage in case a future instantly resolves into an error every time.
    Repeat,
    /// Wait for a given duration and make another attempt then.
    WaitRetry(Duration),
    /// Don't give it another try, just pass the error further to the user.
    ForwardError(E),
}
