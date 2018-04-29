extern crate futures_retry;
extern crate tokio;

use tokio::prelude::*;
use tokio::io;
use tokio::net::TcpListener;
use futures_retry::{RetryPolicy, StreamRetryExt};
use std::time::Duration;

fn main() {
    let addr = "127.0.0.1:12345".parse().unwrap();
    let tcp = TcpListener::bind(&addr).unwrap();

    let server = tcp.incoming()
        .retry(|e| match e.kind() {
            io::ErrorKind::Interrupted
            | io::ErrorKind::ConnectionRefused
            | io::ErrorKind::ConnectionReset
            | io::ErrorKind::ConnectionAborted
            | io::ErrorKind::NotConnected
            | io::ErrorKind::BrokenPipe => RetryPolicy::Repeat,
            io::ErrorKind::PermissionDenied => RetryPolicy::ForwardError(e),
            _ => RetryPolicy::WaitRetry(Duration::from_millis(5)),
        })
        .for_each(|tcp| {
            let (reader, writer) = tcp.split();
            // Copy the data back to the client
            let conn = io::copy(reader, writer)
            // print what happened
            .map(|(n, _, _)| {
                println!("Wrote {} bytes", n)
            })
            // Handle any errors
            .map_err(|err| {
                println!("Can't copy data: IO error {:?}", err)
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
