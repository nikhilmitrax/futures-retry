use futures::{Async, Future, Poll, Stream};
use std::time::Instant;
use tokio_timer;
use {ErrorHandler, RetryPolicy};

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
/// fn serve_connection(stream: TcpStream) -> impl Future<Item = (), Error = ()> + Send {
///   // ...
///   # future::result(Ok(()))
/// }
///
/// fn main() {
///   let listener: TcpListener = // ...
///   # TcpListener::bind(&"[::]:0".parse().unwrap()).unwrap();
///   let server = listener.incoming()
///     .retry(handle_error)
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
    fn retry<F>(self, error_action: F) -> StreamRetry<F, Self>
    where
        Self: Sized,
    {
        StreamRetry::new(self, error_action)
    }
}

impl<S: ?Sized> StreamRetryExt for S where S: Stream {}

enum RetryState {
    WaitingForStream,
    TimerActive(tokio_timer::Delay),
}

impl<F, S> StreamRetry<F, S> {
    /// Creates a `StreamRetry` using a provided stream and an object of `ErrorHandler` type that
    /// decides on a retry-policy depending on an encountered error.
    ///
    /// Please refer to the `tcp-listener` example in the `examples` folder to have a look at a
    /// possible usage or to a very convenient extension trait
    /// [`StreamRetryExt`](trait.StreamRetryExt.html).
    ///
    /// # Arguments
    ///
    /// * `stream`: a stream of future items,
    /// * `error_action`: a type that handles an error and decides which route to take: simply
    ///                   try again, wait and then try, or give up (on a critical error for
    ///                   exapmle).
    pub fn new(stream: S, error_action: F) -> Self
    where
        S: Stream,
    {
        Self {
            error_action,
            stream,
            state: RetryState::WaitingForStream,
        }
    }
}

impl<F, S> Stream for StreamRetry<F, S>
where
    S: Stream,
    F: ErrorHandler<S::Error>,
{
    type Item = S::Item;
    type Error = F::OutError;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        loop {
            let new_state = match self.state {
                RetryState::TimerActive(ref mut delay) => match delay.poll() {
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
                    Ok(x) => {
                        self.error_action.ok();
                        return Ok(x);
                    }
                    Err(e) => match self.error_action.handle(e) {
                        RetryPolicy::ForwardError(e) => return Err(e),
                        RetryPolicy::Repeat => RetryState::WaitingForStream,
                        RetryPolicy::WaitRetry(duration) => RetryState::TimerActive(
                            tokio_timer::Delay::new(Instant::now() + duration),
                        ),
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
    use tokio;

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
        })
        .collect()
        .then(|x| {
            assert_eq!(Ok(vec![19]), x);
            Ok(())
        });
        tokio::run(retry);
    }

    #[test]
    fn propagate() {
        let stream = iter_result(vec![Err(17u8), Ok(19u16)]);
        let mut retry = StreamRetry::new(stream, RetryPolicy::ForwardError);
        assert_eq!(Err(17u8), retry.poll());
    }

    #[test]
    fn propagate_ext() {
        let mut stream = iter_result(vec![Err(17u8), Ok(19u16)]).retry(RetryPolicy::ForwardError);
        assert_eq!(Err(17u8), stream.poll());
    }
}
