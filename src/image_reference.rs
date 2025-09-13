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
