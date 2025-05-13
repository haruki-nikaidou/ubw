use crate::UbwError;
use crate::client::WorkInstance;
use crate::opts::{Opts, WrappedHeaderMap};
use crate::work_mode::{PostWorkModeSpec, RequestCounter, WorkMode};
use std::net::{IpAddr, SocketAddr};
use tokio::net::lookup_host;
use url::Host;

pub async fn read_body_from(path: &std::path::PathBuf) -> Result<bytes::Bytes, std::io::Error> {
    tokio::fs::read(path).await.map(bytes::Bytes::from)
}

/// Resolves a hostname to an IPv4 address
pub async fn resolve_ipv4(host: &str) -> Result<Option<IpAddr>, std::io::Error> {
    let host_with_port = format!("{}:443", host);

    for addr in lookup_host(&host_with_port).await? {
        if let SocketAddr::V4(v4) = addr {
            return Ok(Some(IpAddr::V4(*v4.ip())));
        }
    }

    Ok(None)
}

/// Resolves a hostname to an IPv6 address
pub async fn resolve_ipv6(host: &str) -> Result<Option<IpAddr>, std::io::Error> {
    let host_with_port = format!("{}:443", host);

    for addr in lookup_host(&host_with_port).await? {
        if let SocketAddr::V6(v6) = addr {
            return Ok(Some(IpAddr::V6(*v6.ip())));
        }
    }

    Ok(None)
}

pub async fn prepare_work_instance(args: Opts) -> Result<WorkInstance, UbwError> {
    let url = args.url;
    let resolve = match (url.host(), args.ipv4, args.ipv6) {
        (Some(Host::Domain(host)), v4, v6) => {
            if v6 {
                let v6_resolve = resolve_ipv6(host)
                    .await
                    .map_err(UbwError::FailedToResolveDns)?;
                if v4 {
                    let v4_resolve = resolve_ipv4(host)
                        .await
                        .map_err(UbwError::FailedToResolveDns)?;
                    v6_resolve.or(v4_resolve)
                } else {
                    v6_resolve
                }
            } else if v4 {
                resolve_ipv4(host)
                    .await
                    .map_err(UbwError::FailedToResolveDns)?
            } else {
                None
            }
        }
        (Some(Host::Ipv4(host)), true, _) => Some(IpAddr::V4(host)),
        (Some(Host::Ipv6(host)), _, true) => Some(IpAddr::V6(host)),
        (None, _, _) => None,
        _ => {
            return Err(UbwError::NoWayToResolveHost);
        }
    };
    let address = resolve.or(args.host).ok_or(UbwError::NoWayToResolveHost)?;
    let address = SocketAddr::new(
        address,
        url.port_or_known_default().ok_or(UbwError::WeirdUrl)?,
    );

    let work_mode = match (args.method, args.body_string, args.body_file) {
        (hyper::Method::GET, _, _) => WorkMode::Get,
        (hyper::Method::POST, Some(body), None) => WorkMode::Post(PostWorkModeSpec {
            body: bytes::Bytes::from(body),
            content_type: args.content_type,
        }),
        (hyper::Method::POST, None, Some(path)) => WorkMode::Post(PostWorkModeSpec {
            body: read_body_from(&path)
                .await
                .map_err(UbwError::FailedToReadBodyFromFile)?,
            content_type: args.content_type,
        }),
        (hyper::Method::POST, None, None) => return Err(UbwError::RequirePostBody),
        (hyper::Method::POST, Some(_), Some(_)) => return Err(UbwError::RequirePostBody),
        (method, _, _) => return Err(UbwError::UnsupportedMethod(method)),
    };

    let header_map: WrappedHeaderMap = args.header.try_into()?;
    let header_map = header_map.0;

    Ok(WorkInstance {
        url: url.clone(),
        address,
        mode: work_mode,
        header_map,
        request_counter: RequestCounter::new(),
    })
}

pub async fn shutdown(
    shutdown_tx: tokio::sync::watch::Sender<bool>,
    duration: std::time::Duration,
) -> Result<(), tokio::sync::watch::error::SendError<bool>> {
    tokio::time::sleep(duration).await;
    shutdown_tx.send(true)
}