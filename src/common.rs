use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::fmt;
use std::str::FromStr;
use thiserror::Error;

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct RepoIdentifier {
    owner: String,
    repo: RepoMatcher,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub enum RepoMatcher {
    Specific(String),
    Wildcard,
}

impl<T: Into<String> + AsRef<str>> From<T> for RepoMatcher {
    fn from(data: T) -> Self {
        match data.as_ref() {
            "*" => RepoMatcher::Wildcard,
            data => RepoMatcher::Specific(data.into()),
        }
    }
}

impl fmt::Display for RepoIdentifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        write!(f, "{}/{}", self.owner, self.repo)
    }
}

impl fmt::Display for RepoMatcher {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        match self {
            RepoMatcher::Specific(repo) => write!(f, "{}", repo),
            RepoMatcher::Wildcard => write!(f, "*"),
        }
    }
}

impl RepoIdentifier {
    fn new<T>(owner: T, repo: RepoMatcher) -> Self
    where
        T: Into<String>,
    {
        Self {
            owner: owner.into(),
            repo,
        }
    }
}

impl FromStr for RepoIdentifier {
    type Err = MalformedRepoNameError;

    fn from_str(s: &str) -> Result<Self, MalformedRepoNameError> {
        let mut chunks = s.split('/');
        let owner = chunks.next();
        let repo = chunks.next();
        if chunks.next().is_some() {
            return Err(MalformedRepoNameError("too many slashes"));
        }
        if owner.is_none() || repo.is_none() {
            return Err(MalformedRepoNameError("too few slashes"));
        }
        let owner = owner.unwrap();
        let repo = repo.unwrap();
        if owner.is_empty() || repo.is_empty() {
            return Err(MalformedRepoNameError("empty owner/repo name"));
        }
        if owner == "*" {
            return Err(MalformedRepoNameError("owner cannot be wildcard"));
        }
        Ok(Self::new(owner.to_string(), repo.to_string().into()))
    }
}

#[derive(Error, Debug)]
#[error("malformed repo name: {0}")]
pub struct MalformedRepoNameError(&'static str);

#[derive(Debug, Clone)]
pub struct RepoMap<T> {
    default: T,
    entries: HashMap<RepoIdentifier, T>,
}

impl<T> RepoMap<T> {
    pub fn new(default: T) -> Self {
        Self {
            default,
            entries: HashMap::new(),
        }
    }

    pub fn insert(&mut self, repo: RepoIdentifier, value: T) -> Result<(), RepoMapError> {
        match self.entries.entry(repo) {
            Entry::Vacant(e) => {
                e.insert(value);
                Ok(())
            }
            Entry::Occupied(e) => Err(RepoMapError::DuplicateRepo(e.key().clone())),
        }
    }

    pub fn get(&self, owner: &str, repo: &str) -> &T {
        let repo = RepoIdentifier::new(owner, RepoMatcher::Specific(repo.into()));
        if let Some(value) = self.entries.get(&repo) {
            return value;
        }
        let repo = RepoIdentifier::new(repo.owner, RepoMatcher::Wildcard);
        self.entries.get(&repo).unwrap_or(&self.default)
    }
}

impl<T> Default for RepoMap<T>
where
    T: Default,
{
    fn default() -> Self {
        Self::new(T::default())
    }
}

#[derive(Error, Debug)]
pub enum RepoMapError {
    #[error("duplicate repo entry: {0}")]
    DuplicateRepo(RepoIdentifier),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_repo_id_from_str() {
        assert_eq!(
            "owner/repo".parse::<RepoIdentifier>().unwrap(),
            RepoIdentifier::new("owner", RepoMatcher::Specific("repo".into()))
        );
        assert_eq!(
            "owner/*".parse::<RepoIdentifier>().unwrap(),
            RepoIdentifier::new("owner", RepoMatcher::Wildcard)
        );
        assert!("owner/repo/".parse::<RepoIdentifier>().is_err());
        assert!("owner".parse::<RepoIdentifier>().is_err());
        assert!("owner/".parse::<RepoIdentifier>().is_err());
        assert!("/repo".parse::<RepoIdentifier>().is_err());
        assert!("/".parse::<RepoIdentifier>().is_err());
        assert!("*/something".parse::<RepoIdentifier>().is_err());
    }

    #[test]
    fn test_repo_map() {
        let mut repo_map = RepoMap::default();
        repo_map
            .insert(
                RepoIdentifier::new("owner", RepoMatcher::Specific("repo".into())),
                1,
            )
            .unwrap();
        repo_map
            .insert(RepoIdentifier::new("other", RepoMatcher::Wildcard), 2)
            .unwrap();
        repo_map
            .insert(
                RepoIdentifier::new("other", RepoMatcher::Specific("override".into())),
                3,
            )
            .unwrap();

        assert_eq!(repo_map.get("owner", "repo"), &1);
        assert_eq!(repo_map.get("owner", "foo"), &0);
        assert_eq!(repo_map.get("other", "potato"), &2);
        assert_eq!(repo_map.get("other", "override"), &3);
        assert_eq!(repo_map.get("unrelated", "bar"), &0);
    }
}
