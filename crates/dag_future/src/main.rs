use std::{
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};

use pin_project::pin_project;
use tokio::{sync::oneshot, time::sleep};

#[pin_project]
pub struct DagFuture<F> {
    #[pin]
    inner: F,
    prev: Vec<oneshot::Receiver<()>>,
    next: Vec<oneshot::Sender<()>>,
}

impl<F> DagFuture<F> {
    pub fn new(inner: F) -> Self {
        Self {
            inner,
            prev: Vec::new(),
            next: Vec::new(),
        }
    }

    pub fn before<O>(&mut self, other: &mut DagFuture<O>) {
        let (tx, rx) = oneshot::channel();
        self.next.push(tx);
        other.prev.push(rx);
    }
}

impl<F> Future for DagFuture<F>
where
    F: Future,
{
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();

        while let Some(mut r) = this.prev.pop() {
            let p = Pin::new(&mut r);
            match p.poll(cx) {
                Poll::Ready(_) => {}
                Poll::Pending => {
                    this.prev.push(r);
                    return Poll::Pending;
                }
            }
        }

        match this.inner.poll(cx) {
            Poll::Ready(result) => {
                while let Some(n) = this.next.pop() {
                    let _ = n.send(());
                }
                Poll::Ready(result)
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

#[tokio::main]
async fn main() {
    let mut f1 = DagFuture::new(async {
        println!("f1 started");
        sleep(Duration::from_millis(300)).await;
        println!("f1 done");
        1
    });

    let mut f2 = DagFuture::new(async {
        println!("f2 started");
        sleep(Duration::from_millis(200)).await;
        println!("f2 done");
        2
    });

    let mut f3 = DagFuture::new(async {
        println!("f3 started");
        sleep(Duration::from_millis(100)).await;
        println!("f3 done");
        3
    });

    f1.before(&mut f2);
    f2.before(&mut f3);

    let (_, _, _) = tokio::join!(f1, f2, f3);
}
