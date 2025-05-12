use std::sync::atomic::AtomicU64;
use bytes::Bytes;
use compact_str::CompactString;
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

impl WorkMode {
    pub fn method(&self) -> hyper::Method {
        match self {
            WorkMode::Get => hyper::Method::GET,
            WorkMode::Post(_) => hyper::Method::POST,
        }
    }
}

#[derive(Debug, Default)]
pub struct RequestCounter {
    code2_count: AtomicU64,
    code3_count: AtomicU64,
    code4_count: AtomicU64,
    code5_count: AtomicU64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientResponseCodeType {
    Code2,
    Code3,
    Code4,
    Code5,
}

impl RequestCounter {
    pub fn new() -> Self {
        Self::default()
    }
    
    pub fn inc(&self, code_type: ClientResponseCodeType) {
        match code_type {
            ClientResponseCodeType::Code2 => &self.code2_count,
            ClientResponseCodeType::Code3 => &self.code3_count,
            ClientResponseCodeType::Code4 => &self.code4_count,
            ClientResponseCodeType::Code5 => &self.code5_count,
        }.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
    
    pub fn get(&self, code_type: ClientResponseCodeType) -> u64 {
        match code_type {
            ClientResponseCodeType::Code2 => &self.code2_count,
            ClientResponseCodeType::Code3 => &self.code3_count,
            ClientResponseCodeType::Code4 => &self.code4_count,
            ClientResponseCodeType::Code5 => &self.code5_count,
        }.load(std::sync::atomic::Ordering::Relaxed)
    }
    
    pub fn reset(&self, code_type: ClientResponseCodeType) {
        match code_type {
            ClientResponseCodeType::Code2 => &self.code2_count,
            ClientResponseCodeType::Code3 => &self.code3_count,
            ClientResponseCodeType::Code4 => &self.code4_count,
            ClientResponseCodeType::Code5 => &self.code5_count,
        }.store(0, std::sync::atomic::Ordering::Relaxed);
    }
}