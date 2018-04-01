# futures-retry

[![pipeline status](https://gitlab.com/mexus/futures-retry/badges/master/pipeline.svg)](https://gitlab.com/mexus/futures-retry/commits/master)
[![crates.io](https://img.shields.io/crates/v/futures-retry.svg)](https://crates.io/crates/futures-retry)
[![docs.rs](https://docs.rs/futures-retry/badge.svg)](https://docs.rs/futures-retry)

[[Release docs]](https://docs.rs/futures-retry/)

[[Master docs]](https://mexus.gitlab.io/futures-retry/futures_retry/)

A simple crate that helps you to retry your `Future`s and `Stream`s in a neat
and simple way.

```rust
extern crate futures_retry;
// ...
use futures_retry::{RetryPolicy, StreamRetryExt};

fn handle_error(e: io::Error) -> RetryPolicy<io::Error> {
  match e.kind() {
    io::ErrorKind::Interrupted => RetryPolicy::Repeat,
    io::ErrorKind::PermissionDenied => RetryPolicy::ForwardError(e),
    _ => RetryPolicy::WaitRetry(Duration::from_millis(5)),
  }
}

// Use `Box<...>` instead of `impl ...` if your rust version doesn't support `impl Trait`.
fn serve_connection(stream: TcpStream) -> impl Future<Item = (), Error = ()> + Send {
  // ...
}

fn main() {
  let listener: TcpListener = // ...
  let server = listener.incoming()
    .into_retry(handle_error) // Magic happens here
    .and_then(|stream| {
      tokio::spawn(serve_connection(stream));
      Ok(())
    })
    .for_each(|_| Ok(()))
    .map_err(|e| eprintln!("Caught an error {}", e));
  tokio::run(server);
}
```

## License

Licensed under either of

 * Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
 * MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any
additional terms or conditions.
