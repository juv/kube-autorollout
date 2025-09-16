use crate::secret_string::SecretString;
use anyhow::{Context, Result};
use regex::Regex;
use serde::Deserialize;
use std::{env, fs, path::Path};
use tracing::info;

#[derive(Debug, Deserialize)]
pub struct Config {
    pub webserver: Webserver,
    pub registries: Vec<Registry>,
    #[serde(default, rename = "enableJfrogArtifactoryFallback")]
    pub enable_jfrog_artifactory_fallback: bool,
}

#[derive(Debug, Deserialize)]
pub struct Registry {
    pub username: Option<String>,
    pub token: SecretString,
}

#[derive(Debug, Deserialize)]
pub struct Webserver {
    pub port: u16,
}

pub fn load_config<P: AsRef<Path>>(path: P) -> Result<Config> {
    info!("Loading config from file {}", path.as_ref().display());
    let yaml_str = fs::read_to_string(&path)
        .with_context(|| format!("Failed to read config file: {}", path.as_ref().display()))?;

    let expanded = expand_env_vars(&yaml_str)?;

    let config = serde_yaml_ng::from_str(&expanded)
        .context("Failed to parse YAML config after environment variable expansion")?;

    Ok(config)
}

/// Replaces `${VAR}` placeholders with environment variables values.
/// Returns an error if any env var is missing or regex fails.
fn expand_env_vars(input: &str) -> Result<String> {
    let re =
        Regex::new(r"\$\{([^}]+)}").context("Invalid regex pattern for env var substitution")?;

    let result = re.replace_all(input, |caps: &regex::Captures| {
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
          - username: user
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
}
