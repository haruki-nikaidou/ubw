use std::net::{IpAddr, SocketAddr};
use tokio::net::lookup_host;
use crate::opts::{Opts, WrappedHeaderMap};
use crate::UbwError;
use crate::work_mode::{PostWorkModeSpec, RequestCounter, WorkInstance, WorkMode};

pub async fn read_body_from(path: &std::path::PathBuf) -> Result<bytes::Bytes, std::io::Error> {
    tokio::fs::read(path)
        .await
        .map(bytes::Bytes::from)
}


/// Resolves a hostname to an IPv4 address
pub async fn resolve_ipv4(host: &str) -> Result<Option<IpAddr>, std::io::Error> {
    let host_with_port = format!("{}:0", host);

    for addr in lookup_host(&host_with_port).await? {
        if let SocketAddr::V4(v4) = addr {
            return Ok(Some(IpAddr::V4(*v4.ip())));
        }
    }

    Ok(None)
}

/// Resolves a hostname to an IPv6 address
pub async fn resolve_ipv6(host: &str) -> Result<Option<IpAddr>, std::io::Error> {
    let host_with_port = format!("{}:0", host);

    for addr in lookup_host(&host_with_port).await? {
        if let SocketAddr::V6(v6) = addr {
            return Ok(Some(IpAddr::V6(*v6.ip())));
        }
    }

    Ok(None)
}

pub async fn prepare_work_instance(
    args: Opts,
) -> Result<WorkInstance, UbwError> {
    let resolve = if args.ipv6 {
        let host = args.url.host()
            .ok_or(UbwError::NoWayToResolveHost)?;
        let v6 = resolve_ipv6(host).await.map_err(UbwError::FailedToResolveDns)?;
        let v4 = resolve_ipv4(host).await.map_err(UbwError::FailedToResolveDns)?;
        v6.or(v4)
    } else if args.ipv4 {
        let host = args.url.host()
            .ok_or(UbwError::NoWayToResolveHost)?;
        resolve_ipv4(host).await.map_err(UbwError::FailedToResolveDns)?
    } else {
        None
    };
    let address = resolve.or(args.host);

    let work_mode = match (args.method, args.body_string, args.body_file) {
        (hyper::Method::GET, _, _) => WorkMode::Get,
        (hyper::Method::POST, Some(body), None) => WorkMode::Post(PostWorkModeSpec {
            body: bytes::Bytes::from(body),
            content_type: args.content_type,
        }),
        (hyper::Method::POST, None, Some(path)) => WorkMode::Post(PostWorkModeSpec {
            body: read_body_from(&path).await.map_err(UbwError::FailedToReadBodyFromFile)?,
            content_type: args.content_type,
        }),
        (hyper::Method::POST, None, None) => return Err(UbwError::RequirePostBody),
        (hyper::Method::POST, Some(_), Some(_)) => return Err(UbwError::RequirePostBody),
        (method, _, _) => return Err(UbwError::UnsupportedMethod(method)),
    };
    
    let header_map: WrappedHeaderMap = args.header.try_into()?;
    let header_map = header_map.0;
    
    Ok(WorkInstance {
        url: args.url,
        address: address.ok_or(UbwError::NoWayToResolveHost)?,
        mode: work_mode,
        header_map,
        accept_headers: args.accept_headers,
        proxy_headers: args.proxy_headers,
        max_time: args.max_time,
        request_counter: RequestCounter::new(),
    })
}