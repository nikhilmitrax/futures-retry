use tokio_timer;
use FutureFactory;
use std::time::Duration;
use futures::{Async, Future, Poll};

pub struct FutureRetry<F: FutureFactory> {
    factory: F,
    retry_interval: Option<Duration>,
    timer: tokio_timer::Timer,
    state: RetryState<F::FutureItem>,
}

enum RetryState<F> {
    WaitingForFuture(F),
    TimerActive(tokio_timer::Sleep),
}

impl<F: FutureFactory> FutureRetry<F> {
    pub(crate) fn new(mut factory: F, retry_interval: Option<Duration>) -> Self {
        let current_future = factory.new();
        Self {
            factory,
            retry_interval,
            timer: tokio_timer::Timer::default(),
            state: RetryState::WaitingForFuture(current_future),
        }
    }
}

impl<F: FutureFactory> Future for FutureRetry<F> {
    type Item = <<F as FutureFactory>::FutureItem as Future>::Item;
    type Error = ();

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        loop {
            let new_state = match self.state {
                RetryState::TimerActive(ref mut sleep) => match sleep.poll() {
                    Ok(Async::Ready(())) => RetryState::WaitingForFuture(self.factory.new()),
                    Ok(Async::NotReady) => return Ok(Async::NotReady),
                    Err(e) => panic!("Timer error: {}", e),
                },
                RetryState::WaitingForFuture(ref mut future) => match future.poll() {
                    Ok(Async::Ready(res)) => return Ok(Async::Ready(res)),
                    Ok(Async::NotReady) => return Ok(Async::NotReady),
                    Err(_) => {
                        println!("Caught an error");
                        if let Some(retry_interval) = self.retry_interval {
                            RetryState::TimerActive(self.timer.sleep(retry_interval))
                        } else {
                            continue;
                        }
                    }
                },
            };
            self.state = new_state;
        }
    }
}
