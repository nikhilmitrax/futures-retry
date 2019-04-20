use crate::{ErrorHandler, RetryPolicy};
use futures::{Async, AsyncSink, Future, Poll, Sink, StartSend};
use std::time::Instant;
use tokio_timer;

/// Provides a way to handle errors during a `Sink` execution, i.e. it gives you an ability to
/// flush item's with delay.
///
/// This type is similar to [`StreamRetry`](struct.StreamRetry.html). The diffrence is that
/// SinkRetry is more limited:
/// * SinkItem has to implement `Clone` trait.
/// * error_action `OutError` has to implement `From<SinkError>` trait.
///
/// A `fs-readwrite` example is available in the `examples` folder.
/// A `tcp-copy` example handle StreamRetry and SinkRetry example at once.
///
/// A typical usage might be recovering from errors when flushing data to I/O.
///
/// # Warning (Implementation details)
///
/// It depends from inner sink what happen when inner `start_send()` is resolve to error. This
/// function assume that error has the same meaning as `AsyncSink::NotReady`. If your item will
/// be buffered before error was returned - SinkRetry will insert this item again anyway.
pub struct SinkRetry<F, S> {
    error_action: F,
    sink: S,
    state: RetryState,
}

impl<F, S> SinkRetry<F, S>
where
    S: Sink,
    S::SinkItem: Clone,
    F: ErrorHandler<S::SinkError>,
    F::OutError: From<S::SinkError>,
{
    /// Creates a `SinkRetry` using a provided sink and an object of `ErrorHandler` type that
    /// decides on a retry-policy depending on an encountered error.
    ///
    /// # Arguments
    ///
    /// * `sink`: a sink to be filled,
    /// * `error_action`: a type that handles an error and decides which route to take: simply
    ///                   try again, wait and then try, or give up (on a critical error for
    ///                   exapmle).
    /// # Notes
    /// More common use is in like chain manner. See [SinkRetryExt](trait.SinkRetryExt.html)
    pub fn new(sink: S, error_action: F) -> Self
    where
        S: Sink,
    {
        Self {
            error_action,
            sink,
            state: RetryState::WaitingForSink,
        }
    }

    fn try_send_item(&mut self, item: S::SinkItem) -> StartSend<S::SinkItem, F::OutError> {
        debug_assert!(self.state.is_waiting_for_sink());
        loop {
            let cloned_item = item.clone();
            match self.sink.start_send(cloned_item) {
                Err(err) => {
                    match self.error_action.handle(err) {
                        RetryPolicy::Repeat => continue,
                        RetryPolicy::WaitRetry(duration) => {
                            // before return AsyncSink::NotReady(item) we HAVE TO call timer poll
                            // function.
                            let mut timer = tokio_timer::Delay::new(Instant::now() + duration);

                            match timer.poll().expect("Timer panic!") {
                                Async::Ready(_) => match self.poll_complete()? {
                                    Async::Ready(()) => continue,
                                    Async::NotReady => {}
                                },
                                Async::NotReady => {}
                            }

                            self.state = RetryState::TimerActive(timer);
                            return Ok(AsyncSink::NotReady(item));
                        }
                        RetryPolicy::ForwardError(err) => return Err(err),
                    }
                }
                Ok(ok) => return Ok(ok),
            }
        }
    }
}

