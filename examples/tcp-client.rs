extern crate futures_retry;
extern crate tokio;

use futures_retry::{FutureRetry, RetryPolicy};
use std::time::Duration;
use tokio::io;
use tokio::net::TcpStream;
use tokio::prelude::*;

fn handle_connection_error(e: io::Error) -> RetryPolicy<io::Error> {
    // This is kinda unrealistical error handling, don't use it as it is!
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
    // Try to connect until we succeed or until an unrecoverable error is encountered.
    let connection = FutureRetry::new(|| TcpStream::connect(&addr), handle_connection_error);
    // .. and then try to write some data only once. If you want to retry on an error here as
    // well, wrap up the whole `let connection = ...` & `let res = ...` in a `FutureRetry`.
    let res = connection
        .and_then(|tcp| {
            let (_, mut writer) = tcp.split();
            writer.write_all(b"Yo!")
        })
        .wait();
    match res {
        Ok(_) => println!("Done"),
        Err(e) => println!("Write attempt failed: {}", e),
    }
}
