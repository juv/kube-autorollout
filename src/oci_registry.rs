use crate::image_reference::ImageReference;
use anyhow::{Context, Result};
use reqwest::header::{ACCEPT, AUTHORIZATION};
use reqwest::Response;
use tracing::info;

pub async fn fetch_digest_from_tag(
    image_reference: &ImageReference,
    registry_auth_token: &str,
    enable_jfrog_artifactory_fallback: bool,
) -> Result<String> {
    let url = format!(
        "https://{}/v2/{}/manifests/{}",
        image_reference.registry, image_reference.repository, image_reference.tag
    );

    let digest;
    let response = fetch_docker_manifest(image_reference, registry_auth_token, &url).await;
    if let Ok(ref response) = response {
        digest = get_digest_from_response(&response)?;
    } else if enable_jfrog_artifactory_fallback {
        info!("Falling back to JFrog Artifactory specific Repository Path Method");
        let repository_name = image_reference.repository.split('/').next().unwrap();
        // Create URL according to JFrog Artifactory's Repository Path Method (https://jfrog.com/help/r/jfrog-artifactory-documentation/the-repository-path-method-for-docker)
        let fallback_url = format!(
            "https://{}/artifactory/api/docker/{}/v2/{}/manifests/{}",
            image_reference.registry,
            repository_name,
            image_reference.repository,
            image_reference.tag
        );
        let fallback_response =
            fetch_docker_manifest(image_reference, registry_auth_token, &fallback_url).await?;
        digest = get_digest_from_response(&fallback_response)?;
    } else {
        anyhow::bail!(
            "Failed to fetch digest from registry's {} metadata endpoint",
            image_reference.registry
        );
    }

    info!("Found updated image digest {}", digest);

    Ok(digest)
}

async fn fetch_docker_manifest(
    image_reference: &ImageReference,
    registry_auth_token: &str,
    url: &String,
) -> Result<Response> {
    info!("Fetching docker manifest for from URL {}", url);
    let client = reqwest::Client::new();
    let response = client
        .get(url)
        .header(ACCEPT, "application/vnd.oci.image.manifest.v1+json")
        .header(AUTHORIZATION, format!("Bearer {}", registry_auth_token))
        .send()
        .await
        .context("Failed to send request to fetch manifest")?;

    if !response.status().is_success() {
        anyhow::bail!(
            "Registry {} returned error status {} while fetching OCI image manifest",
            image_reference.registry,
            response.status()
        );
    }
    Ok(response)
}

fn get_digest_from_response(response: &Response) -> Result<String> {
    Ok(response
        .headers()
        .get("Docker-Content-Digest")
        .context("Response does not contain HTTP header Docker-Content-Digest")?
        .to_str()
        .context("Received invalid UTF-8 content in Docker-Content-Digest header")?
        .to_owned())
}