/// An extention trait for `Sink` which allows to use `SinkRetry` in a chain-like manner.
///
/// # Example
///
/// This magic trait allows you to handle errors on sink in a very neat manner:
/// ```
/// # use futures_retry::{RetryPolicy, SinkRetryExt, StreamRetryExt};
/// # use std::time::Duration;
/// # use tokio::io;
/// # use tokio::net::TcpListener;
/// # use tokio::prelude::*;
///
/// fn main() {
///      let addr = "127.0.0.1:12345".parse().unwrap();
///      let tcp = TcpListener::bind(&addr).unwrap();
///
///     let conn_error_handler = |e: io::Error| match e.kind() {
///         io::ErrorKind::Interrupted
///         | io::ErrorKind::ConnectionRefused
///         | io::ErrorKind::ConnectionReset
///         | io::ErrorKind::ConnectionAborted
///         | io::ErrorKind::NotConnected
///         | io::ErrorKind::BrokenPipe => RetryPolicy::Repeat,
///         io::ErrorKind::PermissionDenied => RetryPolicy::ForwardError(e),
///         _ => RetryPolicy::WaitRetry(Duration::from_millis(5)),
///     };
///
///     let data_sending_error_handler = |e: io::Error| match e.kind() {
///         io::ErrorKind::Interrupted => RetryPolicy::Repeat,
///         io::ErrorKind::TimedOut | io::ErrorKind::InvalidInput | io::ErrorKind::InvalidData => {
///             RetryPolicy::WaitRetry(Duration::from_millis(5))
///         }
///         _ => RetryPolicy::ForwardError(e),
///     };
///
///     let server = tcp
///         .incoming()
///         .retry(conn_error_handler)
///         .for_each(move |tcp| {
///             let (reader, writer) = tcp.split();
///
///             let reader = tokio::codec::FramedRead::new(reader, tokio::codec::LinesCodec::new());
///             let writer = tokio::codec::FramedWrite::new(writer, tokio::codec::LinesCodec::new());
///             // Copy the data back to the client
///
///             let conn = writer
///                 .retry(data_sending_error_handler) // retry
///                 .send_all(reader.retry(data_sending_error_handler))
///                 // when future is resolved sink and stream is returned we just drop them.
///                 .map(drop)
///                 // Handle any errors
///                 .map_err(|err| eprintln!("Can't copy data: IO error {:?}", err));
///
///             // Spawn the future as a concurrent task
///             tokio::spawn(conn);
///             Ok(())
///         })
///         .map_err(|err| {
///             eprintln!("server error {:?}", err);
///         });
///     tokio::run(server.select(Ok(())).map(|(_, _)| ()).map_err(|(_, _)| ()));
/// }
/// ```
pub trait SinkRetryExt: Sink {
    /// Converts the sink into a **retry sink**. See `SinkRetry::new` for details.
    ///
    /// # Warning (Implementation details)
    ///
    /// It depends from inner sink what happen when inner `start_send()` is resolve to error. This
    /// function assume that error has the same meaning as `AsyncSink::NotReady`. If your item will
    /// be buffered before error was returned - SinkRetry will insert this item again anyway.
    fn retry<F>(self, error_action: F) -> SinkRetry<F, Self>
    where
        Self: Sized,
        F: ErrorHandler<Self::SinkError>,
        Self::SinkItem: Clone,
        F: ErrorHandler<Self::SinkError>,
        F::OutError: From<Self::SinkError>,
    {
        SinkRetry::new(self, error_action)
    }
}

impl<S: ?Sized> SinkRetryExt for S where S: Sink {}

enum RetryState {
    WaitingForSink,
    TimerActive(tokio_timer::Delay),
}
impl RetryState {
    #[inline]
    fn is_waiting_for_sink(&self) -> bool {
        match self {
            RetryState::WaitingForSink => true,
            _ => false,
        }
    }
}

