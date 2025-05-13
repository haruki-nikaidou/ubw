use crate::work_mode::{ClientResponseCodeType, RequestCounter, WorkMode};
use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::client::conn::http1;
use hyper::{HeaderMap, StatusCode, http};
use hyper_util::rt::TokioIo;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use crossbeam::queue::ArrayQueue;
use tokio::net::TcpStream;
use tokio_native_tls::{TlsStream, native_tls};
use url::Url;

type Http1Conn = http1::SendRequest<Full<Bytes>>;

pub enum Stream {
    Tcp(TcpStream),
    Tls(TlsStream<TcpStream>),
}

impl Stream {
    pub fn is_tls(&self) -> bool {
        matches!(self, Stream::Tls(_))
    }
    async fn handshake_http1(self, with_upgrade: bool) -> Result<Http1Conn, hyper::Error> {
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
    pub request_counter: RequestCounter,
    pub connection_pool: Http1ConnectionPool,
}

#[derive(Debug)]
pub struct Http1ConnectionPool {
    queue: Arc<ArrayQueue<Http1Conn>>,
}

impl Http1ConnectionPool {
    pub fn new(capacity: usize) -> Self {
        Self {
            queue: Arc::new(ArrayQueue::new(capacity)),
        }
    }

    pub fn try_get(&self) -> Option<Http1Conn> {
        self.queue.pop()
    }

    pub fn put(&self, conn: Http1Conn) {
        let _ = self.queue.push(conn); // drop if full
    }

    pub async fn get_or_connect(&self, work_instance: &WorkInstance) -> anyhow::Result<Http1Conn> {
        if let Some(conn) = self.try_get() {
            Ok(conn)
        } else {
            work_instance.connect().await
        }
    }
}


impl WorkInstance {
    /// Connect to the socket, if TLS is needed, perform a TLS handshake.
    async fn connect_socket(&self) -> anyhow::Result<Stream> {
        let stream = TcpStream::connect(&self.address).await?;
        if self.url.scheme() == "https" {
            return Ok(self.tls(stream).await.map(Stream::Tls)?);
        }
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
        // Get path and query from URL
        let path = self.url.path();
        let path_and_query = if let Some(query) = self.url.query() {
            format!("{}?{}", path, query)
        } else {
            path.to_string()
        };

        let mut builder = http::Request::builder()
            .uri(path_and_query)
            .method(self.mode.method())
            .version(http::Version::HTTP_11);

        // Add Host header if not already present
        if let Some(host) = self.url.host_str() {
            let host_value = if let Some(port) = self.url.port() {
                format!("{}:{}", host, port)
            } else {
                host.to_string()
            };
            builder = builder.header("Host", host_value);
        }

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

    /// Initializes the worker state by connecting to the server and performing a TLS handshake if needed.
    pub async fn connect(&self) -> anyhow::Result<Http1Conn> {
        let stream = self.connect_socket().await?;
        let send_request = stream.handshake_http1(false).await?;
        Ok(send_request)
    }

    pub async fn send(
        &self,
        request: http::Request<Full<Bytes>>,
    ) {
        const MAX_RETRIES: usize = 10;
        let mut retries = 0;

        loop {
            let Ok(mut conn) = self.connection_pool.get_or_connect(self).await else {
                self.request_counter.inc(ClientResponseCodeType::Failure);
                return 
            };
            
            match conn.send_request(request.clone()).await {
                Ok(response) => {
                    let status = response.status();
                    let code_type = WorkInstance::status_to_code_type(status);
                    self.request_counter.inc(code_type);
                    // Consume the response body to free up the connection for reuse
                    let _ = response.collect().await;
                    return 
                }
                Err(_) => {
                    retries += 1;
                    if retries >= MAX_RETRIES {
                        self.request_counter.inc(ClientResponseCodeType::Failure);
                        return 
                    }
                    tokio::time::sleep(Duration::from_millis(2u64.pow(retries as u32))).await;
                }
            }
        }
    }
}

pub async fn request_loop(
    work_instance: Arc<WorkInstance>,
    shutdown_signal: &mut tokio::sync::watch::Receiver<bool>,
) -> anyhow::Result<()> {
    let request = work_instance.build_request().await?;
    loop {
        tokio::select! {
            _ = shutdown_signal.changed() => {
                break Ok(());
            }
            _ = work_instance.send(request.clone()) => {}
        }
    }
}
