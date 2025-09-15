use std::fmt;

#[derive(Debug, PartialEq, Eq)]
pub struct ImageReference {
    pub registry: String,
    pub repository: String,
    pub tag: String,
}

#[derive(Debug)]
pub enum ParseError {
    MissingRegistry,
    MissingRepository,
    MissingTag,
    InvalidFormat(String),
    DigestNotAllowed,
}

impl std::error::Error for ParseError {}
impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseError::DigestNotAllowed => write!(f, "digest references are not allowed"),
            ParseError::MissingRegistry => write!(f, "registry is missing"),
            ParseError::MissingRepository => write!(f, "repository is missing"),
            ParseError::MissingTag => write!(f, "tag is missing"),
            ParseError::InvalidFormat(image) => write!(f, "invalid image format: {}", image),
        }
    }
}

impl fmt::Display for ImageReference {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}:{}", self.registry, self.repository, self.tag)
    }
}

impl ImageReference {
    pub fn parse(s: &str) -> Result<Self, ParseError> {
        // digest references are not supported
        if s.contains('@') {
            return Err(ParseError::DigestNotAllowed);
        }

        // Must contain a tag (colon after last slash)
        let (without_tag, tag) = if let Some(pos) = s.rfind(':') {
            let last_slash = s.rfind('/').unwrap_or(0);
            if pos > last_slash {
                (&s[..pos], Some(s[pos + 1..].to_string()))
            } else {
                (s, None)
            }
        } else {
            (s, None)
        };
        let tag = tag.ok_or(ParseError::MissingTag)?;

        // Split into registry and repository by the first slash
        let parts: Vec<&str> = without_tag.splitn(2, '/').collect();
        if parts.len() != 2 {
            return Err(ParseError::InvalidFormat(s.to_string()));
        }

        let registry = parts[0];
        let repository = parts[1];

        if registry.is_empty() {
            return Err(ParseError::MissingRegistry);
        }
        if repository.is_empty() {
            return Err(ParseError::MissingRepository);
        }

        Ok(Self {
            registry: registry.to_string(),
            repository: repository.to_string(),
            tag,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_valid_image_reference() {
        let input = "myregistry.example.com/myrepo/myimage:v1.0.0";
        let result = ImageReference::parse(input);
        assert!(result.is_ok());
        let image_ref = result.unwrap();
        assert_eq!(image_ref.registry, "myregistry.example.com");
        assert_eq!(image_ref.repository, "myrepo/myimage");
        assert_eq!(image_ref.tag, "v1.0.0");
        // Check Display implementation
        assert_eq!(image_ref.to_string(), input);
    }

    #[test]
    fn parse_valid_image_reference_single_level_repo() {
        let input = "registry/repo:latest";
        let result = ImageReference::parse(input).unwrap();
        assert_eq!(result.registry, "registry");
        assert_eq!(result.repository, "repo");
        assert_eq!(result.tag, "latest");
        assert_eq!(result.to_string(), input);
    }

    #[test]
    fn parse_error_digest_not_allowed() {
        let input = "registry/repo@sha256:123abc";
        let err = ImageReference::parse(input).unwrap_err();
        match err {
            ParseError::DigestNotAllowed => {}
            _ => panic!("Expected DigestNotAllowed error"),
        }
    }

    #[test]
    fn parse_error_missing_tag() {
        let input = "registry/repo";
        let err = ImageReference::parse(input).unwrap_err();
        match err {
            ParseError::MissingTag => {}
            _ => panic!("Expected MissingTag error"),
        }
    }

    #[test]
    fn parse_error_invalid_format() {
        // No slash after registry means invalid format
        let input = "registryrepo:tag";
        let err = ImageReference::parse(input).unwrap_err();
        match err {
            ParseError::InvalidFormat(s) => assert_eq!(s, input),
            _ => panic!("Expected InvalidFormat error"),
        }
    }

    #[test]
    fn parse_error_missing_registry() {
        // Leading slash means empty registry part
        let input = "/repo:tag";
        let err = ImageReference::parse(input).unwrap_err();
        match err {
            ParseError::MissingRegistry => {}
            _ => panic!("Expected MissingRegistry error"),
        }
    }

    #[test]
    fn parse_error_missing_repository() {
        // Trailing slash after registry
        let input = "registry/:tag";
        let err = ImageReference::parse(input).unwrap_err();
        match err {
            ParseError::MissingRepository => {}
            _ => panic!("Expected MissingRepository error"),
        }
    }
}
