use serde::{Deserialize, Serializer};
use std::fmt;

/// Wrapper for secret strings (e.g., tokens, passwords) that prints a "<REDACTED, length {length of the secret}>" string for Debug/Display/Serialize
#[derive(Deserialize, Clone, PartialEq, Eq)]
pub struct SecretString(String);

impl SecretString {
    pub fn new(s: String) -> Self {
        SecretString(s)
    }

    pub fn expose_secret(&self) -> &str {
        &self.0
    }

    fn get_redacted_secret(&self) -> String {
        format!("<REDACTED, length {}>", self.0.len())
    }
}

impl fmt::Debug for SecretString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.get_redacted_secret().as_str())
    }
}

impl fmt::Display for SecretString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.get_redacted_secret().as_str())
    }
}

impl serde::Serialize for SecretString {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.get_redacted_secret().as_str())
    }
}
