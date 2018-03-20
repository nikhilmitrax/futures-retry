extern crate futures_retry;
extern crate tokio;

use tokio::prelude::*;
use tokio::io;
use tokio::net::TcpStream;
use futures_retry::{FutureRetry, RetryPolicy};
use std::time::Duration;

fn handle_error(e: io::Error) -> RetryPolicy<io::Error> {
    match e.kind() {
        io::ErrorKind::Interrupted
        | io::ErrorKind::ConnectionRefused
        | io::ErrorKind::ConnectionReset
        | io::ErrorKind::ConnectionAborted
        | io::ErrorKind::NotConnected
        | io::ErrorKind::BrokenPipe => RetryPolicy::Repeat,
        io::ErrorKind::PermissionDenied => RetryPolicy::ForwardError(e),
        _ => RetryPolicy::WaitRetry(Duration::from_millis(5)),
    }
}

fn main() {
    let addr = "127.0.0.1:12345".parse().unwrap();
    let res = FutureRetry::new(
        || {
            FutureRetry::new(|| TcpStream::connect(&addr), handle_error).and_then(|tcp| {
                let (_, mut writer) = tcp.split();
                writer.write_all(b"Yo!")
            })
        },
        handle_error,
    ).wait();
    match res {
        Ok(_) => println!("Done"),
        Err(e) => println!("Write attempt failed: {}", e),
    }
}
