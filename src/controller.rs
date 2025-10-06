use crate::image_reference::ImageReference;
use crate::oci_registry::fetch_digest_from_tag;
use crate::state::{ContainerImageReference, ControllerContext};
use anyhow::Context;
use chrono::Utc;
use k8s_openapi::api::apps::v1::{DaemonSet, Deployment, StatefulSet};
use k8s_openapi::api::core::v1::{ContainerStatus, Pod};
use k8s_openapi::NamespaceResourceScope;
use kube::api::{ListParams, Patch, PatchParams};
use kube::{Api, Client, Resource, ResourceExt};
use serde::de::DeserializeOwned;
use serde_json::json;
use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::fmt::Debug;
use std::sync::Arc;
use tracing::{debug, info, warn};

static KUBE_AUTOROLLOUT_LABEL: &str = "kube-autorollout/enabled=true";
static KUBE_AUTOROLLOUT_ANNOTATION: &str = "kube-autorollout/restartedAt";
static KUBE_AUTOROLLOUT_FIELD_MANAGER: &str = "kube-autorollout";
static KUBECTL_ROLLOUT_ANNOTATION: &str = "kubectl.kubernetes.io/restartedAt";

trait ResourceController
where
    Self: Resource<DynamicType = (), Scope = NamespaceResourceScope>
        + Clone
        + Debug
        + Send
        + Sync
        + DeserializeOwned
        + 'static,
{
    fn get_kind_name() -> &'static str {
        std::any::type_name::<Self>().split("::").last().unwrap()
    }
    fn selector(&self) -> BTreeMap<String, String>;
    fn desired_replicas(&self) -> i32;
    fn actual_replicas(&self) -> i32;

    async fn patch_rollout_annotation(
        api: &Api<Self>,
        resource_name: &str,
        enable_kubectl_annotation: bool,
    ) -> anyhow::Result<()> {
        let k8s_resource_kind = Self::get_kind_name();

        let annotation = match enable_kubectl_annotation {
            true => KUBECTL_ROLLOUT_ANNOTATION,
            false => KUBE_AUTOROLLOUT_ANNOTATION,
        };
        let patch = json!({
            "spec": {
                "template": {
                    "metadata": {
                        "annotations": {
                            annotation: Utc::now().to_rfc3339(),
                        }
                    }
                }
            }
        });

        debug!(
            "Patching {} {} with patch {:?}",
            k8s_resource_kind, resource_name, patch
        );
        api.patch(
            resource_name,
            &PatchParams::apply(KUBE_AUTOROLLOUT_FIELD_MANAGER),
            &Patch::Merge(&patch),
        )
        .await
        .context(format!(
            "Failed to patch {} {} to trigger rollout",
            k8s_resource_kind, resource_name
        ))?;
        Ok(())
    }
}

impl ResourceController for Deployment {
    fn selector(&self) -> BTreeMap<String, String> {
        self.spec
            .as_ref()
            .unwrap()
            .selector
            .match_labels
            .clone()
            .unwrap()
    }

    //https://kubernetes.io/docs/reference/kubernetes-api/workload-resources/deployment-v1/#DeploymentStatus
    fn desired_replicas(&self) -> i32 {
        self.spec.as_ref().unwrap().replicas.unwrap()
    }

    fn actual_replicas(&self) -> i32 {
        self.status.as_ref().unwrap().replicas.unwrap_or(0)
    }
}

impl ResourceController for StatefulSet {
    fn selector(&self) -> BTreeMap<String, String> {
        self.spec
            .as_ref()
            .unwrap()
            .selector
            .match_labels
            .clone()
            .unwrap()
    }

    //https://kubernetes.io/docs/reference/kubernetes-api/workload-resources/stateful-set-v1/#StatefulSetStatus
    fn desired_replicas(&self) -> i32 {
        self.spec.as_ref().unwrap().replicas.unwrap()
    }

    fn actual_replicas(&self) -> i32 {
        self.status.as_ref().unwrap().replicas
    }
}

impl ResourceController for DaemonSet {
    fn selector(&self) -> BTreeMap<String, String> {
        self.spec
            .as_ref()
            .unwrap()
            .selector
            .match_labels
            .clone()
            .unwrap()
    }

    //https://kubernetes.io/docs/reference/kubernetes-api/workload-resources/daemon-set-v1/#DaemonSetStatus
    fn desired_replicas(&self) -> i32 {
        self.status.as_ref().unwrap().desired_number_scheduled
    }

    fn actual_replicas(&self) -> i32 {
        self.status.as_ref().unwrap().number_ready
    }
}

