use crate::secret_string::SecretString;
use anyhow::{Context, Result};
use globset::{Glob, GlobSet};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::{env, fs, path::Path};
use tracing::info;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Config {
    pub webserver: Webserver,
    pub registries: Vec<Registry>,
    #[serde(default, rename = "enableJfrogArtifactoryFallback")]
    pub enable_jfrog_artifactory_fallback: bool,
    #[serde(skip)]
    glob_set: GlobSet,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Registry {
    #[serde(default, rename = "hostnamePattern")]
    pub hostname_pattern: String,
    pub username: Option<String>,
    pub token: SecretString,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Webserver {
    pub port: u16,
}

impl Config {
    pub fn validate(&self) -> Result<()> {
        for registry in &self.registries {
            Glob::new(&registry.hostname_pattern).context(format!(
                "invalid hostname pattern {}",
                registry.hostname_pattern
            ))?;
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
        let yaml_content = r#"
        webserver:
          port: 8080
        registries:
          - hostnamePattern: "*.example.com"
            username: user
            token: secret_token
        enableJfrogArtifactoryFallback: true
        "#;

        let tmp_file = tempfile::NamedTempFile::new().expect("Failed to create temp file");
        let path = tmp_file.path();
        fs::write(path, yaml_content).expect("Failed to write to temp file");

        let config = load_config(path).expect("Should load config");

        assert_eq!(config.webserver.port, 8080);
        assert_eq!(config.registries.len(), 1);
        assert_eq!(config.registries[0].username.as_deref(), Some("user"));
        assert_eq!(config.registries[0].token.expose_secret(), "secret_token");
        assert_eq!(config.enable_jfrog_artifactory_fallback, true);
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
            username: envuser
            token: ${TOKEN}
        enableJfrogArtifactoryFallback: false
        "#;

        let tmp_file = tempfile::NamedTempFile::new().expect("Failed to create temp file");
        let path = tmp_file.path();
        fs::write(path, yaml_content).expect("Failed to write to temp file");

        let config = load_config(path).expect("Should load config with env vars");

        assert_eq!(config.webserver.port, 9090);
        assert_eq!(config.registries.len(), 1);
        assert_eq!(config.registries[0].username.as_deref(), Some("envuser"));
        assert_eq!(config.registries[0].token.expose_secret(), "envtoken");

        unsafe {
            env::remove_var("PORT");
            env::remove_var("TOKEN");
        }
    }

    #[test]
    fn test_validate_invalid_pattern() {
        let config = Config {
            webserver: Webserver { port: 8080 },
            registries: vec![Registry {
                hostname_pattern: "[invalid".to_string(), // invalid glob pattern
                username: None,
                token: SecretString::new("token".to_string()),
            }],
            enable_jfrog_artifactory_fallback: false,
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
            webserver: Webserver { port: 8080 },
            registries: vec![
                Registry {
                    hostname_pattern: "*.example.com".to_string(),
                    username: Some("user1".to_string()),
                    token: SecretString::new("token1".to_string()),
                },
                Registry {
                    hostname_pattern: "registry.*.com".to_string(),
                    username: Some("user2".to_string()),
                    token: SecretString::new("token2".to_string()),
                },
                Registry {
                    hostname_pattern: "registry-exact.com".to_string(),
                    username: Some("user3".to_string()),
                    token: SecretString::new("token3".to_string()),
                },
            ],
            enable_jfrog_artifactory_fallback: false,
            glob_set: GlobSet::empty(),
        };

        config
            .setup_glob_set()
            .expect("setup_glob_set should succeed");

        // Matches first registry
        let reg = config.find_registry_for_hostname("test.example.com");
        assert!(reg.is_some());
        assert_eq!(reg.unwrap().username.as_deref(), Some("user1"));

        // Matches second registry
        let reg = config.find_registry_for_hostname("registry.foo.com");
        assert!(reg.is_some());
        assert_eq!(reg.unwrap().username.as_deref(), Some("user2"));

        // Matches third registry
        let reg = config.find_registry_for_hostname("registry-exact.com");
        assert!(reg.is_some());
        assert_eq!(reg.unwrap().username.as_deref(), Some("user3"));

        // No match
        let reg = config.find_registry_for_hostname("nomatch.com");
        assert!(reg.is_none());
    }
}
