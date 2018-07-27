# futures-retry

[![pipeline status](https://gitlab.com/mexus/futures-retry/badges/master/pipeline.svg)](https://gitlab.com/mexus/futures-retry/commits/master)
[![crates.io](https://img.shields.io/crates/v/futures-retry.svg)](https://crates.io/crates/futures-retry)
[![docs.rs](https://docs.rs/futures-retry/badge.svg)](https://docs.rs/futures-retry)

[[Release docs]](https://docs.rs/futures-retry/)

[[Master docs]](https://mexus.gitlab.io/futures-retry/futures_retry/)

A tool that helps you retry your future :) Well, `Future`s and `Stream`s, to be precise.

It's quite a common task when you need to repeat some action if you've got an error, be it a
connection timeout or some temporary OS error.

When you do stuff in a synchronous manner it's quite easy to implement the attempts logic, but
when it comes to asynchronous programming you suddenly need to write a fool load of a
boilerplate code, introduce state machines and everything.

This library aims to make your life easier and let you write more straightword and nice code,
concentrating on buisness logic rathen than on handling all the mess.

I was inspired to write this library after coming over a [`hyper`
issue](https://github.com/hyperium/hyper/issues/1358), and I came to an understanding that the
problem is more common than I used to think.

For examples have a look in the `examples/` folder in the git repo.

Suggestions and critiques are welcome!

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

fn serve_connection(stream: TcpStream) -> impl Future<Item = (), Error = ()> + Send {
  # future::ok(())
}

fn main() {
  # let addr = "127.0.0.1:12345".parse().unwrap();
  let listener =TcpListener::bind(&addr).unwrap();
  let server = listener.incoming()
    .retry(handle_error) // Magic happens here
    .and_then(|stream| {
      tokio::spawn(serve_connection(stream));
      Ok(())
    })
    .for_each(|_| Ok(()))
    .map_err(|e| eprintln!("Caught an error {}", e));
  # // This nasty hack is required to exit immediately when running the doc tests.
  # let server = future::ok(()).select(server).map(|_| ()).map_err(|_| ());
  tokio::run(server);
}
```

### License

Licensed under either of

 * Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
 * MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

#### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any
additional terms or conditions.

License: MIT/Apache-2.0