pub async fn create_client() -> anyhow::Result<Client> {
    info!("Initializing K8s controller");
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
    let ctx = Arc::new(ctx);

    reconcile::<Deployment>(ctx.clone())
        .await
        .context("Failed to reconcile Deployments")?;
    reconcile::<StatefulSet>(ctx.clone())
        .await
        .context("Failed to reconcile StatefulSets")?;
    reconcile::<DaemonSet>(ctx.clone())
        .await
        .context("Failed to reconcile DaemonSets")?;

    Ok(())
}

async fn reconcile<T>(ctx: Arc<ControllerContext>) -> anyhow::Result<()>
where
    T: ResourceController,
{
    let kind_name = T::get_kind_name();
    let api: Api<T> = Api::default_namespaced(ctx.kube_client.clone());
    let pods: Api<Pod> = Api::default_namespaced(ctx.kube_client.clone());
    let lp = ListParams::default().labels(KUBE_AUTOROLLOUT_LABEL);

    // List the resources based on label selector (server-side filtering)
    let resource_list = api.list(&lp).await?;

    info!(
        "Scanning for digest changes in {} {} resources with label {}",
        resource_list.items.len(),
        kind_name,
        KUBE_AUTOROLLOUT_LABEL
    );

    for resource in resource_list.items {
        let resource_name = resource.name_any();
        info!("Found {} resource with label: {}", kind_name, resource_name);
        let desired_replicas = resource.desired_replicas();
        let actual_replicas = resource.actual_replicas();
        if desired_replicas > 0 && actual_replicas > 0 {
            let selector = resource.selector();
            let pod = get_associated_pod(&pods, &selector).await?;
            let pod_name = pod.metadata.name.as_ref().unwrap();

            warn_misconfigured_container_image_pull_policies(&pod);

            let container_image_references = get_pod_container_image_references(&pod);

            for reference in container_image_references?.iter() {
                info!(
                    "Found pod {} container {} with image {} and current digest {}",
                    pod_name, reference.container_name, reference.image_reference, reference.digest
                );

                let registry_config = ctx
                    .config
                    .find_registry_for_hostname(&reference.image_reference.registry)
                    .context(format!(
                        "Could not find registry configuration for {}",
                        reference.image_reference.registry
                    ))?;

                let recent_digest = fetch_digest_from_tag(
                    &reference.image_reference,
                    &registry_config.secret,
                    &ctx.http_client,
                    ctx.config.feature_flags.enable_jfrog_artifactory_fallback,
                )
                .await
                .context("Failed to retrieve recent digest from registry")?;

                info!("Found recent image digest {}", recent_digest);

                if reference.digest.ne(&recent_digest) {
                    info!(
                        "Triggering rollout for {} resource {} to digest {}",
                        kind_name, resource_name, recent_digest
                    );

                    T::patch_rollout_annotation(
                        &api,
                        &resource_name,
                        ctx.config.feature_flags.enable_kubectl_annotation,
                    )
                    .await
                    .context(format!(
                        "Failed to patch {} resource {} to trigger rollout",
                        kind_name, resource_name
                    ))?;
                    info!(
                        "Successfully triggered rollout for {} resource {}",
                        kind_name, resource_name
                    );
                    continue;
                } else {
                    info!(
                        "Skipping {} resource {}, digest is up to date",
                        kind_name, resource_name
                    );
                }
            }
        } else {
            info!(
                "Skipping {} resource {} as desired replicas are {} and actual replicas are {}",
                kind_name, resource_name, desired_replicas, actual_replicas
            );
        }
    }

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
        .filter(|pod| {
            let container_statuses = pod
                .status
                .clone()
                .unwrap()
                .container_statuses
                .expect("Pods should have container statuses");

            if let Some(invalid_container) = container_statuses.iter().find(|cs| cs.image_id == "")
            {
                info!(
                    "Skipping pod {} because container {} contains an empty imageID field",
                    pod.metadata.name.as_ref().unwrap(),
                    invalid_container.name
                );
                false
            } else {
                true
            }
        })
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

fn warn_misconfigured_container_image_pull_policies(pod: &Pod) {
    pod.spec
        .as_ref()
        .unwrap()
        .containers
        .iter()
        .filter(|container| container.image_pull_policy.as_deref().unwrap() != "Always")
        .for_each(|container| {
            warn!(
                "Container {} in pod {} has a misconfigured imagePullPolicy. Should be 'Always', to have an effect with kube-autorollout",
                container.name, pod.metadata.name.as_ref().unwrap()
            )
        });
}
