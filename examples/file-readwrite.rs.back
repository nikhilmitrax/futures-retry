use futures_retry::{RetryPolicy, SinkRetry, StreamRetryExt};
use std::fs::File;
use std::time::Duration;
use tokio::codec::{FramedRead, FramedWrite, LinesCodec};
use tokio::io;
use tokio::prelude::*;

fn handle_fs_error(e: io::Error) -> RetryPolicy<io::Error> {
    // This is kinda unrealistical error handling, don't use it as it is!
    match e.kind() {
        io::ErrorKind::Interrupted | io::ErrorKind::BrokenPipe => RetryPolicy::Repeat,
        io::ErrorKind::PermissionDenied => RetryPolicy::ForwardError(e),
        _ => RetryPolicy::WaitRetry(Duration::from_millis(5)),
    }
}

fn main() {
    let rd_file = File::open("tests/rd_lines.txt").expect("File exist");
    let wr_file = File::create("tests/wr_lines.txt").expect("read-write fs");

    // We implement Async for files.
    let rd_file = tokio_fs::File::from_std(rd_file);
    let wr_file = tokio_fs::File::from_std(wr_file);

    // create a stream
    let read_stream = FramedRead::new(rd_file, LinesCodec::new()).retry(handle_fs_error);
    let write_sink = SinkRetry::new(
        FramedWrite::new(wr_file, LinesCodec::new()),
        handle_fs_error,
    );

    let forward_stream_to_sink = write_sink.send_all(read_stream);

    tokio::run(
        forward_stream_to_sink
            .map_err(|err| eprintln!("{}", err))
            .map(drop), // when future is resolved sink i stream is returned but we don't need it any more.
    )
}
