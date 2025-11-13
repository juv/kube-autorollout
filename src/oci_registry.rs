use crate::config::RegistrySecret::{ImagePullSecret, Opaque};
use crate::config::{Config, RegistrySecret};
use crate::image_reference::ImageReference;
use crate::secret_string::SecretString;
use anyhow::{Context, Result};
use axum::http::{HeaderMap, StatusCode};
use reqwest::header::{ACCEPT, AUTHORIZATION, WWW_AUTHENTICATE};
use reqwest::{Certificate, Client, Response};
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use tracing::{debug, info};

static OCI_ACCEPT_HEADER: &str =
    "application/vnd.oci.image.manifest.v1+json, application/vnd.oci.image.index.v1+json";

#[derive(Deserialize)]
struct RegistryTokenResponse {
    token: String,
}

pub fn create_client(config: &Config) -> Result<Client> {
    info!("Initializing OCI Registry HTTP client");
    // System certificates are loaded automatically with rustls-tls-native-roots
    let mut client_builder = Client::builder();

    for file_path in &config.tls.ca_certificate_paths {
        let file_content = fs::read(file_path)
            .with_context(|| format!("Failed to read file {}", file_path.to_str().unwrap()))?;
        let cert = Certificate::from_pem(&file_content).context("Failed to parse certificate")?;
        client_builder = client_builder.add_root_certificate(cert);
        info!(
            "Adding ca certificate(s) given in file {} to truststore",
            file_path.to_str().unwrap()
        );
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

    let response = fetch_docker_manifest(client, registry_secret, &url)
        .await
        .with_context(|| format!("Failed to fetch manifest from {}", url))?;

    match response.status() {
        StatusCode::OK => {
            let digest = get_digest_from_response(&response)?;
            return Ok(digest);
        }

        StatusCode::UNAUTHORIZED => {
            if response.headers().contains_key(WWW_AUTHENTICATE) {
                let www_authenticate_header = response
                    .headers()
                    .get(WWW_AUTHENTICATE)
                    .expect(&format!(
                        "Missing header {} from registry {}",
                        WWW_AUTHENTICATE, registry
                    ))
                    .to_str()?;

                let registry_secret = handle_oauth_authentication_challenge(
                    client,
                    registry,
                    registry_secret,
                    www_authenticate_header,
                )
                .await
                .context("Failed to fetch OAuth token from")?;

                let response = fetch_docker_manifest(client, &registry_secret, &url)
                    .await
                    .with_context(|| format!("Failed to fetch manifest from {}", url))?;

                debug!("Authentication challenge response: {:?}", response);

                let digest = get_digest_from_response(&response)?;
                return Ok(digest);
            }
        }

        StatusCode::NOT_FOUND => {
            if enable_jfrog_artifactory_fallback && is_artifactory_response(&response.headers()) {
                let fallback_url = get_artifactory_fallback_url(image_reference, registry)?;
                info!(
                    "Received http status {} previously, fetching digest from Artifactory fallback url {}",
                    response.status(),
                    fallback_url
                );

                let response = fetch_docker_manifest(client, registry_secret, &fallback_url)
                    .await
                    .with_context(|| {
                        format!(
                            "Failed to fetch manifest from Artifactory fallback url {}",
                            fallback_url
                        )
                    })?;

                let digest = get_digest_from_response(&response)?;
                return Ok(digest);
            }
        }

        status => {
            anyhow::bail!(
                "Registry {} returned error status {} while fetching OCI image manifest",
                image_reference.registry,
                status
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
    registry_secret: &RegistrySecret,
    url: &str,
) -> Result<Response> {
    info!("Fetching docker manifest for from URL {}", url);

    let authorization_header = get_authorization_header(registry_secret);
    debug!("Authorization header: {}", authorization_header);

    let response = client
        .get(url)
        .header(ACCEPT, OCI_ACCEPT_HEADER)
        .header(AUTHORIZATION, authorization_header)
        .send()
        .await
        .context("Failed to send request to fetch manifest")?;

    debug!("Fetch Docker manifest response: {:?}", response);

    Ok(response)
}

fn get_artifactory_fallback_url(
    image_reference: &ImageReference,
    registry: &str,
) -> Result<String> {
    let mut repository_parts = image_reference.repository.split('/');
    let repository = repository_parts
        .next()
        .context("Repository name is missing")?;
    let image = repository_parts.next().context("Image name is missing")?;
    // Create URL according to JFrog Artifactory's Repository Path Method (https://jfrog.com/help/r/jfrog-artifactory-documentation/the-repository-path-method-for-docker)
    let fallback_url = format!(
        "https://{}/artifactory/api/docker/{}/v2/{}/manifests/{}",
        registry, repository, image, image_reference.tag
    );

    Ok(fallback_url)
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
            let first_docker_config = docker_config.auths.iter().next().unwrap();
            let docker_secret = &first_docker_config.1.auth;
            format!("Basic {}", docker_secret.expose_secret())
        }
        RegistrySecret::None => String::new(),
    }
}

async fn handle_oauth_authentication_challenge(
    client: &Client,
    registry: &str,
    registry_secret: &RegistrySecret,
    www_authenticate_header: &str,
) -> Result<RegistrySecret> {
    debug!(
        "Trying to parse WWW-Authenticate header response from registry {}: {}",
        registry, www_authenticate_header
    );

    //parse auth challenge information from WWW-Authenticate header: https://datatracker.ietf.org/doc/html/rfc6750#section-3
    //example: WWW-Authenticate: Bearer realm="https://auth.docker.io/token",service="registry.docker.io",scope="repository:samalba/my-app:pull,push"
    let auth_challenge_params: Vec<_> = www_authenticate_header[7..].split(',').collect();
    let mut auth_challenge_map: HashMap<_, _> = auth_challenge_params
        .iter()
        .filter_map(|field| {
            let mut parts = field.splitn(2, '=');
            let key = parts.next()?.trim().trim_matches('"');
            let value = parts.next()?.trim().trim_matches('"');
            Some((key, value))
        })
        .collect();

    let realm = auth_challenge_map.remove("realm").with_context(|| {
        format!(
            "Expected missing field realm in WWW-Authenticate challenge from {}",
            registry
        )
    })?;
    let service = auth_challenge_map.get("service").with_context(|| {
        format!(
            "Expected missing field service in WWW-Authenticate challenge from {}",
            registry
        )
    })?;
    let scope = auth_challenge_map.get("scope").with_context(|| {
        format!(
            "Expected missing field scope in WWW-Authenticate challenge from {}",
            registry
        )
    })?;

    info!(
        "Requesting authentication token from {} for service {} and scope {}",
        realm, service, scope
    );

    let token_url = format!("{}?service={}&scope={}", realm, service, scope);
    let token_response = client
        .get(&token_url)
        .header(AUTHORIZATION, get_authorization_header(registry_secret))
        .send()
        .await
        .context("Failed to get token from registry")?;

    match token_response.status() {
        StatusCode::OK => {
            let token_content = token_response
                .json::<RegistryTokenResponse>()
                .await
                .context("Failed to parse JSON response from registry")?;

            let registry_secret = RegistrySecret::Opaque {
                username: None,
                token: SecretString::new(token_content.token),
            };
            Ok(registry_secret)
        }

        status => {
            anyhow::bail!(
                "Failed to retrieve OAuth authentication token from {}, error code {}",
                realm,
                status
            );
        }
    }
}
