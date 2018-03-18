use tokio_timer;
use FutureFactory;
use RetryPolicy;
use futures::{Async, Future, Poll};

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
    pub(crate) fn new(mut factory: F, error_action: R) -> Self
    where
        R: FnMut(&<F::FutureItem as Future>::Error) -> RetryPolicy,
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

impl<F: FutureFactory, R> Future for FutureRetry<F, R>
where
    R: FnMut(&<F::FutureItem as Future>::Error) -> RetryPolicy,
{
    type Item = <<F as FutureFactory>::FutureItem as Future>::Item;
    type Error = <<F as FutureFactory>::FutureItem as Future>::Error;

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
                    Ok(Async::Ready(res)) => return Ok(Async::Ready(res)),
                    Ok(Async::NotReady) => return Ok(Async::NotReady),
                    Err(e) => match (self.error_action)(&e) {
                        RetryPolicy::ForwardError => return Err(e),
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
