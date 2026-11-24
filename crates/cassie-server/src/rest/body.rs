use std::convert::Infallible;
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};

use bytes::Bytes;
use http_body_util::combinators::UnsyncBoxBody;
use http_body_util::{BodyExt, Full};
use hyper::body::{Body, Frame, SizeHint};
use tokio::io::{AsyncRead, ReadBuf};

pub(crate) const STATIC_FILE_CHUNK_BYTES: usize = 64 * 1024;
pub(crate) type RestBody = UnsyncBoxBody<Bytes, io::Error>;

pub(crate) fn full_body(body: impl Into<Bytes>) -> RestBody {
    Full::new(body.into())
        .map_err(infallible_to_io)
        .boxed_unsync()
}

pub(crate) struct StaticFileBody {
    file: tokio::fs::File,
    remaining: u64,
}

impl StaticFileBody {
    pub(crate) fn new(file: tokio::fs::File, length: u64) -> Self {
        Self {
            file,
            remaining: length,
        }
    }
}

impl Body for StaticFileBody {
    type Data = Bytes;
    type Error = io::Error;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        if self.remaining == 0 {
            return Poll::Ready(None);
        }
        let chunk_len = usize::try_from(self.remaining)
            .unwrap_or(usize::MAX)
            .min(STATIC_FILE_CHUNK_BYTES);
        let mut chunk = vec![0_u8; chunk_len];
        let mut read_buffer = ReadBuf::new(&mut chunk);
        match Pin::new(&mut self.file).poll_read(context, &mut read_buffer) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Err(error)) => Poll::Ready(Some(Err(error))),
            Poll::Ready(Ok(())) => {
                let read = read_buffer.filled().len();
                if read == 0 {
                    self.remaining = 0;
                    return Poll::Ready(Some(Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "static file changed while it was being streamed",
                    ))));
                }
                self.remaining = self.remaining.saturating_sub(read as u64);
                chunk.truncate(read);
                Poll::Ready(Some(Ok(Frame::data(Bytes::from(chunk)))))
            }
        }
    }

    fn is_end_stream(&self) -> bool {
        self.remaining == 0
    }

    fn size_hint(&self) -> SizeHint {
        SizeHint::with_exact(self.remaining)
    }
}

fn infallible_to_io(error: Infallible) -> io::Error {
    match error {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_stream_static_files_in_chunks_no_larger_than_sixty_four_kibibytes() {
        // Arrange
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        runtime.block_on(async {
            let path =
                std::env::temp_dir().join(format!("cassie-static-stream-{}", uuid::Uuid::new_v4()));
            let expected = vec![7_u8; STATIC_FILE_CHUNK_BYTES * 2 + 17];
            std::fs::write(&path, &expected).expect("fixture");
            let file = tokio::fs::File::open(&path).await.expect("open fixture");
            let mut body = StaticFileBody::new(file, expected.len() as u64);
            let mut chunks = Vec::new();

            // Act
            while let Some(frame) = BodyExt::frame(&mut body).await {
                let data = frame.expect("file frame").into_data().expect("data frame");
                chunks.push(data);
            }

            // Assert
            assert!(chunks
                .iter()
                .all(|chunk| chunk.len() <= STATIC_FILE_CHUNK_BYTES));
            assert_eq!(chunks.iter().map(Bytes::len).sum::<usize>(), expected.len());
            let _ = std::fs::remove_file(path);
        });
    }
}
