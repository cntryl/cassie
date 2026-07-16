use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use rustls::ServerConfig;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadBuf};
use tokio::net::TcpStream;
use tokio_rustls::server::TlsStream;
use tokio_rustls::TlsAcceptor;

use super::{MAX_FRONTEND_MESSAGE_BYTES, SSL_REQUEST_CODE};

pub(super) enum PgwireTransport {
    Plain(TcpStream),
    Tls(Box<TlsStream<TcpStream>>),
}

impl PgwireTransport {
    pub(super) async fn negotiate(
        mut socket: TcpStream,
        tls_config: Option<Arc<ServerConfig>>,
    ) -> io::Result<Self> {
        let mut prefix = [0_u8; 8];
        loop {
            let count = socket.peek(&mut prefix).await?;
            if count == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "connection closed before pgwire startup",
                ));
            }
            if count >= 4 {
                let length = i32::from_be_bytes(prefix[..4].try_into().expect("startup length"));
                if length < 8
                    || usize::try_from(length)
                        .map_or(true, |length| length > MAX_FRONTEND_MESSAGE_BYTES)
                {
                    return Ok(Self::Plain(socket));
                }
                if count >= prefix.len() {
                    break;
                }
            }
            tokio::task::yield_now().await;
        }
        if i32::from_be_bytes(prefix[..4].try_into().expect("startup length")) != 8
            || i32::from_be_bytes(prefix[4..].try_into().expect("startup code")) != SSL_REQUEST_CODE
        {
            return Ok(Self::Plain(socket));
        }

        socket.read_exact(&mut prefix).await?;
        let Some(config) = tls_config else {
            socket.write_all(b"N").await?;
            socket.flush().await?;
            return Ok(Self::Plain(socket));
        };
        socket.write_all(b"S").await?;
        socket.flush().await?;
        TlsAcceptor::from(config)
            .accept(socket)
            .await
            .map(Box::new)
            .map(Self::Tls)
    }

    pub(super) const fn is_tls(&self) -> bool {
        matches!(self, Self::Tls(_))
    }
}

impl AsyncRead for PgwireTransport {
    fn poll_read(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Self::Plain(stream) => Pin::new(stream).poll_read(context, buffer),
            Self::Tls(stream) => Pin::new(stream.as_mut()).poll_read(context, buffer),
        }
    }
}

impl AsyncWrite for PgwireTransport {
    fn poll_write(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        match self.get_mut() {
            Self::Plain(stream) => Pin::new(stream).poll_write(context, buffer),
            Self::Tls(stream) => Pin::new(stream.as_mut()).poll_write(context, buffer),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        match self.get_mut() {
            Self::Plain(stream) => Pin::new(stream).poll_flush(context),
            Self::Tls(stream) => Pin::new(stream.as_mut()).poll_flush(context),
        }
    }

    fn poll_shutdown(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<Result<(), io::Error>> {
        match self.get_mut() {
            Self::Plain(stream) => Pin::new(stream).poll_shutdown(context),
            Self::Tls(stream) => Pin::new(stream.as_mut()).poll_shutdown(context),
        }
    }
}
