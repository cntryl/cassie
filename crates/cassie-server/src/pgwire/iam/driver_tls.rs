use std::future::Future;
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};

use rustls::pki_types::ServerName;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio_postgres::tls::{ChannelBinding, MakeTlsConnect, TlsConnect, TlsStream};

pub(super) struct DriverTls(pub(super) tokio_rustls::TlsConnector);

pub(super) struct Connector {
    connector: tokio_rustls::TlsConnector,
    name: ServerName<'static>,
}

impl<S: AsyncRead + AsyncWrite + Unpin + Send + 'static> MakeTlsConnect<S> for DriverTls {
    type Stream = Stream<S>;
    type TlsConnect = Connector;
    type Error = rustls::pki_types::InvalidDnsNameError;

    fn make_tls_connect(&mut self, domain: &str) -> Result<Self::TlsConnect, Self::Error> {
        Ok(Connector {
            connector: self.0.clone(),
            name: ServerName::try_from(domain.to_owned())?,
        })
    }
}

impl<S: AsyncRead + AsyncWrite + Unpin + Send + 'static> TlsConnect<S> for Connector {
    type Stream = Stream<S>;
    type Error = io::Error;
    type Future = Pin<Box<dyn Future<Output = io::Result<Self::Stream>> + Send>>;

    fn connect(self, stream: S) -> Self::Future {
        Box::pin(async move { self.connector.connect(self.name, stream).await.map(Stream) })
    }
}

pub(super) struct Stream<S>(tokio_rustls::client::TlsStream<S>);

impl<S: AsyncRead + AsyncWrite + Unpin> AsyncRead for Stream<S> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().0).poll_read(cx, buf)
    }
}

impl<S: AsyncRead + AsyncWrite + Unpin> AsyncWrite for Stream<S> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.get_mut().0).poll_write(cx, bytes)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().0).poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().0).poll_shutdown(cx)
    }
}

impl<S: AsyncRead + AsyncWrite + Unpin> TlsStream for Stream<S> {
    fn channel_binding(&self) -> ChannelBinding {
        ChannelBinding::none()
    }
}
