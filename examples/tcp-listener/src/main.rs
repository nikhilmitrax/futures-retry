extern crate futures_retry;
extern crate tokio;

use tokio::prelude::*;
use tokio::io;
use tokio::net::{TcpListener, TcpStream};
use futures_retry::{RetryPropagatePolicy, StreamRetryPropagateExt};
use std::time::Duration;

struct TcpListenerAcceptStream(TcpListener);

impl Stream for TcpListenerAcceptStream {
    type Item = Result<TcpStream, io::Error>;
    type Error = io::Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        match self.0.poll_accept() {
            Err(e) => Ok(Async::Ready(Some(Err(e)))),
            Ok(Async::Ready((stream, _))) => Ok(Async::Ready(Some(Ok(stream)))),
            Ok(Async::NotReady) => Ok(Async::NotReady),
        }
    }
}

fn main() {
    let addr = "127.0.0.1:12345".parse().unwrap();
    let tcp = TcpListener::bind(&addr).unwrap();

    let server = TcpListenerAcceptStream(tcp)
        .into_retry(|e| match e.kind() {
            io::ErrorKind::Interrupted
            | io::ErrorKind::ConnectionRefused
            | io::ErrorKind::ConnectionReset
            | io::ErrorKind::ConnectionAborted
            | io::ErrorKind::NotConnected
            | io::ErrorKind::BrokenPipe => RetryPropagatePolicy::Repeat,
            io::ErrorKind::PermissionDenied => RetryPropagatePolicy::ForwardError(e),
            _ => RetryPropagatePolicy::WaitRetry(Duration::from_millis(5)),
        })
        .for_each(|tcp| {
            let (reader, writer) = tcp.split();
            // Copy the data back to the client
            let conn = io::copy(reader, writer)
            // print what happened
            .map(|(n, _, _)| {
                println!("wrote {} bytes", n)
            })
            // Handle any errors
            .map_err(|err| {
                println!("IO error {:?}", err)
            });

            // Spawn the future as a concurrent task
            tokio::spawn(conn);
            Ok(())
        })
        .map_err(|err| {
            println!("server error {:?}", err);
        });
    tokio::run(server);
}
