use tokio_timer;
use RetryPolicy;
use futures::{Async, Future, Poll, Stream};

/// Provides a way to handle errors during a `Stream` execution, i.e. it gives you an ability to
/// poll for future stream's items with a delay.
///
/// This type is similar to [`FutureRetry`](struct.FutureRetry.html), but with a different
/// semantics. For example, if for [`FutureRetry`](struct.FutureRetry.html) we need a factory that
/// creates `Future`s, we don't need one for `Stream`s, since `Stream` itself is a natural producer
/// of new items, so we don't have to recreated it if an error is encountered.
///
/// A typical usage might be recovering from connection errors while trying to accept a connection
/// on a TCP server.
///
/// A `tcp-listener` example is available in the `examples` folder.
///
/// Also have a look at [`StreamRetryExt`](trait.StreamRetryExt.html) trait for a more convenient
/// usage.
pub struct StreamRetry<F, S> {
    error_action: F,
    stream: S,
    timer: tokio_timer::Timer,
    state: RetryState,
}

/// An extention trait for `Stream` which allows to use `StreamRetry` in a chain-like manner.
///
/// # Example
///
/// This magic trait allows you to handle errors on streams in a very neat manner:
///
/// ```
/// extern crate futures_retry;
/// // ...
/// # extern crate tokio;
/// use futures_retry::{RetryPolicy, StreamRetryExt};
/// # use std::io;
/// # use std::time::Duration;
/// # use tokio::net::{TcpListener, TcpStream};
/// # use tokio::prelude::*;
///
/// fn handle_error(e: io::Error) -> RetryPolicy<io::Error> {
///   match e.kind() {
///     io::ErrorKind::Interrupted => RetryPolicy::Repeat,
///     io::ErrorKind::PermissionDenied => RetryPolicy::ForwardError(e),
///     _ => RetryPolicy::WaitRetry(Duration::from_millis(5)),
///   }
/// }
///
/// fn serve_connection(stream: TcpStream) -> Box<Future<Item = (), Error = ()> + Send> {
///   // ...
///   # unimplemented!()
/// }
///
/// fn main() {
///   let listener: TcpListener = // ...
///   # TcpListener::bind(&"[::]:0".parse().unwrap()).unwrap();
///   let server = listener.incoming()
///     .into_retry(handle_error)
///     .and_then(|stream| {
///       tokio::spawn(serve_connection(stream));
///       Ok(())
///     })
///     .for_each(|_| Ok(()))
///     .map_err(|e| eprintln!("Caught an error {}", e));
///   # let server = server.select(Ok(())).map(|(_, _)| ()).map_err(|(_, _)| ());
///   tokio::run(server);
/// }
/// ```
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
