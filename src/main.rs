#![deny(clippy::panic)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]

use clap::Parser;
use tokio::task::JoinSet;
use crate::work_mode::counter_print;

pub mod before_request;
pub mod client;
pub mod opts;
mod pcg64si;
pub mod work_mode;
pub mod emiya;

#[derive(thiserror::Error, Debug)]
pub enum UbwError {
    #[error("Failed to resolve DNS {0}")]
    FailedToResolveDns(std::io::Error),

    #[error("Failed to read body from file {0}")]
    FailedToReadBodyFromFile(std::io::Error),

    #[error(
        "According to the args, there is no way to resolve the host. Please check your arguments."
    )]
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
async fn main() -> anyhow::Result<()> {
    // if no args, only cast
    if std::env::args().len() == 1 {
        emiya::wait_for_incantation().await?;
        return Ok(());
    }
    
    let opts = opts::Opts::parse();
    
    let concurrent = opts.concurrent;
    let shutdown_after = opts.max_time;

    if !opts.instant_cast {
        emiya::wait_for_incantation().await?;
    }

    let work_instance = before_request::prepare_work_instance(opts).await?;
    let work_instance = std::sync::Arc::new(work_instance);
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

    let mut handlers = JoinSet::<()>::new();
    
    for _ in 0..concurrent {
        let work_instance = work_instance.clone();
        let mut shutdown_rx = shutdown_rx.clone();
        handlers.spawn(async move {
            client::request_loop(work_instance, &mut shutdown_rx).await;
        });
    }
    
    let arc_for_counter_monitor = work_instance.clone();
    let shutdown_sig_for_counter_monitor = shutdown_rx.clone();
    tokio::spawn(async move {
        let mut shutdown_sig_for_counter_monitor = shutdown_sig_for_counter_monitor;
        counter_print(
            &arc_for_counter_monitor.request_counter,
            &mut shutdown_sig_for_counter_monitor,
        ).await
    });
    
    if let Some(shutdown_after) = shutdown_after {
        before_request::shutdown(shutdown_tx, *shutdown_after).await?;
    } else {
        // wait for shutdown
        shutdown_rx.clone().changed().await?;
    }
    
    Ok(())
}

