use crate::image_reference::ImageReference;
use anyhow::{Context, Result};
use reqwest::header::{ACCEPT, AUTHORIZATION};
use std::env;
use tracing::info;

pub async fn fetch_digest_from_tag(
    image_reference: &ImageReference,
    registry_auth_token: &str,
) -> Result<String> {
    let url = format!(
        "https://{}/v2/{}/manifests/{}",
        image_reference.registry, image_reference.repository, image_reference.tag
    );

    info!("Fetching docker manifest for from URL {}", url);

    let client = reqwest::Client::new();
    let response = client
        .get(&url)
        .header(ACCEPT, "application/vnd.oci.image.manifest.v1+json")
        .header(AUTHORIZATION, format!("Bearer {}", registry_auth_token))
        .send()
        .await
        .context("Failed to send request to fetch manifest")?;

    if !response.status().is_success() {
        return Err(anyhow::anyhow!(
            "Registry {} returned error status {} while fetching OCI image manifest",
            image_reference.registry,
            response.status()
        ));
    }

    let digest = response
        .headers()
        .get("Docker-Content-Digest")
        .context("Response does not contain HTTP header Docker-Content-Digest")?
        .to_str()
        .context("Received invalid UTF-8 content in Docker-Content-Digest header")?
        .to_owned();

    info!("Found updated image digest {}", digest);

    Ok(digest)
}
