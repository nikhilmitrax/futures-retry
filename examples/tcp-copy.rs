use futures_retry::{RetryPolicy, SinkRetryExt, StreamRetryExt};
use std::time::Duration;
use tokio::io;
use tokio::net::TcpListener;
use tokio::prelude::*;

fn main() {
    let addr = "127.0.0.1:12345".parse().unwrap();
    let tcp = TcpListener::bind(&addr).unwrap();

    let conn_error_handler = |e: io::Error| match e.kind() {
        io::ErrorKind::Interrupted => RetryPolicy::Repeat,
        _ => RetryPolicy::ForwardError(e),
    };

    let data_sending_error_handler = |e: io::Error| match e.kind() {
        io::ErrorKind::Interrupted => RetryPolicy::Repeat,
        io::ErrorKind::TimedOut | io::ErrorKind::InvalidInput | io::ErrorKind::InvalidData => {
            RetryPolicy::WaitRetry(Duration::from_millis(5))
        }
        _ => RetryPolicy::ForwardError(e),
    };

    let server = tcp
        .incoming()
        .retry(conn_error_handler)
        .for_each(move |tcp| {
            let (reader, writer) = tcp.split();

            let reader = tokio::codec::FramedRead::new(reader, tokio::codec::LinesCodec::new());
            let writer = tokio::codec::FramedWrite::new(writer, tokio::codec::LinesCodec::new());
            // Copy the data back to the client

            let conn = writer
                .retry(data_sending_error_handler) // retry
                .send_all(reader.retry(data_sending_error_handler))
                // when future is resolved sink and stream is returned we just drop them.
                .map(drop)
                // Handle any errors
                .map_err(|err| eprintln!("Can't copy data: IO error {:?}", err));

            // Spawn the future as a concurrent task
            tokio::spawn(conn);
            Ok(())
        })
        .map_err(|err| {
            eprintln!("server error {:?}", err);
        });
    tokio::run(server);
}
