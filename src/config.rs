use crate::secret_string::SecretString;
use anyhow::{Context, Result};
use globset::{Glob, GlobSet};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::{env, fs, path::Path};
use tracing::info;

#[derive(Default, Debug, Serialize, Deserialize, Clone)]
pub struct DockerConfig {
    pub auths: HashMap<String, DockerAuth>,
}
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DockerAuth {
    username: String,
    password: SecretString,
    pub auth: SecretString,
    email: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type")]
pub enum RegistrySecret {
    None,
    ImagePullSecret {
        #[serde(rename = "mountPath")]
        mount_path: String,
        #[serde(skip)]
        docker_config: DockerConfig,
    },
    Opaque {
        username: Option<String>,
        token: SecretString,
    },
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Registry {
    #[serde(rename = "hostnamePattern")]
    pub hostname_pattern: String,
    pub secret: RegistrySecret,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Webserver {
    pub port: u16,
}

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct Tls {
    #[serde(default, rename = "caCertificatePaths")]
    pub ca_certificate_paths: Vec<PathBuf>,
}

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct FeatureFlags {
    #[serde(default, rename = "enableJfrogArtifactoryFallback")]
    pub enable_jfrog_artifactory_fallback: bool,
    #[serde(default, rename = "enableKubectlAnnotation")]
    pub enable_kubectl_annotation: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Config {
    #[serde(default = "default_cron_schedule", rename = "cronSchedule")]
    pub cron_schedule: String,
    pub webserver: Webserver,
    pub registries: Vec<Registry>,
    #[serde(default)]
    pub tls: Tls,
    #[serde(default, rename = "featureFlags")]
    pub feature_flags: FeatureFlags,
    #[serde(skip)]
    glob_set: GlobSet,
}

fn default_cron_schedule() -> String {
    "*/45 * * * * *".to_string()
}

impl Config {
    pub fn validate(&self) -> Result<()> {
        for registry in &self.registries {
            Glob::new(&registry.hostname_pattern).with_context(|| {
                format!("invalid hostname pattern {}", registry.hostname_pattern)
            })?;
        }

        for ca_certificate_path in &self.tls.ca_certificate_paths {
            fs::metadata(ca_certificate_path).with_context(|| {
                format!(
                    "File {} does not exist or can not be accessed",
                    ca_certificate_path.to_str().unwrap()
                )
            })?;
        }
        Ok(())
    }

    pub fn parse_image_pull_secrets(&mut self) -> Result<()> {
        for registry in &mut self.registries {
            if let RegistrySecret::ImagePullSecret {
                mount_path,
                docker_config,
            } = &mut registry.secret
            {
                let file_path = format!("{}/.dockerconfigjson", &mount_path);
                let file_content = fs::read_to_string(&file_path).with_context(|| {
                    format!(
                        "Could not read ImagePullSecret content from file {}",
                        file_path
                    )
                })?;

                let parsed_config: DockerConfig = serde_json::from_str(&file_content).with_context(||
                    format!("Could not parse ImagePullSecret content to Docker Config structure from file {}", mount_path),
                )?;

                info!(
                    "Parsed ImagePullSecret content to Docker Config structure {:?}",
                    parsed_config
                );
                *docker_config = parsed_config;
            }
        }

        Ok(())
    }

    pub fn setup_glob_set(&mut self) -> Result<()> {
        let mut builder = globset::GlobSetBuilder::new();
        for registry in &self.registries {
            builder.add(Glob::new(&registry.hostname_pattern)?);
        }
        self.glob_set = builder.build()?;
        Ok(())
    }

    pub fn find_registry_for_hostname(&self, hostname: &str) -> Option<&Registry> {
        let matches = self.glob_set.matches(hostname);
        matches.into_iter().find_map(|i| self.registries.get(i))
    }
}

pub fn load_config<P: AsRef<Path>>(path: P) -> Result<Config> {
    info!("Loading config from file {}", path.as_ref().display());
    let yaml_str = fs::read_to_string(&path)
        .with_context(|| format!("Failed to read config file: {}", path.as_ref().display()))?;

    let expanded = expand_env_vars(&yaml_str)?;

    let mut config: Config = serde_yaml_ng::from_str(&expanded)
        .context("Failed to parse YAML config after environment variable expansion")?;
    config.validate()?;
    config.setup_glob_set()?;
    config.parse_image_pull_secrets()?;

    info!(
        "Parsed valid application config:\n{}",
        serde_yaml_ng::to_string(&config)?
    );

    Ok(config)
}

/// Replaces `${VAR}` placeholders with environment variables values.
/// Returns an error if any env var is missing or regex fails.
fn expand_env_vars(input: &str) -> Result<String> {
    let regex =
        Regex::new(r"\$\{([^}]+)}").context("Invalid regex pattern for env var substitution")?;

    let result = regex.replace_all(input, |caps: &regex::Captures| {
        let var_name = &caps[1];
        env::var(var_name).unwrap_or_else(|_| panic!("Missing environment variable: {}", var_name))
    });

    Ok(result.into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[test]
    fn test_expand_env_vars_success() {
        unsafe {
            env::set_var("TEST_VAR", "value123");
        }
        let input = "This is a test: ${TEST_VAR}";
        let expanded = expand_env_vars(input).expect("Expansion should succeed");
        assert_eq!(expanded, "This is a test: value123");
        unsafe {
            env::remove_var("TEST_VAR");
        }
    }

    #[test]
    #[should_panic(expected = "Missing environment variable: MISSING_VAR")]
    fn test_expand_env_vars_missing_var() {
        let input = "This will fail: ${MISSING_VAR}";
        let _ = expand_env_vars(input).unwrap();
    }

    #[test]
    fn test_expand_env_vars_multiple_vars() {
        unsafe {
            env::set_var("VAR1", "foo");
            env::set_var("VAR2", "bar");
        }
        let input = "${VAR1} and ${VAR2}";
        let expanded = expand_env_vars(input).expect("Expansion should succeed");
        assert_eq!(expanded, "foo and bar");
        unsafe {
            env::remove_var("VAR1");
            env::remove_var("VAR2");
        }
    }

    #[test]
    fn test_expand_env_vars_no_vars() {
        let input = "No variables here";
        let expanded = expand_env_vars(input).expect("Expansion should succeed");
        assert_eq!(expanded, input);
    }

    #[test]
    fn test_load_config_file() {
        let image_pull_secret = r#"{"auths":{"your.private.registry.example.com":{"username":"janedoe","password":"xxxxxxxxxxx","email":"jdoe@example.com","auth":"c3R...zE2"}}}"#;
        let tmp_ips_dir = tempfile::tempdir().expect("Failed to create temp directory");
        let tmp_ips_path = tmp_ips_dir.path().join(".dockerconfigjson");

        fs::write(&tmp_ips_path, image_pull_secret).expect("Failed to write to temp file");

        let yaml_content = format!(
            r#"
        webserver:
          port: 8080
        registries:
          - hostnamePattern: "*.example.com"
            secret:
              type: Opaque
              username: user
              token: secret_token
          - hostnamePattern: "*.whatever.com"
            secret:
              type: ImagePullSecret
              mountPath: {}
        tls:
          ca_certificate_paths: []
        featureFlags:
          enableJfrogArtifactoryFallback: true
        "#,
            tmp_ips_dir.path().to_str().unwrap()
        );

        let tmp_config_file = tempfile::NamedTempFile::new().expect("Failed to create temp file");
        let tmp_config_path = tmp_config_file.path();
        fs::write(tmp_config_path, yaml_content).expect("Failed to write to temp file");

        let config = load_config(tmp_config_path).expect("Should load config");

        assert_eq!(config.webserver.port, 8080);
        assert_eq!(config.registries.len(), 2);

        match &config.registries[0].secret {
            RegistrySecret::Opaque { username, token } => {
                assert_eq!(username.as_deref(), Some("user"));
                assert_eq!(token.expose_secret(), "secret_token");
            }
            other => panic!("Expected Opaque secret, found: {:?}", other),
        }

        match &config.registries[1].secret {
            RegistrySecret::ImagePullSecret { docker_config, .. } => {
                assert_eq!(
                    docker_config.auths.iter().next().unwrap().1.username,
                    "janedoe"
                );
                assert_eq!(
                    docker_config
                        .auths
                        .iter()
                        .next()
                        .unwrap()
                        .1
                        .password
                        .expose_secret(),
                    "xxxxxxxxxxx"
                );
                assert_eq!(
                    docker_config
                        .auths
                        .iter()
                        .next()
                        .unwrap()
                        .1
                        .auth
                        .expose_secret(),
                    "c3R...zE2"
                );
            }
            other => panic!("Expected Opaque secret, found: {:?}", other),
        }
        assert_eq!(config.feature_flags.enable_jfrog_artifactory_fallback, true);
    }

    #[test]
    fn test_load_config_with_env_vars() {
        unsafe {
            env::set_var("PORT", "9090");
            env::set_var("TOKEN", "envtoken");
        }

        let yaml_content = r#"
        webserver:
          port: ${PORT}
        registries:
          - hostnamePattern: "*.env.com"
            secret:
              type: Opaque
              username: envuser
              token: ${TOKEN}
        tls:
          ca_certificate_paths: []
        featureFlags:
          enableJfrogArtifactoryFallback: false
        "#;

        let tmp_file = tempfile::NamedTempFile::new().expect("Failed to create temp file");
        let path = tmp_file.path();
        fs::write(path, yaml_content).expect("Failed to write to temp file");

        let config = load_config(path).expect("Should load config with env vars");

        assert_eq!(config.webserver.port, 9090);
        assert_eq!(config.registries.len(), 1);

        match &config.registries[0].secret {
            RegistrySecret::Opaque { username, token } => {
                assert_eq!(username.as_deref(), Some("envuser"));
                assert_eq!(token.expose_secret(), "envtoken");
            }
            other => panic!("Expected Opaque secret, found: {:?}", other),
        }

        unsafe {
            env::remove_var("PORT");
            env::remove_var("TOKEN");
        }
    }

    #[test]
    fn test_validate_invalid_pattern() {
        let config = Config {
            cron_schedule: String::new(),
            webserver: Webserver { port: 8080 },
            registries: vec![Registry {
                hostname_pattern: "[invalid".to_string(), // invalid glob pattern
                secret: RegistrySecret::Opaque {
                    username: None,
                    token: SecretString::new("token".to_string()),
                },
            }],
            tls: Tls {
                ca_certificate_paths: Vec::new(),
            },
            feature_flags: FeatureFlags {
                enable_jfrog_artifactory_fallback: false,
                enable_kubectl_annotation: false,
            },
            glob_set: GlobSet::empty(),
        };
        let result = config.validate();
        assert!(
            result.is_err(),
            "Expected validate to fail on invalid glob pattern"
        );
    }

    #[test]
    fn test_setup_glob_set_and_find_registry() {
        let mut config = Config {
            cron_schedule: String::new(),
            webserver: Webserver { port: 8080 },
            registries: vec![
                Registry {
                    hostname_pattern: "*.example.com".to_string(),
                    secret: RegistrySecret::Opaque {
                        username: Some("user1".to_string()),
                        token: SecretString::new("token1".to_string()),
                    },
                },
                Registry {
                    hostname_pattern: "registry.*.com".to_string(),
                    secret: RegistrySecret::Opaque {
                        username: Some("user2".to_string()),
                        token: SecretString::new("token2".to_string()),
                    },
                },
                Registry {
                    hostname_pattern: "registry-exact.com".to_string(),
                    secret: RegistrySecret::Opaque {
                        username: Some("user3".to_string()),
                        token: SecretString::new("token3".to_string()),
                    },
                },
            ],
            tls: Tls {
                ca_certificate_paths: Vec::new(),
            },
            feature_flags: FeatureFlags {
                enable_jfrog_artifactory_fallback: false,
                enable_kubectl_annotation: false,
            },
            glob_set: GlobSet::empty(),
        };

        config
            .setup_glob_set()
            .expect("setup_glob_set should succeed");

        // Matches first registry
        let reg = config.find_registry_for_hostname("test.example.com");
        assert!(reg.is_some());
        match &config.registries[0].secret {
            RegistrySecret::Opaque { username, token } => {
                assert_eq!(username.as_deref(), Some("user1"));
                assert_eq!(token.expose_secret(), "token1");
            }
            other => panic!("Expected Opaque secret, found: {:?}", other),
        }

        // Matches second registry
        let reg = config.find_registry_for_hostname("registry.foo.com");
        assert!(reg.is_some());
        match &config.registries[1].secret {
            RegistrySecret::Opaque { username, token } => {
                assert_eq!(username.as_deref(), Some("user2"));
                assert_eq!(token.expose_secret(), "token2");
            }
            other => panic!("Expected Opaque secret, found: {:?}", other),
        }

        // Matches third registry
        let reg = config.find_registry_for_hostname("registry-exact.com");
        assert!(reg.is_some());
        match &config.registries[2].secret {
            RegistrySecret::Opaque { username, token } => {
                assert_eq!(username.as_deref(), Some("user3"));
                assert_eq!(token.expose_secret(), "token3");
            }
            other => panic!("Expected Opaque secret, found: {:?}", other),
        }

        // No match
        let reg = config.find_registry_for_hostname("nomatch.com");
        assert!(reg.is_none());
    }
}
