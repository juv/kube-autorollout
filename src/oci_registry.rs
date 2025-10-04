use crate::config::RegistrySecret::{ImagePullSecret, Opaque};
use crate::config::{Config, RegistrySecret};
use crate::image_reference::ImageReference;
use anyhow::{Context, Result};
use axum::http::HeaderMap;
use reqwest::header::{ACCEPT, AUTHORIZATION};
use reqwest::{Certificate, Client, Response};
use std::fs;
use tracing::{debug, info};

pub fn create_client(config: &Config) -> Result<Client> {
    info!("Initializing OCI Registry HTTP client");
    // System certificates are loaded automatically with rustls-tls-native-roots
    let mut client_builder = Client::builder();

    for file_path in &config.tls.ca_certificate_paths {
        let file_content = fs::read(file_path).context(format!(
            "Failed to read file {}",
            file_path.to_str().unwrap()
        ))?;
        let cert = Certificate::from_pem(&file_content).context("Failed to parse certificate")?;
        client_builder = client_builder.add_root_certificate(cert);
    }

    Ok(client_builder
        .build()
        .context("Failed to build HTTP client")?)
}

pub async fn fetch_digest_from_tag(
    image_reference: &ImageReference,
    registry_secret: &RegistrySecret,
    client: &Client,
    enable_jfrog_artifactory_fallback: bool,
) -> Result<String> {
    let registry = rewrite_docker_io_registry_target(&image_reference.registry);
    let url = format!(
        "https://{}/v2/{}/manifests/{}",
        registry, image_reference.repository, image_reference.tag
    );

    let response = fetch_docker_manifest(client, image_reference, registry_secret, &url)
        .await
        .with_context(|| format!("Failed to fetch manifest from {}", url))?;

    if let Ok(digest) = get_digest_from_response(&response) {
        return Ok(digest);
    }

    if enable_jfrog_artifactory_fallback {
        if is_artifactory_response(&response.headers()) {
            let fallback_url = get_artifactory_fallback_url(image_reference, registry);
            info!(
                "Received http status {} previously, fetching digest from Artifactory fallback url {}",
                response.status(),
                fallback_url
            );

            let response =
                fetch_docker_manifest(client, image_reference, registry_secret, &fallback_url)
                    .await
                    .context(format!(
                        "Failed to fetch manifest from Artifactory fallback url {}",
                        fallback_url
                    ))?;

            let digest = get_digest_from_response(&response).context("Failed to re")?;
            return Ok(digest);
        } else {
            anyhow::bail!(
                "Artifactory fallback is enabled but no Artifactory indicators were found in response headers from {}",
                registry
            );
        }
    }

    anyhow::bail!(
        "Failed to fetch digest from registry's {} metadata endpoint",
        registry
    );
}

async fn fetch_docker_manifest(
    client: &Client,
    image_reference: &ImageReference,
    registry_secret: &RegistrySecret,
    url: &str,
) -> Result<Response> {
    info!("Fetching docker manifest for from URL {}", url);

    let authorization_header = get_authorization_header(registry_secret);
    info!("Using authorization header {}", authorization_header);

    let response = client
        .get(url)
        .header(ACCEPT, "application/vnd.oci.image.manifest.v1+json")
        .header(AUTHORIZATION, authorization_header)
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

fn get_artifactory_fallback_url(image_reference: &ImageReference, registry: &str) -> String {
    let repository_name = image_reference.repository.split('/').next().unwrap();
    // Create URL according to JFrog Artifactory's Repository Path Method (https://jfrog.com/help/r/jfrog-artifactory-documentation/the-repository-path-method-for-docker)
    let fallback_url = format!(
        "https://{}/artifactory/api/docker/{}/v2/{}/manifests/{}",
        registry, repository_name, image_reference.repository, image_reference.tag
    );

    fallback_url
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

fn rewrite_docker_io_registry_target(registry: &str) -> &str {
    if registry.eq("docker.io") {
        //rewrite "docker.io" to "registry-1.docker.io", to mimic containerd
        debug!("Rewriting docker.io to registry-1.docker.io");
        return "registry-1.docker.io";
    }
    registry
}

fn is_artifactory_response(response_headers: &HeaderMap) -> bool {
    response_headers.contains_key("x-jfrog-version")
        || response_headers.contains_key("x-artifactory-id")
        || response_headers.contains_key("x-artifactory-node-id")
}

fn get_authorization_header(registry_secret: &RegistrySecret) -> String {
    match registry_secret {
        Opaque { token, .. } => format!("Bearer {}", token.expose_secret()),
        ImagePullSecret { docker_config, .. } => {
            format!(
                "Basic {}",
                docker_config.auths.iter().next().unwrap().1.auth
            )
        }
        RegistrySecret::None => String::new(),
    }
}
