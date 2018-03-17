use super::*;
use std::time::Duration;

#[test]
fn naive() {
    let f = retry(|| futures::future::ok::<u8, u8>(1u8), None);
    assert_eq!(Ok(1u8), f.wait());
}

#[test]
fn more_complicated() {
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
    let f = retry(StateMachine(State::No), Some(Duration::from_millis(10)));
    assert_eq!(Ok(1u8), f.wait());
}
