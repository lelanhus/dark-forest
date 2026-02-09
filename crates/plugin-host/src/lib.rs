use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EntryType {
    Native,
    Wasm,
    Process,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    FsRead,
    FsWrite,
    Net,
    OpenUrl,
    Clipboard,
    Clock,
    Random,
    TerminalRawInput,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Scope {
    None,
    Path(String),
    Paths(Vec<String>),
    Allowlist(Vec<String>),
    Prompt,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Decision {
    Allow,
    Deny,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityGrant {
    pub capability: Capability,
    pub scope: Scope,
    pub decision: Decision,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub author: String,
    pub entry_type: EntryType,
    pub entry: String,
    pub host_api: String,
    pub permissions: Vec<CapabilityGrant>,
}

pub fn validate_entry_type(
    entry_type: EntryType,
    source_scheme: &str,
    trusted_native: bool,
) -> Result<()> {
    if entry_type == EntryType::Native && !trusted_native && source_scheme != "builtin" {
        bail!("third-party native entry_type is not allowed by default policy");
    }

    if entry_type == EntryType::Process && source_scheme != "builtin" {
        bail!("process entry_type is disabled by default");
    }

    Ok(())
}
