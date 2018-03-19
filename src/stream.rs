use tokio_timer;
use RetryPolicy;
use futures::{Async, Future, Poll, Stream};

/// Takes a stream that creates `Result` objects, process the possible errors and return the
/// result.
///
/// For example, if you have a `Stream` which `::Item` is `Result<T, E>` and you want to propagate
/// the error up, so in the end you'll get a `Stream` with `::Item = T`, this is your choice. Also
/// `E` should be convertible into `Stream::Error`.
pub struct StreamRetry<F, S> {
    error_action: F,
    stream: S,
    timer: tokio_timer::Timer,
    state: RetryState,
}

/// An extention trait for `Stream` which allows to use `StreamRetry` in a chain-like manner.
pub trait StreamRetryExt: Stream {
    /// Converts the stream into a **retry stream**. See `StreamRetry::new` for details.
    fn into_retry<F, ExtErr>(self, error_action: F) -> StreamRetry<F, Self>
    where
        F: FnMut(Self::Error) -> RetryPolicy<ExtErr>,
        Self: Sized,
    {
        StreamRetry::new(self, error_action)
    }
}

impl<S: ?Sized> StreamRetryExt for S
where
    S: Stream,
{
}

enum RetryState {
    WaitingForStream,
    TimerActive(tokio_timer::Sleep),
}

impl<F, S> StreamRetry<F, S> {
    pub fn new<ExtErr>(stream: S, error_action: F) -> Self
    where
        S: Stream,
        F: FnMut(S::Error) -> RetryPolicy<ExtErr>,
    {
        Self {
            error_action,
            stream,
            timer: tokio_timer::Timer::default(),
            state: RetryState::WaitingForStream,
        }
    }
}

impl<F, S, ExtErr> Stream for StreamRetry<F, S>
where
    S: Stream,
    F: FnMut(S::Error) -> RetryPolicy<ExtErr>,
{
    type Item = S::Item;
    type Error = ExtErr;

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
                    Ok(x) => return Ok(x),
                    Err(e) => match (self.error_action)(e) {
                        RetryPolicy::ForwardError(e) => return Err(e),
                        RetryPolicy::Repeat => RetryState::WaitingForStream,
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
mod test {
    use super::*;
    use futures::stream::iter_result;
    use std::time::Duration;

    #[test]
    fn naive() {
        let stream = iter_result(vec![Ok::<_, u8>(17), Ok(19)]);
        let retry = StreamRetry::new(stream, |_| RetryPolicy::Repeat::<()>);
        assert_eq!(Ok(vec![17, 19]), retry.collect().wait());
    }

    #[test]
    fn repeat() {
        let stream = iter_result(vec![Ok(1), Err(17), Ok(19)]);
        let retry = StreamRetry::new(stream, |_| RetryPolicy::Repeat::<()>);
        assert_eq!(Ok(vec![1, 19]), retry.collect().wait());
    }

    #[test]
    fn wait() {
        let stream = iter_result(vec![Err(17), Ok(19)]);
        let retry = StreamRetry::new(stream, |_| {
            RetryPolicy::WaitRetry::<()>(Duration::from_millis(10))
        });
        assert_eq!(Ok(vec![19]), retry.collect().wait());
    }

    #[test]
    fn propagate() {
        let stream = iter_result(vec![Err(17u8), Ok(19u16)]);
        let mut retry = StreamRetry::new(stream, RetryPolicy::ForwardError);
        assert_eq!(Err(17u8), retry.poll());
    }

    #[test]
    fn propagate_ext() {
        let mut stream =
            iter_result(vec![Err(17u8), Ok(19u16)]).into_retry(RetryPolicy::ForwardError);
        assert_eq!(Err(17u8), stream.poll());
    }
}
