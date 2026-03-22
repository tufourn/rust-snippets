use std::{
    pin::Pin,
    task::{Context, Poll},
    time::{Duration, Instant},
};

const TIMEOUT_SEC: u64 = 1;

#[tokio::main]
async fn main() {
    println!("---");
    {
        println!("foo() returned {}", foo().await);
        println!("foo_desugared() returned {}", foo_desugared().await);
    }
    println!("---");
    {
        let start = Instant::now();
        let result = bar().await;
        let elapsed = start.elapsed();
        println!("bar() returned {} after {:.3?}", result, elapsed);
    }
    {
        let start = Instant::now();
        let result = bar_desugared().await;
        let elapsed = start.elapsed();
        println!("bar_desugared() returned {} after {:.3?}", result, elapsed);
    }
    println!("---");
    {
        let start = Instant::now();
        let result = baz().await;
        let elapsed = start.elapsed();
        println!("baz() returned {} after {:.3?}", result, elapsed);
    }
    {
        let start = Instant::now();
        let result = baz_desugared().await;
        let elapsed = start.elapsed();
        println!("baz_desugared() returned {} after {:.3?}", result, elapsed);
    }
    println!("---");
}

// --- trivial future that's instantly ready
async fn foo() -> i32 {
    69
}

fn foo_desugared() -> impl Future<Output = i32> {
    FooFuture
}

struct FooFuture;

impl Future for FooFuture {
    type Output = i32;

    fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
        Poll::Ready(69)
    }
}

// --- simple future with an await/yield point
async fn bar() -> i32 {
    tokio::time::sleep(Duration::from_secs(TIMEOUT_SEC)).await;
    69
}

fn bar_desugared() -> impl Future<Output = i32> {
    BarFuture {
        state: BarState::Start,
    }
}

struct BarFuture {
    state: BarState,
}

enum BarState {
    Start,
    Sleeping { sleep: tokio::time::Sleep },
    Done,
}

impl Future for BarFuture {
    type Output = i32;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // Safety: not moving data out of this &mut
        let this: &mut BarFuture = unsafe { self.as_mut().get_unchecked_mut() };
        loop {
            match this.state {
                BarState::Start => {
                    let sleep = tokio::time::sleep(Duration::from_secs(TIMEOUT_SEC));
                    this.state = BarState::Sleeping { sleep };
                }
                BarState::Sleeping { ref mut sleep } => {
                    // Safety: sleep is not moved
                    let inner = unsafe { Pin::new_unchecked(sleep) };
                    match inner.poll(cx) {
                        Poll::Pending => return Poll::Pending,
                        Poll::Ready(_) => {
                            this.state = BarState::Done;
                            return Poll::Ready(69);
                        }
                    }
                }
                BarState::Done => panic!("futures should not be polled after completion"),
            }
        }
    }
}

// future with self references
async fn baz() -> i32 {
    let s = String::from("420");
    let s_ref: &String = &s; // borrow created before .await
    tokio::time::sleep(Duration::from_secs(TIMEOUT_SEC)).await;
    let s_i32: i32 = s_ref.parse().unwrap(); // borrow used after .await
    s_i32 * 100 + 69
}

fn baz_desugared() -> impl Future<Output = i32> {
    BazFuture {
        state: BazState::Start,
    }
}

struct BazFuture {
    state: BazState,
}

enum BazState {
    Start,
    Waiting {
        #[allow(dead_code)]
        s: String,
        // Sleep is !Unpin, so BazState and BazFuture is also !Unpin
        // s_ptr is created after s is placed in the enum variant,
        // and since BazFuture is pinned (and !Unpin), s won't move again,
        // keeping s_ptr valid
        s_ptr: *const String,
        sleep: tokio::time::Sleep,
    },
    Done,
}

impl Future for BazFuture {
    type Output = i32;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // Safety: not moving data out of this &mut
        let this = unsafe { self.as_mut().get_unchecked_mut() };

        // we'd break the pinning contract if we do something like this
        //
        // let other = BazFuture {
        //     state: BazState::Start,
        // };
        // let _ = std::mem::replace(this, other);

        loop {
            match this.state {
                BazState::Start => {
                    let s = String::from("420");
                    let sleep = tokio::time::sleep(Duration::from_secs(TIMEOUT_SEC));

                    // we'd get UB if we do this, because s may be moved during the
                    // construction of state, and s_ptr would then point to the wrong location
                    //
                    // let s_ptr = &s as *const String;
                    // this.state = BazState::Waiting { s, s_ptr, sleep };

                    // must assign s_ptr to point to s after the state is constructed
                    // to ensure s_ptr points to the final location of s
                    this.state = BazState::Waiting {
                        s,
                        s_ptr: std::ptr::null(), // dummy, to be reassigned to point to s
                        sleep,
                    };
                    if let BazState::Waiting { s, s_ptr, .. } = &mut this.state {
                        *s_ptr = s as *const String;
                    } else {
                        unreachable!("state must be BazState::Waiting")
                    }
                }
                BazState::Waiting {
                    s_ptr,
                    ref mut sleep,
                    ..
                } => {
                    // Safety: sleep is not moved
                    let sleep = unsafe { Pin::new_unchecked(sleep) };
                    match sleep.poll(cx) {
                        Poll::Ready(_) => {
                            // Safety: s_ptr is a pointer to the string s
                            let s_ref: &String = unsafe { &*s_ptr };
                            let s_i32: i32 = s_ref.parse().unwrap();
                            this.state = BazState::Done;
                            return Poll::Ready(s_i32 * 100 + 69);
                        }
                        Poll::Pending => return Poll::Pending,
                    }
                }
                BazState::Done => panic!("futures should not be polled after completion"),
            }
        }
    }
}
