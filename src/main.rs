use crate::controller::Context;
use std::env;
use tokio_cron_scheduler::{Job, JobScheduler};
use tracing::info;
use tracing_subscriber;

mod controller;
mod webserver;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    info!("Starting kube-autorollout {}", env!("CARGO_PKG_VERSION"));

    info!("Initializing K8s controller");
    let client = controller::create_client().await?;
    let ctx = Context {
        client: client.clone(),
    };

    let cron_schedule = env::var("CRON_SCHEDULE").unwrap_or_else(|_| "*/5 * * * * *".to_string());
    info!("Executing job scheduler at cron schedule {}", cron_schedule);
    let scheduler = JobScheduler::new().await?;

    // Add a job scheduled to run
    let job = Job::new_async(cron_schedule, move |_uuid, _l| {
        let ctx = ctx.clone();
        Box::pin(async move {
            info!("Cron job running every 5 seconds");
            if let Err(e) = controller::run(ctx).await {
                tracing::error!("Error running controller job: {:?}", e);
            }
        })
    })?;
    scheduler.add(job).await?;
    scheduler.start().await?;

    let app = webserver::create_app();
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], 8080));
    info!("Starting webserver on {}", addr);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
