use crate::config::RegistrySecret::{ImagePullSecret, Opaque};
use crate::config::{Config, RegistrySecret};
use crate::image_reference::ImageReference;
use crate::secret_string::SecretString;
use anyhow::{bail, Context, Result};
use axum::http::{HeaderMap, StatusCode};
use reqwest::header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE, WWW_AUTHENTICATE};
use reqwest::{Certificate, Client, Response};
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use tracing::{debug, info};

const OCI_ACCEPT_HEADER: &str = "application/vnd.oci.image.index.v1+json, application/vnd.docker.distribution.manifest.list.v2+json, application/vnd.oci.image.manifest.v1+json, application/vnd.docker.distribution.manifest.v2+json";
const OCI_IMAGE_MANIFEST_CONTENT_TYPE: &str = "application/vnd.oci.image.manifest.v1+json";
const OCI_IMAGE_INDEX_CONTENT_TYPE: &str = "application/vnd.oci.image.index.v1+json";
const DOCKER_DISTRIBUTION_MANIFEST_CONTENT_TYPE: &str =
    "application/vnd.docker.distribution.manifest.v2+json";
const DOCKER_DISTRIBUTION_INDEX_CONTENT_TYPE: &str =
    "application/vnd.docker.distribution.manifest.list.v2+json";

#[derive(Deserialize)]
struct OciIndexManifest {
    digest: String,
}

/// OCI_IMAGE_INDEX_CONTENT_TYPE and DOCKER_DISTRIBUTION_INDEX_CONTENT_TYPE share the same content structure
#[derive(Deserialize)]
struct OciIndexResponse {
    manifests: Vec<OciIndexManifest>,
}

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
            file = %file_path.display(),
            "Adding ca certificate(s) given in file to truststore"
        );
    }

    Ok(client_builder
        .build()
        .context("Failed to build HTTP client")?)
}

pub async fn fetch_digests_from_tag(
    image_reference: &ImageReference,
    registry_secret: &RegistrySecret,
    client: &Client,
    enable_jfrog_artifactory_fallback: bool,
) -> Result<Vec<String>> {
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
            let digest = get_digests_from_response(response).await?;
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

                debug!(
                    response = ?response,
                    "Authentication challenge response"
                );

                let digest = get_digests_from_response(response).await?;
                return Ok(digest);
            }
        }

        StatusCode::NOT_FOUND => {
            if enable_jfrog_artifactory_fallback && is_artifactory_response(&response.headers()) {
                let fallback_url = get_artifactory_fallback_url(image_reference, registry)?;
                info!(
                    status = %response.status(),
                    url = %fallback_url,
                    "Received previous error status, fetching digest from Artifactory fallback url"
                );

                let response = fetch_docker_manifest(client, registry_secret, &fallback_url)
                    .await
                    .with_context(|| {
                        format!(
                            "Failed to fetch manifest from Artifactory fallback url {}",
                            fallback_url
                        )
                    })?;

                let digest = get_digests_from_response(response).await?;
                return Ok(digest);
            }
        }

        status => {
            bail!(
                "Registry {} returned error status {} while fetching OCI image manifest",
                image_reference.registry,
                status
            );
        }
    }

    bail!(
        "Failed to fetch digest from registry's {} metadata endpoint",
        registry
    );
}

