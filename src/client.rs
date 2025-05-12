use crate::work_mode::{ClientResponseCodeType, RequestCounter, WorkMode};
use bytes::Bytes;
use compact_str::CompactString;
use http_body_util::{BodyExt, Full};
use hyper::client::conn::http1;
use hyper::{HeaderMap, StatusCode, http};
use hyper_util::rt::TokioIo;
use std::net::SocketAddr;
use tokio::net::TcpStream;
use tokio_native_tls::{TlsStream, native_tls};
use url::Url;

#[derive(Debug)]
pub enum ClientResult {
    Code(ClientResponseCodeType),
    Timeout,
    TlsError(native_tls::Error),
    HyperError(hyper::Error),
    HttpError(http::Error),
    IoError(std::io::Error),
    ConnectionClosed,
}

pub enum Stream {
    Tcp(TcpStream),
    Tls(TlsStream<TcpStream>),
}

impl Stream {
    pub fn is_tls(&self) -> bool {
        matches!(self, Stream::Tls(_))
    }
    async fn handshake_http1(
        self,
        with_upgrade: bool,
    ) -> Result<http1::SendRequest<Full<Bytes>>, hyper::Error> {
        match self {
            Stream::Tcp(stream) => {
                let (send_request, conn) = http1::handshake(TokioIo::new(stream)).await?;
                if with_upgrade {
                    tokio::spawn(conn.with_upgrades());
                } else {
                    tokio::spawn(conn);
                }
                Ok(send_request)
            }
            Stream::Tls(stream) => {
                let (send_request, conn) = http1::handshake(TokioIo::new(stream)).await?;
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
    pub max_time: Option<chrono::Duration>,
    pub request_counter: RequestCounter,
}

impl WorkInstance {
    pub async fn connect(&self) -> Result<Stream, std::io::Error> {
        let stream = TcpStream::connect(&self.address).await?;
        Ok(Stream::Tcp(stream))
    }

    pub async fn tls(&self, stream: TcpStream) -> Result<TlsStream<TcpStream>, native_tls::Error> {
        let connector = tokio_native_tls::TlsConnector::from(native_tls::TlsConnector::new()?);
        let Some(domain) = self.url.host_str() else {
            unreachable!(
                "If the URL has no host, it's not a valid URL. And the check must have failed before."
            );
        };
        let stream = connector.connect(domain, stream).await?;
        Ok(stream)
    }

    pub async fn build_request(&self) -> Result<http::Request<Full<Bytes>>, http::Error> {
        let mut builder = http::Request::builder()
            .uri(self.url.as_str())
            .method(self.mode.method())
            .version(http::Version::HTTP_11);

        for header in &self.header_map {
            builder = builder.header(header.0, header.1);
        }

        match &self.mode {
            WorkMode::Get => builder.body(Full::new(Bytes::new())),
            WorkMode::Post(spec) => {
                builder = builder.header("Content-Length", spec.body.len().to_string());
                if let Some(content_type) = &spec.content_type {
                    builder = builder.header("Content-Type", content_type.as_str());
                }
                builder.body(Full::new(spec.body.clone()))
            }
        }
    }

    /// Determines the ClientResponseCodeType from an HTTP status code
    fn status_to_code_type(status: StatusCode) -> ClientResponseCodeType {
        match status.as_u16() {
            200..=299 => ClientResponseCodeType::Code2,
            300..=399 => ClientResponseCodeType::Code3,
            400..=499 => ClientResponseCodeType::Code4,
            500..=599 => ClientResponseCodeType::Code5,
            _ => ClientResponseCodeType::Failure,
        }
    }

    /// Sends a single request using an existing connection if provided, or creates a new one.
    /// Returns the result of the request and a boolean indicating if a TLS handshake was performed.
    pub async fn send_single_request(
        &self,
        existing_send_request: Option<http1::SendRequest<Full<Bytes>>>,
    ) -> (ClientResult, bool) {
        // Build the request
        let request = match self.build_request().await {
            Ok(req) => req,
            Err(e) => return (ClientResult::HttpError(e), false),
        };

        // If we have an existing connection, try to use it
        if let Some(mut send_request) = existing_send_request {
            match send_request.send_request(request).await {
                Ok(response) => {
                    let status = response.status();
                    let code_type = Self::status_to_code_type(status);
                    self.request_counter.inc(code_type);
                    return (ClientResult::Code(code_type), false);
                }
                Err(e) => {
                    // Connection might be closed, need to create a new one
                    self.request_counter.inc(ClientResponseCodeType::Failure);
                    return (ClientResult::HyperError(e), false);
                }
            }
        }

        // No existing connection or it failed, create a new one
        let stream = match self.connect().await {
            Ok(stream) => stream,
            Err(e) => {
                self.request_counter.inc(ClientResponseCodeType::Failure);
                return (ClientResult::IoError(e), false);
            }
        };

        // Check if we need TLS
        let (stream, did_tls_handshake) = if self.url.scheme() == "https" {
            match stream {
                Stream::Tcp(tcp_stream) => match self.tls(tcp_stream).await {
                    Ok(tls_stream) => (Stream::Tls(tls_stream), true),
                    Err(e) => {
                        self.request_counter.inc(ClientResponseCodeType::Failure);
                        return (ClientResult::TlsError(e), false);
                    }
                },
                Stream::Tls(tls_stream) => (Stream::Tls(tls_stream), false),
            }
        } else {
            (stream, false)
        };

        // Perform HTTP handshake
        let mut send_request = match stream.handshake_http1(false).await {
            Ok(send_req) => send_req,
            Err(e) => {
                self.request_counter.inc(ClientResponseCodeType::Failure);
                return (ClientResult::HyperError(e), did_tls_handshake);
            }
        };

        // Send the request
        match send_request.send_request(request).await {
            Ok(response) => {
                let status = response.status();
                let code_type = Self::status_to_code_type(status);
                self.request_counter.inc(code_type);
                (ClientResult::Code(code_type), did_tls_handshake)
            }
            Err(e) => {
                self.request_counter.inc(ClientResponseCodeType::Failure);
                (ClientResult::HyperError(e), did_tls_handshake)
            }
        }
    }

    /// Sends a single request and returns a new SendRequest object that can be reused
    /// for subsequent requests, along with the result and whether a TLS handshake was performed.
    pub async fn send_request_with_reuse(
        &self,
        existing_send_request: Option<http1::SendRequest<Full<Bytes>>>,
    ) -> (ClientResult, bool, Option<http1::SendRequest<Full<Bytes>>>) {
        // Build the request
        let request = match self.build_request().await {
            Ok(req) => req,
            Err(e) => return (ClientResult::HttpError(e), false, None),
        };

        // If we have an existing connection, try to use it
        if let Some(mut send_request) = existing_send_request {
            match send_request.send_request(request).await {
                Ok(response) => {
                    let status = response.status();
                    let code_type = Self::status_to_code_type(status);
                    self.request_counter.inc(code_type);
                    // Consume the response body to free up the connection for reuse
                    let _ = response.collect().await;
                    return (ClientResult::Code(code_type), false, Some(send_request));
                }
                Err(_) => {
                    // Connection might be closed, need to create a new one
                    self.request_counter.inc(ClientResponseCodeType::Failure);
                    // Don't return the failed send_request
                    return (ClientResult::ConnectionClosed, false, None);
                }
            }
        }

        // No existing connection or it failed, create a new one
        let stream = match self.connect().await {
            Ok(stream) => stream,
            Err(e) => {
                self.request_counter.inc(ClientResponseCodeType::Failure);
                return (ClientResult::IoError(e), false, None);
            }
        };

        // Check if we need TLS
        let (stream, did_tls_handshake) = if self.url.scheme() == "https" {
            match stream {
                Stream::Tcp(tcp_stream) => match self.tls(tcp_stream).await {
                    Ok(tls_stream) => (Stream::Tls(tls_stream), true),
                    Err(e) => {
                        self.request_counter.inc(ClientResponseCodeType::Failure);
                        return (ClientResult::TlsError(e), false, None);
                    }
                },
                Stream::Tls(tls_stream) => (Stream::Tls(tls_stream), false),
            }
        } else {
            (stream, false)
        };

        // Perform HTTP handshake
        let mut send_request = match stream.handshake_http1(false).await {
            Ok(send_req) => send_req,
            Err(e) => {
                self.request_counter.inc(ClientResponseCodeType::Failure);
                return (ClientResult::HyperError(e), did_tls_handshake, None);
            }
        };

        // Send the request
        match send_request.send_request(request).await {
            Ok(response) => {
                let status = response.status();
                let code_type = Self::status_to_code_type(status);
                self.request_counter.inc(code_type);
                // Consume the response body to free up the connection for reuse
                let _ = response.collect().await;
                (
                    ClientResult::Code(code_type),
                    did_tls_handshake,
                    Some(send_request),
                )
            }
            Err(e) => {
                self.request_counter.inc(ClientResponseCodeType::Failure);
                (ClientResult::HyperError(e), did_tls_handshake, None)
            }
        }
    }
}
