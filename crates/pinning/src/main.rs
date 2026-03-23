use std::{
    marker::PhantomPinned,
    pin::{Pin, pin},
    ptr::addr_of,
    task::{Context, Poll},
};

#[derive(Debug, Default)]
struct Foo {
    bar: u8,
}

#[derive(Debug, Default)]
struct FooNotUnpin {
    bar: u8,
    _marker: PhantomPinned,
}

#[derive(Debug, Default)]
struct SelfRefFuture {
    s: String,
    s_ptr: *const String,
    polled_once: bool,
    _marker: PhantomPinned,
}

impl Future for SelfRefFuture {
    type Output = String;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = unsafe { self.get_unchecked_mut() };
        if !this.polled_once {
            this.s_ptr = &this.s as *const String;
            this.polled_once = true;
            cx.waker().wake_by_ref();
            Poll::Pending
        } else {
            let s_ref: &String = unsafe { &*this.s_ptr };
            Poll::Ready(format!("s_ptr points to: {}", s_ref))
        }
    }
}

// what if Future::poll() takes in a &mut Self, instead of Pin<&mut Self>?
trait FakeFuture {
    type Output;

    fn poll_not_pin(&mut self, cx: &mut Context<'_>) -> Poll<Self::Output>;
}

impl FakeFuture for SelfRefFuture {
    type Output = String;