async fn fetch_docker_manifest(
    client: &Client,
    registry_secret: &RegistrySecret,
    url: &str,
) -> Result<Response> {
    info!(url = %url, "Fetching docker manifest from URL");

    let authorization_header = get_authorization_header(registry_secret);

    debug!(
        authorization_header_length = %authorization_header.len(),
        "Acquired authorization header"
    );

    let response = client
        .get(url)
        .header(ACCEPT, OCI_ACCEPT_HEADER)
        .header(AUTHORIZATION, authorization_header)
        .send()
        .await
        .context("Failed to send request to fetch manifest")?;

    debug!(
        response = ?response,
        "Fetch Docker manifest response"
    );

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

async fn get_digests_from_response(response: Response) -> Result<Vec<String>> {
    let content_type = get_content_type_from_response(&response)?;
    let digests = match content_type.as_str() {
        OCI_IMAGE_MANIFEST_CONTENT_TYPE | DOCKER_DISTRIBUTION_MANIFEST_CONTENT_TYPE => {
            vec![parse_manifest_digest_from_response(&response)?]
        }
        OCI_IMAGE_INDEX_CONTENT_TYPE | DOCKER_DISTRIBUTION_INDEX_CONTENT_TYPE => {
            parse_index_digests_from_response(response).await?
        }
        _ => bail!("Unknown content type '{}'", content_type),
    };

    if digests.is_empty() {
        bail!(
            "Parsed digests for content type {} are empty",
            &content_type
        );
    }

    Ok(digests)
}

fn parse_manifest_digest_from_response(response: &Response) -> Result<String> {
    Ok(response
        .headers()
        .get("Docker-Content-Digest")
        .context("Response does not contain HTTP header Docker-Content-Digest")?
        .to_str()
        .context("Received invalid UTF-8 content in Docker-Content-Digest header")?
        .to_owned())
}

async fn parse_index_digests_from_response(response: Response) -> Result<Vec<String>> {
    let top_level_digest = parse_manifest_digest_from_response(&response)?;
    let index_body = response
        .text()
        .await
        .context("Failed to read OCI index response")?;

    collect_index_response_digests(&index_body, &top_level_digest)
}

pub(crate) fn collect_index_response_digests(
    body: &str,
    top_level_digest: &str,
) -> Result<Vec<String>> {
    let digests: OciIndexResponse =
        serde_json::from_str(body).context("Failed to parse OCI index response")?;

    let mut digests: Vec<String> = digests.manifests.iter().map(|m| m.digest.clone()).collect();
    digests.push(top_level_digest.to_owned());
    if digests.is_empty() {
        bail!("Parsed digests are empty");
    }

    Ok(digests)
}

fn get_content_type_from_response(response: &Response) -> Result<String> {
    let raw_content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .context("Response does not contain Content-Type")?
        .to_str()
        .context("Content-Type is not a string")?;

    parse_content_type(raw_content_type)
}

pub(crate) fn parse_content_type(raw_content_type: &str) -> Result<String> {
    let media_type = raw_content_type
        .split(';')
        .next()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .context("Content-Type header is empty")?;

    Ok(media_type.to_owned())
}

fn rewrite_docker_io_registry_target(registry: &str) -> &str {
    if registry.eq("docker.io") {
        //rewrite "docker.io" to "registry-1.docker.io", to mimic containerd
        debug!(
            registry = %registry,
            rewrite_to = "registry-1.docker.io",
            "Rewriting docker.io to registry-1.docker.io"
        );
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
        registry = %registry,
        header = %www_authenticate_header,
        "Trying to parse WWW-Authenticate header response from registry"
    );

    //parse auth challenge information from WWW-Authenticate header: [https://datatracker.ietf.org/doc/html/rfc6750#section-3](https://datatracker.ietf.org/doc/html/rfc6750#section-3)
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
        realm = %realm,
        service = %service,
        scope = %scope,
        "Requesting authentication token for service and scope"
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
            bail!(
                "Failed to retrieve OAuth authentication token from {}, error code {}",
                realm,
                status
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn contains_all(actual: &[String], expected: &[&str]) {
        for expected_digest in expected {
            assert!(
                actual.iter().any(|d| d == expected_digest),
                "expected digest '{}' to be present in {:?}",
                expected_digest,
                actual
            );
        }
    }

    #[test]
    fn parse_content_type_returns_media_type_without_parameters() {
        let parsed = parse_content_type("application/vnd.oci.image.index.v1+json; charset=utf-8")
            .expect("content type should parse");
        assert_eq!(parsed, "application/vnd.oci.image.index.v1+json");
    }

    #[test]
    fn parse_content_type_trims_whitespace() {
        let parsed = parse_content_type(
            " application/vnd.docker.distribution.manifest.list.v2+json ; charset=UTF-8 ",
        )
        .expect("content type should parse");
        assert_eq!(
            parsed,
            "application/vnd.docker.distribution.manifest.list.v2+json"
        );
    }

    #[test]
    fn parse_content_type_rejects_empty_value() {
        let err = parse_content_type(" ; charset=utf-8").expect_err("expected parse to fail");
        let message = format!("{err:#}");
        assert!(
            message.contains("Content-Type header is empty"),
            "unexpected error: {message}"
        );
    }

    #[test]
    fn parse_oci_index_body_returns_child_and_top_level_digests() {
        let body = r#"
        {
          "schemaVersion": 2,
          "mediaType": "application/vnd.oci.image.index.v1+json",
          "manifests": [
            {
              "mediaType": "application/vnd.oci.image.manifest.v1+json",
              "digest": "sha256:amd64digest",
              "size": 1234,
              "platform": {
                "architecture": "amd64",
                "os": "linux"
              }
            },
            {
              "mediaType": "application/vnd.oci.image.manifest.v1+json",
              "digest": "sha256:arm64digest",
              "size": 1235,
              "platform": {
                "architecture": "arm64",
                "os": "linux"
              }
            }
          ]
        }
        "#;

        let result = collect_index_response_digests(body, "sha256:indexdigest")
            .expect("OCI index body should parse");

        assert_eq!(result.len(), 3);
        contains_all(
            &result,
            &[
                "sha256:amd64digest",
                "sha256:arm64digest",
                "sha256:indexdigest",
            ],
        );
    }

    #[test]
    fn parse_docker_manifest_list_body_returns_child_and_top_level_digests() {
        let body = r#"
        {
          "schemaVersion": 2,
          "mediaType": "application/vnd.docker.distribution.manifest.list.v2+json",
          "manifests": [
            {
              "mediaType": "application/vnd.docker.distribution.manifest.v2+json",
              "digest": "sha256:docker-amd64",
              "size": 2234,
              "platform": {
                "architecture": "amd64",
                "os": "linux"
              }
            },
            {
              "mediaType": "application/vnd.docker.distribution.manifest.v2+json",
              "digest": "sha256:docker-arm64",
              "size": 2235,
              "platform": {
                "architecture": "arm64",
                "os": "linux"
              }
            }
          ]
        }
        "#;

        let result = collect_index_response_digests(body, "sha256:docker-list")
            .expect("Docker manifest list body should parse");

        assert_eq!(result.len(), 3);
        contains_all(
            &result,
            &[
                "sha256:docker-amd64",
                "sha256:docker-arm64",
                "sha256:docker-list",
            ],
        );
    }

    #[test]
    fn parse_manifest_index_body_rejects_invalid_json() {
        let body = r#"{ "manifests": [ { "digest": 123 } ] }"#;

        let err = collect_index_response_digests(body, "sha256:indexdigest")
            .expect_err("expected parse to fail");
        let message = format!("{err:#}");
        assert!(
            message.contains("Failed to parse OCI index response"),
            "unexpected error: {message}"
        );
    }

    #[test]
    fn parse_manifest_index_body_allows_top_level_only_when_manifests_is_empty() {
        let body = r#"
        {
          "schemaVersion": 2,
          "mediaType": "application/vnd.oci.image.index.v1+json",
          "manifests": []
        }
        "#;

        let result = collect_index_response_digests(body, "sha256:indexdigest")
            .expect("empty manifests should still return top-level digest");

        assert_eq!(result, vec!["sha256:indexdigest".to_string()]);
    }
}
