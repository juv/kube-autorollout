use serde::{Deserialize, Serialize};
use std::fmt;

/// Wrapper for secret strings (e.g., tokens, passwords) that prints a "<REDACTED, length {length of the secret}>" string for Debug/Display
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct SecretString(String);

impl SecretString {
    pub fn new(s: String) -> Self {
        SecretString(s)
    }

    /// Access the raw secret if explicitly needed
    pub fn expose_secret(&self) -> &str {
        &self.0
    }

    fn fmt_redacted_secret(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "<REDACTED, length {}>", self.0.len())
    }
}

impl fmt::Debug for SecretString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.fmt_redacted_secret(f)
    }
}

impl fmt::Display for SecretString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.fmt_redacted_secret(f)
    }
}
