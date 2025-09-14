use crate::image_reference::ImageReference;
use crate::oci_registry::fetch_digest_from_tag;
use anyhow::Context;
use chrono::Utc;
use k8s_openapi::api::apps::v1::Deployment;
use k8s_openapi::api::core::v1::{ContainerStatus, Pod};
use kube::api::{ListParams, Patch, PatchParams};
use kube::{Api, Client};
use serde_json::json;
use std::cmp::Ordering;
use std::collections::BTreeMap;
use tracing::{debug, info};

static KUBE_AUTOROLLOUT_LABEL: &str = "kube-autorollout/enabled=true";
static KUBE_AUTOROLLOUT_ANNOTATION: &str = "kube-autorollout/restartedAt";
static KUBE_AUTOROLLOUT_FIELD_MANAGER: &str = "kube-autorollout";

#[derive(Clone)]
pub struct ControllerContext {
    /// Kubernetes client
    pub client: Client,
    pub(crate) registry_token: String,
    pub enable_jfrog_artifactory_fallback: bool,
}

struct ContainerImageReference {
    container_name: String,
    image_reference: ImageReference,
    digest: String,
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

pub async fn run(ctx: ControllerContext) -> anyhow::Result<()> {
    let deployments: Api<Deployment> = Api::default_namespaced(ctx.client.clone());
    let pods: Api<Pod> = Api::default_namespaced(ctx.client.clone());
    let lp = ListParams::default().labels(KUBE_AUTOROLLOUT_LABEL);

    // List the deployments based on label selector (server-side filtering)
    let deployment_list = deployments.list(&lp).await?;

    info!(
        "Scanning for digest changes in {} deployments with label {}",
        deployment_list.items.len(),
        KUBE_AUTOROLLOUT_LABEL
    );

    for deployment in deployment_list.items {
        let deployment_name = deployment.metadata.name.unwrap();
        info!("Found deployment with label: {}", deployment_name);
        let status = deployment.status.unwrap();
        let spec = deployment.spec.unwrap();
        let desired_replicas = spec.replicas.unwrap();
        let actual_replicas = status.replicas.unwrap_or(0);
        if desired_replicas > 0 && actual_replicas > 0 {
            let selector = spec.selector.match_labels.clone().unwrap();
            let pod = get_associated_pod(&pods, &selector).await?;
            let pod_name = pod.metadata.name.as_ref().unwrap();

            let container_image_references = get_pod_container_image_references(&pod);

            for reference in container_image_references?.iter() {
                info!(
                    "Found pod {} container {} with image {} and current digest {}",
                    pod_name, reference.container_name, reference.image_reference, reference.digest
                );

                let updated_digest = fetch_digest_from_tag(
                    &reference.image_reference,
                    ctx.registry_token.as_ref(),
                    ctx.enable_jfrog_artifactory_fallback,
                )
                .await
                .context("Failed to retrieve updated digest from registry")?;

                if reference.digest.ne(&updated_digest) {
                    info!(
                        "Triggering rollout for deployment {} to digest {}",
                        deployment_name, updated_digest
                    );
                    patch_deployment(&deployments, &deployment_name)
                        .await
                        .context(format!(
                            "Failed to patch deployment {} to trigger rollout",
                            deployment_name
                        ))?;
                    info!(
                        "Successfully triggered rollout for deployment {}",
                        deployment_name
                    );
                    continue;
                } else {
                    info!(
                        "Skipping deployment {}, digest is up to date",
                        deployment_name
                    );
                }
            }
        } else {
            info!(
                "Skipping deployment {} as desired replicas are {} and actual replicas are {}",
                deployment_name, desired_replicas, actual_replicas
            );
        }
    }

    Ok(())
}

async fn patch_deployment(deployments: &Api<Deployment>, name: &String) -> anyhow::Result<()> {
    let patch = json!({
        "spec": {
            "template": {
                "metadata": {
                    "annotations": {
                        KUBE_AUTOROLLOUT_ANNOTATION: Utc::now().to_rfc3339(),
                    }
                }
            }
        }
    });

    debug!("Patching deployment {} with patch {}", name, patch);

    deployments
        .patch(
            name,
            &PatchParams::apply(KUBE_AUTOROLLOUT_FIELD_MANAGER),
            &Patch::Merge(&patch),
        )
        .await?;
    Ok(())
}

async fn get_associated_pod(
    pods: &Api<Pod>,
    selector: &BTreeMap<String, String>,
) -> anyhow::Result<Pod> {
    // Build label selector string like "key1=value1,key2=value2"
    let label_selector = selector
        .iter()
        .map(|(k, v)| format!("{}={}", k, v))
        .collect::<Vec<_>>()
        .join(",");

    // List pods with the label selector
    let lp = ListParams::default().labels(&label_selector);
    let mut pod_list = pods.list(&lp).await?;

    pod_list
        .items
        .sort_by(|a, b| sort_pods_by_creation_timestamp(&a, &b));

    pod_list
        .into_iter()
        .next()
        .context(format!("No pod found matching selector {}", label_selector))
}

fn sort_pods_by_creation_timestamp(a: &Pod, b: &Pod) -> Ordering {
    let a = &a.metadata.creation_timestamp;
    let b = &b.metadata.creation_timestamp;

    b.cmp(&a)
}

fn get_pod_container_image_references(pod: &Pod) -> anyhow::Result<Vec<ContainerImageReference>> {
    let container_statuses = pod
        .status
        .as_ref()
        .and_then(|s| s.container_statuses.as_ref())
        .context("Failed to get container status")?;

    let references: Result<Vec<_>, _> = container_statuses
        .iter()
        .map(|container_status| get_container_image_reference(container_status))
        .collect();

    Ok(references?)
}

fn get_container_image_reference(
    container_status: &ContainerStatus,
) -> anyhow::Result<ContainerImageReference> {
    let container_name = container_status.name.clone();
    let image = container_status.image.clone();
    let image_id = container_status.image_id.clone();

    let image_reference: ImageReference =
        ImageReference::parse(&image).context("Failed to parse image reference")?;
    let digest = image_id.split("@").collect::<Vec<&str>>()[1].to_string();

    Ok(ContainerImageReference {
        container_name,
        image_reference,
        digest,
    })
}
