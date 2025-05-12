use bytes::Bytes;
use http_body_util::Full;
use hyper_util::rt::TokioIo;
use rand::{Rng, SeedableRng};
use tokio::net::TcpStream;
use tokio_native_tls::{native_tls, TlsStream};
use url::Url;
use std::net::{IpAddr, SocketAddr};
use hyper::{http, HeaderMap};
use compact_str::CompactString;
use hyper::client::conn::http1;
use crate::pcg64si::Pcg64Si;
use crate::work_mode::{ClientResponseCodeType, RequestCounter, WorkMode};

#[derive(Debug)]
pub enum ClientResult {
    Code(ClientResponseCodeType),
    Timeout,
    TlsError(native_tls::Error),
    HyperError(hyper::Error),
}


enum Stream{
    Tcp(TcpStream),
    Tls(TlsStream<TcpStream>)
}

impl Stream {
    async fn handshake_http1(self, with_upgrade: bool) -> Result<http1::SendRequest<Full<Bytes>>, hyper::Error> {
        match self {
            Stream::Tcp(stream) => {
                let (send_request, conn) =
                    http1::handshake(TokioIo::new(stream)).await?;
                if with_upgrade {
                    tokio::spawn(conn.with_upgrades());
                } else {
                    tokio::spawn(conn);
                }
                Ok(send_request)
            }
            Stream::Tls(stream) => {
                let (send_request, conn) =
                    http1::handshake(TokioIo::new(stream)).await?;
                if with_upgrade {
                    tokio::spawn(conn.with_upgrades());
                } else {
                    tokio::spawn(conn);
                }
                Ok(send_request)
            }
        }
    }
}

#[derive(Debug)]
pub struct WorkInstance {
    pub url: Url,
    pub address: SocketAddr,
    pub mode: WorkMode,
    pub header_map: HeaderMap,
    pub accept_headers: Vec<CompactString>,
    pub proxy_headers: Vec<CompactString>,
    pub max_time: Option<humantime::Duration>,
    pub request_counter: RequestCounter,
}

impl WorkInstance {
    pub async fn connect(
        &self,
    ) -> Result<Stream, std::io::Error> {
        let stream = TcpStream::connect(&self.address).await?;
        Ok(Stream::Tcp(stream))
    }
    
    pub async fn tls(
        &self,
        stream: TcpStream,
    ) -> Result<TlsStream<TcpStream>, native_tls::Error> {
        let connector = tokio_native_tls::TlsConnector::from(
            native_tls::TlsConnector::new()?,
        );
        let Some(domain) = self.url.host_str() else {
            unreachable!("If the URL has no host, it's not a valid URL. And the check must have failed before.");
        };
        let stream = connector.connect(domain, stream).await?;
        Ok(stream)
    }
    
    pub async fn build_request(
        &self,
    ) -> Result<http::Request<Full<Bytes>>, http::Error> {
        let mut builder = http::Request::builder()
            .uri(self.url.as_str())
            .method(self.mode.method())
            .version(http::Version::HTTP_11);
        
        for header in &self.header_map {
            builder = builder.header(header.0, header.1);
        }
        
        match &self.mode {
            WorkMode::Get => {
                builder.body(Full::new(Bytes::new()))
                
            },
            WorkMode::Post(spec) => {
                builder = builder.header(
                    "Content-Length",
                    spec.body.len().to_string(),
                );
                if let Some(content_type) = &spec.content_type {
                    builder = builder.header("Content-Type", content_type.as_str());
                }
                builder.body(Full::new(spec.body.clone()))
            }
        }
    }
    
    
}