impl<F, S> Sink for SinkRetry<F, S>
where
    S: Sink,
    S::SinkItem: Clone,
    F: ErrorHandler<S::SinkError>,
    F::OutError: From<S::SinkError>,
{
    /// The type of value that the sink accepts.
    type SinkItem = S::SinkItem;

    /// The type of value produced by the sink when an error occurs.
    type SinkError = F::OutError;

    fn start_send(&mut self, item: Self::SinkItem) -> StartSend<Self::SinkItem, Self::SinkError> {
        match self.state {
            RetryState::WaitingForSink => self.try_send_item(item),
            RetryState::TimerActive(ref mut timer) => match timer.poll().expect("Timer panic!") {
                Async::NotReady => Ok(AsyncSink::NotReady(item)),
                Async::Ready(()) => self.try_send_item(item),
            },
        }
    }

    fn poll_complete(&mut self) -> Poll<(), Self::SinkError> {
        loop {
            let new_state = match self.state {
                //FIXME BROKEN RULE: DON'T REAPEAT YOURSELF -- the same code is in stream.rs!
                RetryState::TimerActive(ref mut delay) => match delay.poll() {
                    Ok(Async::Ready(())) => RetryState::WaitingForSink,
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
                RetryState::WaitingForSink => match self.sink.poll_complete() {
                    Ok(x) => {
                        self.error_action.ok();
                        return Ok(x);
                    }
                    Err(e) => match self.error_action.handle(e) {
                        RetryPolicy::ForwardError(e) => return Err(e),
                        RetryPolicy::Repeat => RetryState::WaitingForSink,
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
mod tests {
    use super::*;
    use std::marker::PhantomData;

    enum SinkReturn {
        ReadyToFlush,
        NotReadyToFlush,
    }

    struct SinkIterResultMock<T, I> {
        iter: I,
        _sink_item: PhantomData<T>,
    }

    fn iter_result<T, J, E>(i: J) -> SinkIterResultMock<T, J::IntoIter>
    where
        J: IntoIterator<Item = Result<SinkReturn, E>>,
    {
        SinkIterResultMock {
            iter: i.into_iter(),
            _sink_item: PhantomData,
        }
    }

    impl<T, I, E> Sink for SinkIterResultMock<T, I>
    where
        I: Iterator<Item = Result<SinkReturn, E>>,
    {
        type SinkItem = T;
        type SinkError = E;

        fn start_send(
            &mut self,
            item: Self::SinkItem,
        ) -> StartSend<Self::SinkItem, Self::SinkError> {
            match self.poll_complete()? {
                Async::Ready(()) => Ok(AsyncSink::Ready),
                Async::NotReady => Ok(AsyncSink::NotReady(item)),
            }
        }

        fn poll_complete(&mut self) -> Poll<(), E> {
            match self.iter.next().expect("Iterator called after done!")? {
                SinkReturn::ReadyToFlush => Ok(Async::Ready(())),
                SinkReturn::NotReadyToFlush => Ok(Async::NotReady),
            }
        }
    }

    #[test]
    fn get_item_when_error_is_handling() {
        let sink = iter_result(vec![
            Ok(SinkReturn::NotReadyToFlush),
            Err(17u8),
            Ok(SinkReturn::NotReadyToFlush),
            Ok(SinkReturn::NotReadyToFlush),
        ]);
        let mut retry = sink.retry(|_| RetryPolicy::Repeat::<u8>);

        assert_eq!(Ok(AsyncSink::NotReady(5)), retry.start_send(5));
        assert_eq!(Ok(AsyncSink::NotReady(7)), retry.start_send(7));
        assert_eq!(Ok(AsyncSink::NotReady(8)), retry.start_send(8));
    }

    #[test]
    fn repeat() {
        let sink = iter_result::<u8, _, _>(vec![
            Ok(SinkReturn::ReadyToFlush),
            Err(17u64),
            Ok(SinkReturn::ReadyToFlush),
            Ok(SinkReturn::NotReadyToFlush),
        ]);
        let mut retry = SinkRetry::new(sink, |_| RetryPolicy::Repeat::<u64>);

        assert_eq!(Ok(AsyncSink::Ready), retry.start_send(2));
        assert_eq!(Ok(AsyncSink::Ready), retry.start_send(2));
        assert_eq!(Ok(AsyncSink::NotReady(2)), retry.start_send(2));
    }

    #[test]
    fn propagate() {
        let sink = iter_result::<u8, _, _>(vec![Err(17u8), Ok(SinkReturn::ReadyToFlush)]);
        let mut retry = SinkRetry::new(sink, RetryPolicy::ForwardError);
        assert_eq!(Err(17u8), retry.start_send(3));
        assert_eq!(Ok(AsyncSink::Ready), retry.start_send(3));
    }
}
