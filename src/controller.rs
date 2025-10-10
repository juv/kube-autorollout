use crate::config::{Config, DockerConfig, RegistrySecret};
use crate::image_reference::ImageReference;
use crate::oci_registry::fetch_digest_from_tag;
use crate::rollout::Rollout;
use crate::state::{ContainerImageReference, ControllerContext};
use anyhow::Context;
use futures::future::try_join_all;
use globset::Glob;
use k8s_openapi::api::apps::v1::{DaemonSet, Deployment, StatefulSet};
use k8s_openapi::api::core::v1::{ContainerStatus, Pod, Secret};
use kube::api::ListParams;
use kube::{Api, Client, ResourceExt};
use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::sync::Arc;
use tracing::{debug, info, warn};

static KUBE_AUTOROLLOUT_LABEL: &str = "kube-autorollout/enabled=true";

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
    T: Rollout,
{
    let kind_name = T::kind_name();
    let api: Api<T> = Api::default_namespaced(ctx.kube_client.clone());
    let pods: Api<Pod> = Api::default_namespaced(ctx.kube_client.clone());
    let lp = ListParams::default().labels(KUBE_AUTOROLLOUT_LABEL);
    let secrets: Api<Secret> = Api::default_namespaced(ctx.kube_client.clone());

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

            let container_image_references = get_pod_container_image_references(&pod)
                .with_context(|| {
                    format!(
                        "Could not retrieve container image references for pod {}",
                        pod_name
                    )
                })?;

            let image_pull_secrets = resource.image_pull_secrets();
            debug!(
                "Parsed image pull secrets {:?} for resource {}",
                image_pull_secrets, resource_name
            );

            let image_pull_secrets = collect_image_pull_secrets(&secrets, &image_pull_secrets)
                .await
                .with_context(|| {
                    format!("Failed to collect image pull secrets for pod {}", pod_name)
                })?;

            for reference in container_image_references.iter() {
                info!(
                    "Found pod {} container {} with image {} and current digest {}",
                    pod_name, reference.container_name, reference.image_reference, reference.digest
                );

                let registry_secret =
                    find_matching_image_pull_secret(&image_pull_secrets, reference)
                        .or_else(|_| get_registry_secret_from_config(&ctx.config, reference))?;

                let recent_digest = fetch_digest_from_tag(
                    &reference.image_reference,
                    &registry_secret,
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
                    .with_context(|| {
                        format!(
                            "Failed to patch {} resource {} to trigger rollout",
                            kind_name, resource_name
                        )
                    })?;
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
        .with_context(|| format!("No pod found matching selector {}", label_selector))
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

fn normalize_image_registry_name(registry_name: &str) -> String {
    let registry_name = registry_name
        .trim_start_matches("https://")
        .trim_start_matches("http://");

    //docker.io is internally wrapped to registry-1.docker.io
    if registry_name.eq("docker.io") {
        return format!("*.{}", registry_name);
    }
    registry_name.to_string()
}

fn find_matching_image_pull_secret(
    image_pull_secrets: &Vec<DockerConfig>,
    container_image_reference: &ContainerImageReference,
) -> anyhow::Result<RegistrySecret> {
    let normalized_pod_registry_name =
        normalize_image_registry_name(&container_image_reference.image_reference.registry);
    for image_pull_secret in image_pull_secrets {
        for auth in &image_pull_secret.auths {
            let pull_secret_hostname_pattern = normalize_image_registry_name(auth.0);

            //As opposed to Docker's config json, the Kubernetes .dockerconfigjson can include * wildcards in the keys, so it needs to be glob'ed: https://kubernetes.io/docs/concepts/containers/images/#config-json
            let glob = Glob::new(&pull_secret_hostname_pattern)
                .with_context(|| {
                    format!("invalid hostname pattern {}", pull_secret_hostname_pattern)
                })?
                .compile_matcher();

            info!(
                "normalized_pod_registry_name: {}, pull_secret_hostname_pattern: {}",
                normalized_pod_registry_name, pull_secret_hostname_pattern
            );

            if glob.is_match(&normalized_pod_registry_name) {
                let registry_secret = RegistrySecret::ImagePullSecret {
                    mount_path: String::new(),
                    docker_config: image_pull_secret.clone(),
                };

                info!(
                    "Found matching image pull secret for pod registry {}",
                    normalized_pod_registry_name
                );

                return Ok(registry_secret);
            }
        }
    }
    anyhow::bail!("No matching image pull secret found");
}

async fn collect_image_pull_secrets(
    secrets: &Api<Secret>,
    image_pull_secrets: &Vec<String>,
) -> anyhow::Result<Vec<DockerConfig>> {
    let futures_vec = image_pull_secrets
        .iter()
        .map(|name| get_image_pull_secret_content(secrets, name))
        .collect::<Vec<_>>();

    let configs: Vec<DockerConfig> = try_join_all(futures_vec).await?;

    Ok(configs)
}

async fn get_image_pull_secret_content(
    secrets: &Api<Secret>,
    secret_name: &str,
) -> anyhow::Result<DockerConfig> {
    debug!("Getting secret content for secret {}", secret_name);

    let secret = secrets
        .get(secret_name)
        .await
        .with_context(|| format!("Failed to retrieve secret {}", secret_name))?;

    let data = secret
        .data
        .with_context(|| format!("Failed to retrieve secret data for secret {}", secret_name))?;

    let docker_config_bytes = &data
        .get(".dockerconfigjson")
        .with_context(|| {
            format!(
                "Failed to get .dockerconfigjson key from secret {}",
                secret_name
            )
        })?
        .0;

    let docker_config_str = str::from_utf8(docker_config_bytes)
        .context("Failed to convert .dockerconfigjson bytes to UTF-8 string")?;

    let docker_config: DockerConfig =
        serde_json::from_str(&docker_config_str).with_context(|| {
            format!(
                "Could not parse secret content to Docker Config structure for secret {}",
                secret_name
            )
        })?;

    Ok(docker_config)
}

fn get_registry_secret_from_config(
    config: &Config,
    reference: &ContainerImageReference,
) -> anyhow::Result<RegistrySecret> {
    let registry_name = &reference.image_reference.registry;
    let secret: RegistrySecret = config
        .find_registry_for_hostname(registry_name)
        .with_context(|| {
            format!(
                "Could not find registry configuration for {}",
                registry_name
            )
        })?
        .secret
        .clone();
    Ok(secret)
}
