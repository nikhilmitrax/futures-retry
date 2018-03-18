use tokio_timer;
use RetryPropagatePolicy;
use futures::{Async, Future, Poll, Stream};

pub struct StreamRetryPropagate<R, S> {
    error_action: R,
    stream: S,
    timer: tokio_timer::Timer,
    state: RetryState,
}

enum RetryState {
    WaitingForStream,
    TimerActive(tokio_timer::Sleep),
}

impl<R, S> StreamRetryPropagate<R, S> {
    pub fn new<T, E>(stream: S, error_action: R) -> Self
    where
        S: Stream<Item = Result<T, E>>,
        R: FnMut(E) -> RetryPropagatePolicy<E>,
    {
        Self {
            error_action,
            stream,
            timer: tokio_timer::Timer::default(),
            state: RetryState::WaitingForStream,
        }
    }
}

impl<R, S, T, E> Stream for StreamRetryPropagate<R, S>
where
    S: Stream<Item = Result<T, E>>,
    R: FnMut(E) -> RetryPropagatePolicy<E>,
    E: From<S::Error>,
{
    type Item = T;
    type Error = E;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        loop {
            let new_state = match self.state {
                RetryState::TimerActive(ref mut sleep) => match sleep.poll() {
                    Ok(Async::Ready(())) => RetryState::WaitingForStream,
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
                RetryState::WaitingForStream => match self.stream.poll()? {
                    Async::NotReady => return Ok(Async::NotReady),
                    Async::Ready(res) => match res {
                        None => return Ok(Async::Ready(None)),
                        Some(Ok(x)) => return Ok(Async::Ready(Some(x))),
                        Some(Err(e)) => match (self.error_action)(e) {
                            RetryPropagatePolicy::ForwardError(e) => return Err(e),
                            RetryPropagatePolicy::Repeat => RetryState::WaitingForStream,
                            RetryPropagatePolicy::WaitRetry(duration) => {
                                RetryState::TimerActive(self.timer.sleep(duration))
                            }
                        },
                    },
                },
            };
            self.state = new_state;
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use futures::stream::iter_ok;
    use futures::Async;
    use std::time::Duration;

    #[test]
    fn naive() {
        let stream = iter_ok::<_, u8>(vec![Ok::<_, u8>(17), Ok::<_, u8>(19)]);
        let mut retry = StreamRetryPropagate::new(stream, |_| RetryPropagatePolicy::Repeat);
        assert_eq!(Ok(Async::Ready(Some(17))), retry.poll());
        assert_eq!(Ok(Async::Ready(Some(19))), retry.poll());
        assert_eq!(Ok(Async::Ready(None)), retry.poll());
    }

    #[test]
    fn repeat() {
        let stream = iter_ok(vec![Err(17u8), Ok(19u8)]);
        let mut retry = StreamRetryPropagate::new(stream, |_| RetryPropagatePolicy::Repeat);
        assert_eq!(Ok(Async::Ready(Some(19))), retry.poll());
        assert_eq!(Ok(Async::Ready(None)), retry.poll());
    }

    #[test]
    fn wait() {
        let stream = iter_ok(vec![Err(17u8), Ok(19u8)]);
        let mut retry = StreamRetryPropagate::new(stream, |_| {
            RetryPropagatePolicy::WaitRetry(Duration::from_millis(10))
        });
        assert_eq!(Ok(Async::Ready(Some(19))), retry.poll());
        assert_eq!(Ok(Async::Ready(None)), retry.poll());
    }

    #[test]
    fn propagate() {
        let stream = iter_ok(vec![Err(17u8), Ok(19u16)]);
        let mut retry = StreamRetryPropagate::new(stream, RetryPropagatePolicy::ForwardError);
        assert_eq!(Err(17u8), retry.poll());
    }
}