    fn poll_not_pin(&mut self, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if !self.polled_once {
            self.s_ptr = &self.s as *const String;
            self.polled_once = true;
            cx.waker().wake_by_ref();
            Poll::Pending
        } else {
            let s_ref: &String = unsafe { &*self.s_ptr };
            Poll::Ready(format!("s_ptr points to: {}", s_ref))
        }
    }
}

fn main() {
    println!("---");
    moves_are_memcpy();
    pinning_an_unpin_does_nothing();
    pinning_not_unpin_disallows_getting_mut();
    println!("---");
    violating_the_pinning_contract();
    println!("---");
    pinning_futures();
    println!("---");
    why_poll_requires_pin();
    println!("---");
    violating_contract_breaks_futures();
}

fn moves_are_memcpy() {
    let f1 = Foo { bar: 69 };
    println!("f1 addr: {:p}", addr_of!(f1));

    // in rust, a move is a bitwise copy + invalidation of the source, unlike in C++
    // the compiler is free to do a memcpy here instead of having f2 point to the old f1 memory
    let f2 = f1;
    println!("f2 addr: {:p} <- different from f1", addr_of!(f2));
}

fn pinning_an_unpin_does_nothing() {
    let f = Foo { bar: 69 };

    // the pin! macro pins a value on the stack and shadows it
    // we can only access it via the Pin pointer afterwards
    let pinned_foo: Pin<&mut Foo> = pin!(f);

    // because Foo: Unpin, we can safely get a &mut to foo and modify it
    let foo_ref: &mut Foo = pinned_foo.get_mut();
    foo_ref.bar = 67;
}

fn pinning_not_unpin_disallows_getting_mut() {
    let foo_not_unpin = FooNotUnpin {
        bar: 69,
        _marker: PhantomPinned,
    };

    let pinned_foo: Pin<&mut FooNotUnpin> = pin!(foo_not_unpin);
    // because FooNotUnpin: !Unpin, the compiler doesn't allow getting a &mut to foo_not_unpin
    // let foo_ref: &mut FooNotUnpin = pinned_foo.get_mut();

    // but getting a shared reference is fine
    let foo_ref: &FooNotUnpin = &pinned_foo;
    assert_eq!(foo_ref.bar, 69);

    // without &mut, we can't mem::swap, mem::replace, or move it
    // the value stays at this address until it's dropped
}

// it's possible to violate the pinning contract, but only with unsafe
fn violating_the_pinning_contract() {
    // We say that a value has been pinned when it has been put into a state
    // where it is guaranteed to remain located at the same place in memory
    // from the time it is pinned until its drop is called.
    // https://doc.rust-lang.org/std/pin/index.html#what-is-pinning
    //
    // therefore, after a Pin is created, it must not be possible to get
    // a &mut to the pointee even after the Pin is dropped

    let f_not_unpin = FooNotUnpin {
        bar: 69,
        _marker: PhantomPinned,
    };

    // before a Pin to it is created, we're free to move it around
    let mut foo_not_unpin = f_not_unpin;

    {
        let foo_ref_mut: &mut FooNotUnpin = &mut foo_not_unpin;
        // creating a pinned pointer to a struct that is !Unpin from a reference to it is unsafe,
        // because there's no guarantee that the data won't be moved afterwards
        //
        // Safety: foo_ref_mut must not be moved after this to uphold the pinning contract
        let _foo_pinned_ptr = unsafe { Pin::new_unchecked(foo_ref_mut) };

        // however the pin! macro is safe because it makes the
        // original value inaccessible, preventing any moves

        // foo_pinned_ptr holds a &mut to foo_not_unpin, so it's not possible
        // to (safely) mutate foo_not_unpin while foo_pinned_ptr is alive
    }

    // once foo_pinned_ptr gets dropped, it's possible to move foo_not_unpin in safe rust
    // but that would be a violation of the pinning contract
    println!(
        "violating_the_pinning_contract: before move: {:p}",
        addr_of!(foo_not_unpin)
    );

    let foo_not_unpin = foo_not_unpin;

    println!(
        "violating_the_pinning_contract: after  move: {:p}",
        addr_of!(foo_not_unpin)
    );
}

#[forbid(unsafe_code)]
fn pinning_futures() {
    let waker = std::task::Waker::noop();
    let mut cx = Context::from_waker(waker);

    // pin! shadows the value, no way to (safely) get &mut fut afterwards
    let mut fut = pin!(SelfRefFuture {
        s: String::from("hello"),
        s_ptr: std::ptr::null(),
        polled_once: false,
        _marker: PhantomPinned,
    });

    let p = fut.as_mut().poll(&mut cx);
    assert!(matches!(p, Poll::Pending));

    // can only access through Pin<&mut SelfRefFuture>
    // not possible to safely get a &mut SelfRefFuture
    // and no way to violate the contract

    if let Poll::Ready(result) = fut.as_mut().poll(&mut cx) {
        println!("pinning_futures(): {result}");
    }
}

// if poll() takes in a &mut Self instead of Pin<&mut Self>,
// it's trivial to break futures even without unsafe
#[forbid(unsafe_code)]
fn why_poll_requires_pin() {
    let waker = std::task::Waker::noop();
    let mut cx = Context::from_waker(waker);

    let mut fut = SelfRefFuture {
        s: String::from("hello"),
        s_ptr: std::ptr::null(),
        polled_once: false,
        _marker: PhantomPinned,
    };

    {
        let fut_mut_ref = &mut fut;
        let p = fut_mut_ref.poll_not_pin(&mut cx);
        assert!(matches!(p, Poll::Pending));
    }

    println!("why_poll_requires_pin(): before move: {:p}", addr_of!(fut));

    // this would be a violation of the pinning contract
    let mut fut2 = fut;

    println!("why_poll_requires_pin(): after  move: {:p}", addr_of!(fut2));
    println!("  fut.s     = {:p}", &fut2.s as *const String);
    println!("  fut.s_ptr = {:p} <- doesn't point to fut.s", fut2.s_ptr,);

    {
        let fut_mut_ref = &mut fut2;
        let p = fut_mut_ref.poll_not_pin(&mut cx);
        assert!(matches!(p, Poll::Ready(_)));
        if let Poll::Ready(result) = p {
            println!("result: {result}");
        }
    }
}

// futures with self references rely on the pinning contract being upheld
// and it's impossible to break the contract without unsafe code
fn violating_contract_breaks_futures() {
    let waker = std::task::Waker::noop();
    let mut cx = Context::from_waker(waker);

    let mut fut = SelfRefFuture {
        s: String::from("hello"),
        s_ptr: std::ptr::null(),
        polled_once: false,
        _marker: PhantomPinned,
    };

    let mut pinned = unsafe { Pin::new_unchecked(&mut fut) };
    let p = pinned.as_mut().poll(&mut cx);
    assert!(matches!(p, Poll::Pending));

    println!(
        "violating_contract_breaks_futures(): before move: {:p}",
        addr_of!(fut)
    );

    // this would be a violation of the pinning contract
    let mut fut2 = fut;

    println!(
        "violating_contract_breaks_futures(): after  move: {:p}",
        addr_of!(fut2)
    );
    println!("  fut.s     = {:p}", &fut2.s as *const String);
    println!("  fut.s_ptr = {:p} <- doesn't point to fut.s", fut2.s_ptr,);

    // this poll() dereferences s_ptr, which is dangling
    let mut pinned = unsafe { Pin::new_unchecked(&mut fut2) };
    if let Poll::Ready(result) = pinned.as_mut().poll(&mut cx) {
        println!("result: {result}");
    }
}

// executors/runtimes like tokio is responsible for upholding this pinning contract
// by only accessing futures through Pin<&mut F>, and never moving it after pinning
