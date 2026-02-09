use std::collections::BTreeMap;

use anyhow::{Result, anyhow};
use plugin_host::EntryType;
use semver::Version;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceRef {
    pub scheme: String,
    pub locator: String,
}

impl SourceRef {
    #[must_use]
    pub fn builtin() -> Self {
        Self {
            scheme: "builtin".to_string(),
            locator: "dark-forest".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameListing {
    pub id: String,
    pub name: String,
    pub description: String,
    pub tags: Vec<String>,
    pub author: String,
    pub source: SourceRef,
    pub permissions_summary: Vec<String>,
    pub host_api_range: String,
    pub entry_type: EntryType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactRef {
    pub id: String,
    pub version: String,
    pub source: SourceRef,
    pub entry_type: EntryType,
}

pub trait RegistryProvider: Send + Sync {
    fn list(&self) -> Result<Vec<GameListing>>;
    fn resolve(&self, id: &str, version: Option<&str>) -> Result<ArtifactRef>;
}

#[derive(Debug, Clone, Default)]
pub struct BuiltinRegistry {
    listings: Vec<GameListing>,
    versions: BTreeMap<String, Version>,
}

impl BuiltinRegistry {
    #[must_use]
    pub fn new(listings: Vec<GameListing>) -> Self {
        let mut versions = BTreeMap::new();
        for listing in &listings {
            versions.insert(listing.id.clone(), Version::new(0, 1, 0));
        }

        Self { listings, versions }
    }
}

impl RegistryProvider for BuiltinRegistry {
    fn list(&self) -> Result<Vec<GameListing>> {
        Ok(self.listings.clone())
    }

    fn resolve(&self, id: &str, version: Option<&str>) -> Result<ArtifactRef> {
        let listing = self
            .listings
            .iter()
            .find(|item| item.id == id)
            .ok_or_else(|| anyhow!("unknown game id: {id}"))?;

        let resolved = version
            .map(ToString::to_string)
            .or_else(|| self.versions.get(id).map(ToString::to_string))
            .unwrap_or_else(|| "0.1.0".to_string());

        Ok(ArtifactRef {
            id: id.to_string(),
            version: resolved,
            source: listing.source.clone(),
            entry_type: listing.entry_type,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub author: String,
    pub entry_type: EntryType,
    pub entry: String,
    pub host_api: String,
    pub permissions: Vec<String>,
}

pub fn parse_manifest(input: &str) -> Result<Manifest> {
    let parsed: Manifest = serde_json::from_str(input)?;
    if parsed.id.trim().is_empty() {
        return Err(anyhow!("manifest id cannot be empty"));
    }
    if parsed.name.trim().is_empty() {
        return Err(anyhow!("manifest name cannot be empty"));
    }
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::{EntryType, parse_manifest};
    use anyhow::Result;
    use proptest::prelude::*;

    #[test]
    fn manifest_parser_accepts_valid_payload() -> Result<()> {
        let payload = r#"{
            "id":"snake",
            "name":"Snake+",
            "version":"0.1.0",
            "author":"Dark Forest",
            "entry_type":"native",
            "entry":"builtin:snake",
            "host_api":"^0.1",
            "permissions":[]
        }"#;

        let manifest = parse_manifest(payload)?;
        assert_eq!(manifest.id, "snake");
        assert_eq!(manifest.entry_type, EntryType::Native);
        Ok(())
    }

    proptest! {
        #[test]
        fn parser_rejects_empty_id(name in ".{1,20}") {
            let payload = format!(
                r#"{{"id":"","name":"{name}","version":"0.1.0","author":"a","entry_type":"native","entry":"x","host_api":"^0.1","permissions":[]}}"#
            );
            prop_assert!(parse_manifest(&payload).is_err());
        }
    }
}
