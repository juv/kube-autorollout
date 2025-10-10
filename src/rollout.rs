use anyhow::Context;
use chrono::Utc;
use k8s_openapi::api::apps::v1::{DaemonSet, Deployment, StatefulSet};
use k8s_openapi::api::core::v1::PodSpec;
use k8s_openapi::NamespaceResourceScope;
use kube::api::{Patch, PatchParams};
use kube::{Api, Resource};
use serde::de::DeserializeOwned;
use serde_json::json;
use std::collections::BTreeMap;
use std::fmt::Debug;
use tracing::debug;

static KUBE_AUTOROLLOUT_ANNOTATION: &str = "kube-autorollout/restartedAt";
static KUBE_AUTOROLLOUT_FIELD_MANAGER: &str = "kube-autorollout";
static KUBECTL_ROLLOUT_ANNOTATION: &str = "kubectl.kubernetes.io/restartedAt";

pub trait Rollout
where
    Self: Resource<DynamicType = (), Scope = NamespaceResourceScope>
        + Clone
        + Debug
        + Send
        + Sync
        + DeserializeOwned
        + 'static,
{
    fn kind_name() -> &'static str {
        std::any::type_name::<Self>().split("::").last().unwrap()
    }
    fn selector(&self) -> BTreeMap<String, String>;
    fn desired_replicas(&self) -> i32;
    fn actual_replicas(&self) -> i32;
    fn pod_spec(&self) -> Option<&PodSpec>;

    fn image_pull_secrets(&self) -> Vec<String> {
        self.pod_spec()
            .and_then(|ps| ps.image_pull_secrets.as_ref())
            .map(|secrets| secrets.iter().map(|s| s.name.clone()).collect())
            .unwrap_or_default()
    }

    async fn patch_rollout_annotation(
        api: &Api<Self>,
        resource_name: &str,
        enable_kubectl_annotation: bool,
    ) -> anyhow::Result<()> {
        let k8s_resource_kind = Self::kind_name();

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
        .with_context(|| {
            format!(
                "Failed to patch {} {} to trigger rollout",
                k8s_resource_kind, resource_name
            )
        })?;
        Ok(())
    }
}

impl Rollout for Deployment {
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

    fn pod_spec(&self) -> Option<&PodSpec> {
        self.spec.as_ref().and_then(|s| s.template.spec.as_ref())
    }
}

impl Rollout for StatefulSet {
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

    fn pod_spec(&self) -> Option<&PodSpec> {
        self.spec.as_ref().and_then(|s| s.template.spec.as_ref())
    }
}

impl Rollout for DaemonSet {
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

    fn pod_spec(&self) -> Option<&PodSpec> {
        self.spec.as_ref().and_then(|s| s.template.spec.as_ref())
    }
}
