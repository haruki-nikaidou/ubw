use std::net::IpAddr;
use std::sync::atomic::AtomicU64;
use bytes::Bytes;
use compact_str::CompactString;
use hyper::HeaderMap;

#[derive(Debug, Clone)]
pub struct PostWorkModeSpec {
    pub body: Bytes,
    pub content_type: Option<CompactString>,
}

#[derive(Debug, Clone)]
pub enum WorkMode {
    Get,
    Post(PostWorkModeSpec),
}

#[derive(Debug)]
pub struct WorkInstance {
    pub url: hyper::Uri,
    pub address: IpAddr,
    pub mode: WorkMode,
    pub header_map: HeaderMap,
    pub accept_headers: Vec<CompactString>,
    pub proxy_headers: Vec<CompactString>,
    pub max_time: Option<humantime::Duration>,
    pub request_counter: RequestCounter,
}

#[derive(Debug, Default)]
pub struct RequestCounter {
    count: AtomicU64
}

impl RequestCounter {
    pub fn new() -> Self {
        Self::default()
    }
    
    pub fn inc(&self) {
        self.count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
    
    pub fn get(&self) -> u64 {
        self.count.load(std::sync::atomic::Ordering::Relaxed)
    }
    
    pub fn reset(&self) {
        self.count.store(0, std::sync::atomic::Ordering::Relaxed);
    }
}