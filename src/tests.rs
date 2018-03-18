use super::*;
use std::time::Duration;

struct StateMachine(State);
enum State {
    Yes,
    No,
}

impl FutureFactory for StateMachine {
    type FutureItem = futures::future::FutureResult<u8, u8>;

    fn new(&mut self) -> Self::FutureItem {
        match self.0 {
            State::Yes => {
                self.0 = State::No;
                futures::future::ok(1u8)
            }
            State::No => {
                self.0 = State::Yes;
                futures::future::err(2u8)
            }
        }
    }
}

#[test]
fn naive() {
    let f = retry(
        || futures::future::ok::<u8, u8>(1u8),
        |_| RetryPolicy::Repeat,
    );
    assert_eq!(Ok(1u8), f.wait());
}

#[test]
fn naive_error_forward() {
    let f = retry(
        || futures::future::err::<u8, u8>(1u8),
        |_| RetryPolicy::ForwardError,
    );
    assert_eq!(Err(1u8), f.wait());
}

#[test]
fn more_complicated_wait() {
    let f = retry(StateMachine(State::No), |_| {
        RetryPolicy::WaitRetry(Duration::from_millis(10))
    });
    assert_eq!(Ok(1u8), f.wait());
}

#[test]
fn more_complicated_repeate() {
    let f = retry(StateMachine(State::No), |_| RetryPolicy::Repeat);
    assert_eq!(Ok(1u8), f.wait());
}
