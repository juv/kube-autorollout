use crate::config::Config;
use crate::image_reference::ImageReference;

#[derive(Clone)]
pub struct ControllerContext {
    pub(crate) kube_client: kube::Client,
    pub(crate) config: Config,
    pub(crate) http_client: reqwest::Client,
}

pub struct ContainerImageReference {
    pub(crate) container_name: String,
    pub(crate) image_reference: ImageReference,
    pub(crate) digest: String,
}
