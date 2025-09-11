use chrono::Utc;
use k8s_openapi::api::apps::v1::Deployment;
use kube::api::{ListParams, Patch, PatchParams};
use kube::{Api, Client};
use serde_json::json;
use tracing::info;

static KUBE_AUTOROLLOUT_LABEL: &str = "kube-autorollout/enabled=true";
static KUBE_AUTOROLLOUT_FIELD_MANAGER: &str = "kube-autorollout";

#[derive(Clone)]
pub struct Context {
    /// Kubernetes client
    pub client: Client,
}

pub async fn create_client() -> anyhow::Result<Client> {
    let client = Client::try_default().await?;
    let api_server_info = client.apiserver_version().await?;
    info!(
        "Connected to namespace {}, in-cluster Kubernetes API server with version {}.{}",
        client.default_namespace(),
        api_server_info.major,
        api_server_info.minor
    );
    Ok(client)
}

pub async fn run(ctx: Context) -> anyhow::Result<()> {
    let deployments: Api<Deployment> = Api::default_namespaced(ctx.client);
    let lp = ListParams::default().labels(KUBE_AUTOROLLOUT_LABEL);

    // List the deployments based on label selector (server-side filtering)
    let deployment_list = deployments.list(&lp).await?;

    info!(
        "Scanning for digest changes in {} deployments with label {}",
        deployment_list.items.len(),
        KUBE_AUTOROLLOUT_LABEL
    );

    for deployment in deployment_list.items {
        let name = deployment.metadata.name.unwrap();
        info!("Found deployment with label: {}", name);
    }

    Ok(())
}
