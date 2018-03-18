use tokio_timer;
use RetryPolicy;
use futures::{Async, Future, Poll, Stream};

pub struct StreamRetry<R, S> {
    error_action: R,
    stream: S,
    timer: tokio_timer::Timer,
    state: RetryState,
}

/// An extention trait for `Stream` which allows to use `StreamRetry` in a chain-like manner.
pub trait StreamRetryExt<E>: Sized {
    /// Converts the stream in a **retry stream**. See `StreamRetry::new` for details.
    fn into_retry<R>(self, error_action: R) -> StreamRetry<R, Self>
    where
        R: FnMut(&E) -> RetryPolicy;
}

impl<S, T, E> StreamRetryExt<E> for S
where
    S: Stream<Item = Result<T, E>>,
{
    fn into_retry<R>(self, error_action: R) -> StreamRetry<R, Self>
    where
        R: FnMut(&E) -> RetryPolicy,
    {
        StreamRetry::new(self, error_action)
    }
}

enum RetryState {
    WaitingForStream,
    TimerActive(tokio_timer::Sleep),
}

impl<R, S> StreamRetry<R, S> {
    pub fn new<T, E>(stream: S, error_action: R) -> Self
    where
        S: Stream<Item = Result<T, E>>,
        R: FnMut(&E) -> RetryPolicy,
    {
        Self {
            error_action,
            stream,
            timer: tokio_timer::Timer::default(),
            state: RetryState::WaitingForStream,
        }
    }
}

impl<R, S, T, E> Stream for StreamRetry<R, S>
where
    S: Stream<Item = Result<T, E>>,
    R: FnMut(&E) -> RetryPolicy,
{
    type Item = S::Item;
    type Error = S::Error;

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
                RetryState::WaitingForStream => match self.stream.poll() {
                    Ok(Async::NotReady) => return Ok(Async::NotReady),
                    Ok(Async::Ready(res)) => match res {
                        None => return Ok(Async::Ready(None)),
                        Some(Ok(x)) => return Ok(Async::Ready(Some(Ok(x)))),
                        Some(Err(e)) => match (self.error_action)(&e) {
                            RetryPolicy::ForwardError => return Ok(Async::Ready(Some(Err(e)))),
                            RetryPolicy::Repeat => RetryState::WaitingForStream,
                            RetryPolicy::WaitRetry(duration) => {
                                RetryState::TimerActive(self.timer.sleep(duration))
                            }
                        },
                    },
                    Err(e) => return Err(e),
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
        let stream = iter_ok::<_, ()>(vec![Ok::<_, u8>(17), Ok::<_, u8>(19)]);
        let mut retry = StreamRetry::new(stream, |_| RetryPolicy::Repeat);
        assert_eq!(Ok(Async::Ready(Some(Ok(17)))), retry.poll());
        assert_eq!(Ok(Async::Ready(Some(Ok(19)))), retry.poll());
        assert_eq!(Ok(Async::Ready(None)), retry.poll());
    }

    #[test]
    fn repeat() {
        let stream = iter_ok::<_, ()>(vec![Err::<u8, u8>(17), Ok::<u8, u8>(19)]);
        let mut retry = StreamRetry::new(stream, |_| RetryPolicy::Repeat);
        assert_eq!(Ok(Async::Ready(Some(Ok(19)))), retry.poll());
        assert_eq!(Ok(Async::Ready(None)), retry.poll());
    }

    #[test]
    fn wait() {
        let stream = iter_ok::<_, ()>(vec![Err::<u8, u8>(17), Ok::<u8, u8>(19)]);
        let mut retry = StreamRetry::new(stream, |_| {
            RetryPolicy::WaitRetry(Duration::from_millis(10))
        });
        assert_eq!(Ok(Async::Ready(Some(Ok(19)))), retry.poll());
        assert_eq!(Ok(Async::Ready(None)), retry.poll());
    }

    #[test]
    fn forward() {
        let stream = iter_ok::<_, ()>(vec![Err::<u8, u8>(17), Ok::<u8, u8>(19)]);
        let mut retry = StreamRetry::new(stream, |_| RetryPolicy::ForwardError);
        assert_eq!(Ok(Async::Ready(Some(Err(17)))), retry.poll());
        assert_eq!(Ok(Async::Ready(Some(Ok(19)))), retry.poll());
        assert_eq!(Ok(Async::Ready(None)), retry.poll());
    }

    #[test]
    fn forward_ext() {
        let mut stream = iter_ok::<_, ()>(vec![Err::<u8, u8>(17), Ok::<u8, u8>(19)])
            .into_retry(|_| RetryPolicy::ForwardError);
        assert_eq!(Ok(Async::Ready(Some(Err(17)))), stream.poll());
        assert_eq!(Ok(Async::Ready(Some(Ok(19)))), stream.poll());
        assert_eq!(Ok(Async::Ready(None)), stream.poll());
    }
}
