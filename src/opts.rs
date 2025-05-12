use std::net::IpAddr;
use std::str::FromStr;
use clap::Parser;
use compact_str::CompactString;
use hyper::{HeaderMap, Method};
use url::Url;

#[derive(Parser)]
#[command(version, about, long_about = None)]
#[command(arg_required_else_help = true)]
pub struct Opts {
    #[arg(help = "The URL to fetch", short)]
    pub url: Url,
    
    #[arg(help = "The number of concurrent requests", short, default_value_t = 1)]
    pub concurrent: u16,
    
    #[arg(help = "The maximum time to wait for a response", short = 't')]
    pub max_time: Option<humantime::Duration>,
    
    #[arg(help = "Simulate host file", short = 'i')]
    pub host: Option<IpAddr>,
    
    #[arg(help = "Add headers to the request", short = 'H', long = "header")]
    pub header: Vec<HeaderListItem>,
    
    #[arg(help = "The HTTP method to use", short = 'X', default_value_t = Method::GET)]
    pub method: Method,
    
    #[arg(help = "The body to send", short = 'd', long = "data")]
    pub body_string: Option<String>,
    
    #[arg(help = "The file to send", short = 'D', long = "data-binary")]
    pub body_file: Option<std::path::PathBuf>,
    
    #[arg(help = "Add headers to the proxy request", long = "proxy-header")]
    pub proxy_headers: Vec<CompactString>,
    
    #[arg(help = "Add headers to the accept request", short = 'A', long = "accept-header")]
    pub accept_headers: Vec<CompactString>,
    
    #[arg(help = "The content type to use", short = 'T', long = "content-type")]
    pub content_type: Option<CompactString>,
    
    #[arg(help = "Use IPv6", short = '6', default_value_t = true)]
    pub ipv6: bool,
    
    #[arg(help = "Use IPv4", short = '4', default_value_t = true)]
    pub ipv4: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeaderListItem {
    pub header_name: CompactString,
    pub header_value: CompactString,
}

impl FromStr for HeaderListItem {
    type Err = anyhow::Error;
    
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (header_name, header_value) = s.split_once(':').ok_or_else(|| anyhow::anyhow!("Invalid header format"))?;
        Ok(Self {
            header_name: header_name.trim().into(),
            header_value: header_value.trim().into(),
        })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ParseHeaderListError {
    #[error("Invalid header name {0}")]
    InvalidHeaderName(#[from] hyper::http::header::InvalidHeaderName),
    
    #[error("Invalid header value {0}")]
    InvalidHeaderValue(#[from] hyper::http::header::InvalidHeaderValue),
}

#[derive(Debug, Clone)]
pub struct WrappedHeaderMap(pub hyper::HeaderMap);

impl TryFrom<Vec<HeaderListItem>> for WrappedHeaderMap {
    type Error = ParseHeaderListError;
    
    fn try_from(value: Vec<HeaderListItem>) -> Result<Self, Self::Error> {
        value
            .into_iter()
            .map(|h| (h.header_name, h.header_value))
            .map(|(name, value)| {
                let name = name.parse::<hyper::http::header::HeaderName>().map_err(ParseHeaderListError::InvalidHeaderName)?;
                let value = value.parse::<hyper::http::header::HeaderValue>().map_err(ParseHeaderListError::InvalidHeaderValue)?;
                Ok((name, value))
            })
            .try_fold(WrappedHeaderMap(HeaderMap::new()), |mut acc, item: Result<_, ParseHeaderListError>| {
                let (name, value) = item?;
                acc.0.insert(name, value);
                Ok(acc)
            })
    }
}