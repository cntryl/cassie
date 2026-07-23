use std::future::Future;
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

pub(crate) struct TimedWriteTransport<T> {
    inner: T,
    idle_timeout: Duration,
    write_deadline: Option<Pin<Box<tokio::time::Sleep>>>,
}

impl<T> TimedWriteTransport<T> {
    pub(crate) fn new(inner: T, idle_timeout: Duration) -> Self {
        Self {
            inner,
            idle_timeout: idle_timeout.max(Duration::from_millis(1)),
            write_deadline: None,
        }
    }

    fn pending_or_timed_out(&mut self, context: &mut Context<'_>) -> Poll<io::Result<()>> {
        let deadline = self
            .write_deadline
            .get_or_insert_with(|| Box::pin(tokio::time::sleep(self.idle_timeout)));
        if deadline.as_mut().poll(context).is_ready() {
            self.write_deadline = None;
            Poll::Ready(Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "transport write idle timeout",
            )))
        } else {
            Poll::Pending
        }
    }

    fn completed(&mut self) {
        self.write_deadline = None;
    }
}

impl<T: AsyncRead + Unpin> AsyncRead for TimedWriteTransport<T> {
    fn poll_read(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_read(context, buffer)
    }
}

impl<T: AsyncWrite + Unpin> AsyncWrite for TimedWriteTransport<T> {
    fn poll_write(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        match Pin::new(&mut this.inner).poll_write(context, buffer) {
            Poll::Ready(result) => {
                this.completed();
                Poll::Ready(result)
            }
            Poll::Pending => match this.pending_or_timed_out(context) {
                Poll::Ready(Err(error)) => Poll::Ready(Err(error)),
                _ => Poll::Pending,
            },
        }
    }

    fn poll_flush(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        match Pin::new(&mut this.inner).poll_flush(context) {
            Poll::Ready(result) => {
                this.completed();
                Poll::Ready(result)
            }
            Poll::Pending => this.pending_or_timed_out(context),
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        match Pin::new(&mut this.inner).poll_shutdown(context) {
            Poll::Ready(result) => {
                this.completed();
                Poll::Ready(result)
            }
            Poll::Pending => this.pending_or_timed_out(context),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncWriteExt;

    #[test]
    fn should_fail_a_stalled_writer_after_its_idle_deadline() {
        // Arrange
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        runtime.block_on(async {
            let (stream, _reader) = tokio::io::duplex(1);
            let mut writer = TimedWriteTransport::new(stream, Duration::from_millis(10));

            // Act
            let error = writer
                .write_all(&[1, 2])
                .await
                .expect_err("stalled write must time out");

            // Assert
            assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        });
    }
}
