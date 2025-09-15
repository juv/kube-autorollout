use crate::controller::ControllerContext;
use anyhow::Context;
use std::env;
use tokio_cron_scheduler::{Job, JobScheduler};
use tracing::info;
use tracing_subscriber;

mod controller;
mod image_reference;
mod oci_registry;
mod webserver;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    info!("Starting kube-autorollout {}", env!("CARGO_PKG_VERSION"));

    info!("Initializing K8s controller");
    let client = controller::create_client().await?;
    let token = env::var("REGISTRY_TOKEN").context("REGISTRY_TOKEN is not set")?;
    let enable_jfrog_artifactory_fallback = env::var("ENABLE_JFROG_ARTIFACTORY_FALLBACK")
        .context("ENABLE_JFROG_ARTIFACTORY_FALLBACK is not set")?
        .parse::<bool>()
        .context("ENABLE_JFROG_ARTIFACTORY_FALLBACK can not be parsed to boolean")?;
    let port = env::var("WEBSERVER_PORT")
        .context("WEBSERVER_PORT is not set")?
        .parse::<u16>()
        .context("WEBSERVER_PORT can not be parsed to uint16")?;
    let ctx = ControllerContext {
        client: client.clone(),
        registry_token: token.clone(),
        enable_jfrog_artifactory_fallback,
    };

    let cron_schedule = env::var("CRON_SCHEDULE").unwrap_or_else(|_| "*/15 * * * * *".to_string());
    info!("Executing job scheduler at cron schedule {}", cron_schedule);
    let scheduler = JobScheduler::new().await?;

    // Add a job scheduled to run
    let job = Job::new_async(cron_schedule, move |_uuid, _l| {
        let ctx = ctx.clone();
        Box::pin(async move {
            if let Err(e) = controller::run(ctx).await {
                tracing::error!("Error running controller job: {:?}", e);
            }
        })
    })?;
    scheduler.add(job).await?;
    scheduler.start().await?;

    let app = webserver::create_app();
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    info!("Starting webserver on {}", addr);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
