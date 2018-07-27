extern crate futures_retry;
extern crate tokio;

use futures_retry::{FutureRetry, RetryPolicy};
use std::net::SocketAddr;
use std::time::Duration;
use tokio::io;
use tokio::net::TcpStream;
use tokio::prelude::*;

/// Handles an I/O error.
fn handle_io_error(e: io::Error, attempts_left: usize) -> RetryPolicy<io::Error> {
    println!("{} attempts left", attempts_left);
    match e.kind() {
        io::ErrorKind::Interrupted
        | io::ErrorKind::ConnectionReset
        | io::ErrorKind::ConnectionAborted
        | io::ErrorKind::NotConnected
        | io::ErrorKind::BrokenPipe => RetryPolicy::Repeat,
        io::ErrorKind::ConnectionRefused => RetryPolicy::WaitRetry(Duration::from_secs(5)),
        _ => RetryPolicy::ForwardError(e),
    }
}

/// Creates a closure-like instance that will handle an error for a limited amount of times, e.g.
/// after reaching the limit an error will be simply forwarded.
fn make_limited_handler(
    attempts: usize,
    mut handler: impl FnMut(io::Error, usize) -> RetryPolicy<io::Error>,
) -> impl FnMut(io::Error) -> RetryPolicy<io::Error> {
    let mut attempts_left = attempts;
    move |e| {
        println!("Attempt {}/{}", (attempts - attempts_left + 1), attempts);
        if attempts_left == 1 {
            RetryPolicy::ForwardError(e)
        } else {
            attempts_left -= 1;
            handler(e, attempts_left)
        }
    }
}

/// In this function we try to establish a connection to a given address for 3 times, and then try
/// to send some data exactly once.
fn connect_and_send(addr: SocketAddr) -> impl Future<Item = (), Error = io::Error> {
    // Try to connect until we succeed or until an unrecoverable error is encountered.
    let connection = FutureRetry::new(
        move || {
            println!("Trying to connect to {}", addr);
            TcpStream::connect(&addr)
        },
        make_limited_handler(3, handle_io_error),
    );
    connection.and_then(|tcp| {
        let (_, mut writer) = tcp.split();
        writer.write_all(b"Yo!")
    })
}

fn main() {
    let addr = "127.0.0.1:12345".parse().unwrap();
    /// Try to connect and send data 2 times.
    let action = FutureRetry::new(
        move || {
            println!("Trying to connect and to send data");
            connect_and_send(addr)
        },
        make_limited_handler(2, handle_io_error),
    ).map_err(|e| eprintln!("Connect and send has failed: {}", e))
        .map(|_| println!("Done"));
    tokio::run(action);
}
