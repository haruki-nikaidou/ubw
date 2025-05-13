#![deny(clippy::panic)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]

use clap::Parser;
use tokio::task::JoinSet;
use tokio::signal;
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
    
    // Handle graceful shutdown from signals
    tokio::spawn(handle_shutdown_signals(shutdown_tx.clone()));
    
    if let Some(shutdown_after) = shutdown_after {
        // Create a separate task for the timed shutdown
        let shutdown_tx_for_timer = shutdown_tx.clone();
        tokio::spawn(async move {
            if let Err(e) = before_request::shutdown(shutdown_tx_for_timer, *shutdown_after).await {
                eprintln!("Failed to send shutdown signal: {e}");
            }
        });
    }
    
    // Wait for shutdown signal
    shutdown_rx.clone().changed().await?;
    println!("Shutting down gracefully...");
    
    // Wait for all tasks to complete (optional timeout could be added)
    while handlers.join_next().await.is_some() {}
    
    println!("All tasks completed, goodbye!");
    Ok(())
}

async fn handle_shutdown_signals(shutdown_tx: tokio::sync::watch::Sender<bool>) -> anyhow::Result<()> {
    let ctrl_c = async {
        #[allow(clippy::expect_used)]
        signal::ctrl_c().await.expect("Failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        #[allow(clippy::expect_used)]
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("Failed to install terminate signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    #[cfg(windows)]
    #[allow(clippy::expect_used)]
    let ctrl_shutdown = async {
        signal::windows::ctrl_shutdown()
            .expect("Failed to install Ctrl+Shutdown handler")
            .recv()
            .await;
    };

    #[cfg(not(windows))]
    let ctrl_shutdown = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {
            println!("Received Ctrl+C signal");
        }
        _ = terminate => {
            println!("Received terminate signal");
        }
        _ = ctrl_shutdown => {
            println!("Received shutdown signal");
        }
    }

    // Send shutdown signal to all tasks
    let _ = shutdown_tx.send(true);
    
    Ok(())
}
