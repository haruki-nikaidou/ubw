#![deny(clippy::panic)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]

pub mod opts;
pub mod work_mode;
pub mod before_request;
pub mod client;
mod pcg64si;

#[derive(thiserror::Error, Debug)]
pub enum UbwError {
    #[error("Failed to resolve DNS {0}")]
    FailedToResolveDns(std::io::Error),
    
    #[error("Failed to read body from file {0}")]
    FailedToReadBodyFromFile(std::io::Error),

    #[error("According to the args, there is no way to resolve the host. Please check your arguments.")]
    NoWayToResolveHost,
    
    #[error("You need to specify a body for a POST request")]
    RequirePostBody,
    
    #[error("Unsupported method {0}")]
    UnsupportedMethod(hyper::Method),
    
    #[error("The URL is not HTTP or HTTPS")]
    WeirdUrl,
    
    #[error("Failed to parse header list {0}")]
    InvalidHeaderList(#[from] opts::ParseHeaderListError),
}

#[tokio::main]
async fn main() {
    
}
