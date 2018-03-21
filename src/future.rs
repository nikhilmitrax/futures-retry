use tokio_timer;
use RetryPolicy;
use futures::{Async, Future, Poll};

/// A factory trait used to create futures.
///
/// We need a factory for the retry logic because when (and if) a future returns an error, its
/// internal state is undefined and we can't poll on it anymore. Hence we need to create a new one.
///
/// By the way, this trait is implemented for any closure that returns a `Future`, so you don't
/// have to write your own type and implement it to handle some simple cases.
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
    timer: tokio_timer::Timer,
    state: RetryState<F::FutureItem>,
}

enum RetryState<F> {
    WaitingForFuture(F),
    TimerActive(tokio_timer::Sleep),
}

impl<F: FutureFactory, R> FutureRetry<F, R> {
    /// Creates a `FutureRetry` using a provided factory and a closure that decides on a
    /// retry-policy depending on an encountered error.
    ///
    /// Please refer to the `tcl-client` example in the `examples` folder to have a look at a
    /// possible usage.
    ///
    /// # Arguments
    ///
    /// * `factory`: a factory that creates futures,
    /// * `error_action`: a closure that accepts and error and decides which route to take: simply
    ///                   try again, wait and then try, or give up (on a critical error for
    ///                   exapmle).
    pub fn new<ExtErr>(mut factory: F, error_action: R) -> Self
    where
        R: FnMut(<F::FutureItem as Future>::Error) -> RetryPolicy<ExtErr>,
    {
        let current_future = factory.new();
        Self {
            factory,
            error_action,
            timer: tokio_timer::Timer::default(),
            state: RetryState::WaitingForFuture(current_future),
        }
    }
}

impl<F: FutureFactory, R, ExtErr> Future for FutureRetry<F, R>
where
    R: FnMut(<F::FutureItem as Future>::Error) -> RetryPolicy<ExtErr>,
{
    type Item = <<F as FutureFactory>::FutureItem as Future>::Item;
    type Error = ExtErr;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        loop {
            let new_state = match self.state {
                RetryState::TimerActive(ref mut sleep) => match sleep.poll() {
                    Ok(Async::Ready(())) => RetryState::WaitingForFuture(self.factory.new()),
                    Ok(Async::NotReady) => return Ok(Async::NotReady),
                    Err(e) => {
                        // There could be two possible errors: timeout (TimerError::TooLong) or no
                        // new timer could be created (TimerError::NoCapacity).
                        // Since we are using the `sleep` method there could be no **timeout**
                        // error emitted.
                        // If the timer has reached its capacity.. well.. we are using just one
                        // timer.. so it will make me panic for sure.
                        panic!("Timer error: {}", e)
                    }
                },
                RetryState::WaitingForFuture(ref mut future) => match future.poll() {
                    Ok(x) => return Ok(x),
                    Err(e) => match (self.error_action)(e) {
                        RetryPolicy::ForwardError(e) => return Err(e),
                        RetryPolicy::Repeat => RetryState::WaitingForFuture(self.factory.new()),
                        RetryPolicy::WaitRetry(duration) => {
                            RetryState::TimerActive(self.timer.sleep(duration))
                        }
                    },
                },
            };
            self.state = new_state;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use futures::future::{err, ok};

    /// Just a help type for the tests.
    struct FutureIterator<F>(F);

    impl<I, F> FutureFactory for FutureIterator<I>
    where
        I: Iterator<Item = F>,
        F: Future,
    {
        type FutureItem = F;

        /// # Warning
        ///
        /// Will panic if there is no *next* future.
        fn new(&mut self) -> Self::FutureItem {
            self.0.next().expect("No more futures!")
        }
    }

    #[test]
    fn naive() {
        let f = FutureRetry::new(|| ok::<_, u8>(1u8), |_| RetryPolicy::Repeat::<u8>);
        assert_eq!(Ok(1u8), f.wait());
    }

    #[test]
    fn naive_error_forward() {
        let f = FutureRetry::new(|| err::<u8, _>(1u8), RetryPolicy::ForwardError);
        assert_eq!(Err(1u8), f.wait());
    }

    #[test]
    fn more_complicated_wait() {
        let f = FutureRetry::new::<u8>(FutureIterator(vec![err(2u8), ok(3u8)].into_iter()), |_| {
            RetryPolicy::WaitRetry(Duration::from_millis(10))
        });
        assert_eq!(Ok(3u8), f.wait());
    }

    #[test]
    fn more_complicated_repeate() {
        let f = FutureRetry::new(FutureIterator(vec![err(2u8), ok(3u8)].into_iter()), |_| {
            RetryPolicy::Repeat::<u8>
        });
        assert_eq!(Ok(3u8), f.wait());
    }

    #[test]
    fn more_complicated_forward() {
        let f = FutureRetry::new(
            FutureIterator(vec![err(2u8), ok(3u8)].into_iter()),
            RetryPolicy::ForwardError,
        );
        assert_eq!(Err(2u8), f.wait());
    }
}
