use crate::{ErrorHandler, RetryPolicy};
use futures::{ready, task::Context, Future, Poll, Stream, TryStream};
use std::{pin::Pin, time::Instant};
use tokio::timer;

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
/// #![feature(async_await)]
/// // ...
/// use futures_retry::{RetryPolicy, StreamRetryExt};
/// # use futures::{TryStreamExt, TryFutureExt, future::{ok, select}, FutureExt};
/// # use std::io;
/// # use std::time::Duration;
/// # use tokio::net::{TcpListener, TcpStream};
///
/// fn handle_error(e: io::Error) -> RetryPolicy<io::Error> {
///   match e.kind() {
///     io::ErrorKind::Interrupted => RetryPolicy::Repeat,
///     io::ErrorKind::PermissionDenied => RetryPolicy::ForwardError(e),
///     _ => RetryPolicy::WaitRetry(Duration::from_millis(5)),
///   }
/// }
///
/// async fn serve_connection(stream: TcpStream) {
///   // ...
/// }
///
/// #[tokio::main]
/// async fn main() {
///   let listener: TcpListener = // ...
///   # TcpListener::bind(&"[::]:0".parse().unwrap()).unwrap();
///   let server = listener.incoming()
///     .retry(handle_error)
///     .and_then(|stream| {
///       tokio::spawn(serve_connection(stream));
///       ok(())
///     })
///     .try_for_each(|_| ok(()))
///     .map_err(|e| eprintln!("Caught an error {}", e));
///   # // This nasty hack is required to exit immediately when running the doc tests.
///   # let server = select(ok::<_, ()>(()), server).map(|_| ());
///   server.await
/// }
/// ```
pub trait StreamRetryExt: TryStream {
    /// Converts the stream into a **retry stream**. See `StreamRetry::new` for details.
    fn retry<F>(self, error_action: F) -> StreamRetry<F, Self>
    where
        Self: Sized,
    {
        StreamRetry::new(self, error_action)
    }
}

impl<S: ?Sized> StreamRetryExt for S where S: TryStream {}

enum RetryState {
    WaitingForStream,
    TimerActive(timer::Delay),
}

impl<F, S> StreamRetry<F, S> {
    pin_utils::unsafe_pinned!(stream: S);
    pin_utils::unsafe_pinned!(state: RetryState);
    pin_utils::unsafe_pinned!(error_action: F);

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
        S: TryStream,
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
    S: TryStream,
    F: ErrorHandler<S::Error>,
{
    type Item = Result<S::Ok, F::OutError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        loop {
            let new_state = match self.as_mut().state().get_mut() {
                RetryState::TimerActive(delay) => {
                    let delay = unsafe { Pin::new_unchecked(delay) };
                    ready!(delay.poll(cx));
                    RetryState::WaitingForStream
                }
                RetryState::WaitingForStream => {
                    match ready!(self.as_mut().stream().try_poll_next(cx)) {
                        Some(Ok(x)) => {
                            self.as_mut().error_action().ok();
                            return Poll::Ready(Some(Ok(x)));
                        }
                        None => {
                            return Poll::Ready(None);
                        }
                        Some(Err(e)) => match self.as_mut().error_action().handle(e) {
                            RetryPolicy::ForwardError(e) => return Poll::Ready(Some(Err(e))),
                            RetryPolicy::Repeat => RetryState::WaitingForStream,
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
mod test {
    use super::*;
    use futures::{executor::block_on_stream, stream::iter, TryFutureExt, TryStreamExt};
    use std::time::Duration;

    #[test]
    fn naive() {
        let stream = iter(vec![Ok::<_, u8>(17), Ok(19)]);
        let retry = StreamRetry::new(stream, |_| RetryPolicy::Repeat::<()>);
        assert_eq!(
            Ok(vec![17, 19]),
            block_on_stream(retry.into_stream()).collect()
        );
    }

    #[test]
    fn repeat() {
        let stream = iter(vec![Ok(1), Err(17), Ok(19)]);
        let retry = StreamRetry::new(stream, |_| RetryPolicy::Repeat::<()>);
        assert_eq!(
            Ok(vec![1, 19]),
            block_on_stream(retry.into_stream()).collect()
        );
    }

    #[test]
    fn wait() {
        let stream = iter(vec![Err(17), Ok(19)]);
        let retry = StreamRetry::new(stream, |_| {
            RetryPolicy::WaitRetry::<()>(Duration::from_millis(10))
        })
        .try_collect()
        .into_future();
        let rt = tokio::runtime::Runtime::new().unwrap();
        assert_eq!(Ok(vec!(19)), rt.block_on(retry));
    }

    #[test]
    fn propagate() {
        let stream = iter(vec![Err(17u8), Ok(19u16)]);
        let retry = StreamRetry::new(stream, RetryPolicy::ForwardError);
        assert_eq!(Some(Err(17u8)), block_on_stream(retry.into_stream()).next());
    }
}
