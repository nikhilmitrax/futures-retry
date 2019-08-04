use crate::{ErrorHandler, RetryPolicy};
use futures::{ready, task::Context, Future, Poll, TryFuture};
use std::{marker::Unpin, pin::Pin, time::Instant};
use tokio::timer;

/// A factory trait used to create futures.
///
/// We need a factory for the retry logic because when (and if) a future returns an error, its
/// internal state is undefined and we can't poll on it anymore. Hence we need to create a new one.
///
/// By the way, this trait is implemented for any closure that returns a `Future`, so you don't
/// have to write your own type and implement it to handle some simple cases.
pub trait FutureFactory {
    /// An future type that is created by the `new` method.
    type FutureItem: TryFuture;

    /// Creates a new future. We don't need the factory to be immutable so we pass `self` as a
    /// mutable reference.
    fn new(self: Pin<&mut Self>) -> Self::FutureItem;
}

impl<T, F> FutureFactory for T
where
    T: Unpin + FnMut() -> F,
    F: TryFuture,
{
    type FutureItem = F;

    #[allow(clippy::new_ret_no_self)]
    fn new(self: Pin<&mut Self>) -> F {
        (*self.get_mut())()
    }
}

/// A future that transparently launches an underlying future (created by a provided factory each
/// time) as many times as needed to get things done.
///
/// It is useful fot situations when you need to make several attempts, e.g. for establishing
/// connections, RPC calls.
///
/// There is also a type to handle `Stream` errors: [`StreamRetry`](struct.StreamRetry.html).
pub struct FutureRetry<F, R>
where
    F: FutureFactory,
{
    factory: F,
    error_action: R,
    state: RetryState<F::FutureItem>,
}

enum RetryState<F> {
    NotStarted,
    WaitingForFuture(F),
    TimerActive(timer::Delay),
}

impl<F: FutureFactory, R> FutureRetry<F, R> {
    pin_utils::unsafe_pinned!(factory: F);
    pin_utils::unsafe_pinned!(error_action: R);
    pin_utils::unsafe_pinned!(state: RetryState<F::FutureItem>);

    /// Creates a `FutureRetry` using a provided factory and an object of `ErrorHandler` type that
    /// decides on a retry-policy depending on an encountered error.
    ///
    /// Please refer to the `tcp-client` example in the `examples` folder to have a look at a
    /// possible usage.
    ///
    /// # Arguments
    ///
    /// * `factory`: a factory that creates futures,
    /// * `error_action`: a type that handles an error and decides which route to take: simply
    ///                   try again, wait and then try, or give up (on a critical error for
    ///                   exapmle).
    pub fn new(factory: F, error_action: R) -> Self {
        Self {
            factory,
            error_action,
            state: RetryState::NotStarted,
        }
    }
}

impl<F: FutureFactory, R> Future for FutureRetry<F, R>
where
    R: ErrorHandler<<F::FutureItem as TryFuture>::Error>,
{
    type Output = Result<<<F as FutureFactory>::FutureItem as TryFuture>::Ok, R::OutError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        loop {
            let new_state = match unsafe { self.as_mut().state().get_unchecked_mut() } {
                RetryState::NotStarted => {
                    RetryState::WaitingForFuture(self.as_mut().factory().new())
                }
                RetryState::TimerActive(delay) => {
                    let delay = unsafe { Pin::new_unchecked(delay) };
                    ready!(delay.poll(cx));
                    RetryState::WaitingForFuture(self.as_mut().factory().new())
                }
                RetryState::WaitingForFuture(future) => {
                    let future = unsafe { Pin::new_unchecked(future) };
                    match ready!(future.try_poll(cx)) {
                        Ok(x) => {
                            self.as_mut().error_action().ok();
                            return Poll::Ready(Ok(x));
                        }
                        Err(e) => match self.as_mut().error_action().handle(e) {
                            RetryPolicy::ForwardError(e) => return Poll::Ready(Err(e)),
                            RetryPolicy::Repeat => {
                                RetryState::WaitingForFuture(self.as_mut().factory().new())
                            }
                            RetryPolicy::WaitRetry(duration) => RetryState::TimerActive(
                                timer::Delay::new(Instant::now() + duration),
                            ),
                        },
                    }
                }
            };
            self.as_mut().state().set(new_state);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::executor::block_on;
    use futures::{
        future::{err, ok},
        TryFutureExt,
    };
    use std::time::Duration;

    /// Just a help type for the tests.
    struct FutureIterator<F>(F);

    impl<I, F> FutureFactory for FutureIterator<I>
    where
        I: Unpin + Iterator<Item = F>,
        F: TryFuture,
    {
        type FutureItem = F;

        /// # Warning
        ///
        /// Will panic if there is no *next* future.
        fn new(mut self: Pin<&mut Self>) -> Self::FutureItem {
            self.0.next().expect("No more futures!")
        }
    }

    #[test]
    fn naive() {
        let f = FutureRetry::new(|| ok::<_, u8>(1u8), |_| RetryPolicy::Repeat::<u8>);
        assert_eq!(Ok(1u8), block_on(f.into_future()));
    }

    #[test]
    fn naive_error_forward() {
        let f = FutureRetry::new(|| err::<u8, _>(1u8), RetryPolicy::ForwardError);
        assert_eq!(Err(1u8), block_on(f.into_future()));
    }

    #[test]
    fn more_complicated_wait() {
        let f = FutureRetry::new(FutureIterator(vec![err(2u8), ok(3u8)].into_iter()), |_| {
            RetryPolicy::WaitRetry::<u8>(Duration::from_millis(10))
        })
        .into_future();
        let rt = tokio::runtime::Runtime::new().unwrap();
        assert_eq!(Ok(3), rt.block_on(f));
    }

    #[test]
    fn more_complicated_repeat() {
        let f = FutureRetry::new(FutureIterator(vec![err(2u8), ok(3u8)].into_iter()), |_| {
            RetryPolicy::Repeat::<u8>
        });
        assert_eq!(Ok(3u8), block_on(f.into_future()));
    }

    #[test]
    fn more_complicated_forward() {
        let f = FutureRetry::new(
            FutureIterator(vec![err(2u8), ok(3u8)].into_iter()),
            RetryPolicy::ForwardError,
        );
        assert_eq!(Err(2u8), block_on(f.into_future()));
    }
}
