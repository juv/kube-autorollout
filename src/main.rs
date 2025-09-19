use crate::state::ControllerContext;
use anyhow::Context;
use std::env;
use tokio_cron_scheduler::{Job, JobScheduler};
use tokio_util::sync::CancellationToken;
use tracing::{error, info};
use tracing_subscriber;

mod config;
mod controller;
mod image_reference;
mod oci_registry;
mod secret_string;
mod state;
mod webserver;

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("Failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        let mut sigterm =
            signal(SignalKind::terminate()).expect("Failed to install SIGTERM handler");

        tokio::select! {
            _ = ctrl_c => {},
            _ = sigterm.recv() => {},
        }
    }

    #[cfg(not(unix))]
    {
        ctrl_c.await;
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    info!("Starting kube-autorollout {} 🚀", env!("CARGO_PKG_VERSION"));

    let config_file = env::var("CONFIG_FILE").context("CONFIG_FILE is not set")?;
    let config = config::load_config(config_file)?;

    let kube_client = controller::create_client().await?;
    let http_client = oci_registry::create_client(&config)?;

    let ctx = ControllerContext {
        kube_client: kube_client.clone(),
        config: config.clone(),
        http_client,
    };

    let cron_schedule = env::var("CRON_SCHEDULE").unwrap_or_else(|_| "*/15 * * * * *".to_string());
    info!("Executing job scheduler at cron schedule {}", cron_schedule);
    let mut scheduler = JobScheduler::new().await?;
    let main_cancellation_token = CancellationToken::new();
    let cronjob_cancellation_token = main_cancellation_token.clone();

    // Add a job scheduled to run
    let job = Job::new_async(cron_schedule, move |_uuid, _l| {
        let ctx = ctx.clone();
        let cronjob_cancellation_token = cronjob_cancellation_token.clone();
        Box::pin(async move {
            tokio::select! {
            _ = cronjob_cancellation_token.cancelled() => {
                info!("Shutdown signal received, stopping controller job scheduler");
            }
            result = controller::run(ctx) => {
                if let Err(e) = result {
                    error!("Error while running controller job: {:?}", e);
                }
            }
            }
        })
    })?;
    scheduler.add(job).await?;
    scheduler.start().await?;

    let app = webserver::create_app();
    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], config.webserver.port));
    info!("Starting webserver on {}", addr);
    let listener = tokio::net::TcpListener::bind(addr).await?;

    tokio::select! {
        res = axum::serve(listener, app)
            .with_graceful_shutdown(shutdown_signal()) => {
            if let Err(e) = res {
                error!("Webserver error: {:?}", e);
            }
        }
        _ = shutdown_signal() => {
            info!("Shutdown signal received, stopping webserver");
        }
    }

    // Cancel the cron scheduler jobs
    main_cancellation_token.cancel();
    scheduler.shutdown().await?;

    Ok(())
}
