use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use flate2::read::GzDecoder;
use plugin_host::{Capability, CapabilityGrant, Decision, EntryType, Scope};
use semver::Version;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tar::Archive;

const HTTP_MAX_ATTEMPTS: usize = 3;
const HTTP_RETRY_BACKOFF_BASE_MS: u64 = 40;
const HTTP_CONNECT_TIMEOUT_MS: u64 = 800;
const HTTP_READ_TIMEOUT_MS: u64 = 1_500;

#[derive(Debug, Clone)]
enum HttpAttemptError {
    Status(u16),
    Transport(String),
    Read(String),
}

#[cfg(test)]
#[derive(Debug, Clone)]
enum MockHttpStep {
    Bytes(Vec<u8>),
    Status(u16),
    Transport(String),
}

#[cfg(test)]
static HTTP_MOCKS: std::sync::LazyLock<
    std::sync::Mutex<std::collections::BTreeMap<String, Vec<MockHttpStep>>>,
> = std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::BTreeMap::new()));

#[cfg(test)]
fn register_http_mock(locator: &str, steps: Vec<MockHttpStep>) {
    if let Ok(mut mocks) = HTTP_MOCKS.lock() {
        mocks.insert(locator.to_string(), steps);
    }
}

#[cfg(test)]
fn pop_http_mock_step(locator: &str) -> Option<MockHttpStep> {
    let Ok(mut mocks) = HTTP_MOCKS.lock() else {
        return None;
    };
    let entry = mocks.get_mut(locator)?;
    if entry.is_empty() {
        mocks.remove(locator);
        return None;
    }
    let step = entry.remove(0);
    if entry.is_empty() {
        mocks.remove(locator);
    }
    Some(step)
}

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
    #[serde(default)]
    pub verified: bool,
    #[serde(default)]
    pub publisher_id: Option<String>,
    #[serde(default)]
    pub collections: Vec<String>,
    #[serde(default)]
    pub compatibility: Option<CompatibilityBadge>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactRef {
    pub id: String,
    pub version: String,
    pub source: SourceRef,
    pub entry_type: EntryType,
    #[serde(default)]
    pub artifact_uri: Option<String>,
    #[serde(default)]
    pub artifact_sha256: Option<String>,
    #[serde(default)]
    pub signature_uri: Option<String>,
    #[serde(default)]
    pub publisher_id: Option<String>,
    #[serde(default)]
    pub signature_fingerprint: Option<String>,
    #[serde(default)]
    pub size_bytes: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CompatibilityBadge {
    pub host_api: String,
    pub permissions: String,
}

pub trait RegistryProvider: Send + Sync {
    fn list(&self) -> Result<Vec<GameListing>>;
    fn resolve(&self, id: &str, version: Option<&str>) -> Result<ArtifactRef>;
}

pub enum AnyRegistryProvider {
    Index(IndexRegistryProvider),
    Github(GithubRegistryProvider),
}

impl RegistryProvider for AnyRegistryProvider {
    fn list(&self) -> Result<Vec<GameListing>> {
        match self {
            Self::Index(provider) => provider.list(),
            Self::Github(provider) => provider.list(),
        }
    }

    fn resolve(&self, id: &str, version: Option<&str>) -> Result<ArtifactRef> {
        match self {
            Self::Index(provider) => provider.resolve(id, version),
            Self::Github(provider) => provider.resolve(id, version),
        }
    }
}

pub fn provider_from_locator(locator: &str, cache_root: PathBuf) -> Result<AnyRegistryProvider> {
    let trimmed = locator.trim();
    if trimmed.starts_with("github://") {
        return Ok(AnyRegistryProvider::Github(GithubRegistryProvider::new(
            trimmed.to_string(),
            cache_root,
        )?));
    }

    let normalized = if let Some(rest) = trimmed.strip_prefix("index://") {
        rest.to_string()
    } else {
        trimmed.to_string()
    };

    Ok(AnyRegistryProvider::Index(IndexRegistryProvider::new(
        normalized, cache_root,
    )))
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
            artifact_uri: None,
            artifact_sha256: None,
            signature_uri: None,
            publisher_id: None,
            signature_fingerprint: None,
            size_bytes: None,
        })
    }
}

#[derive(Debug, Clone)]
pub struct IndexRegistryProvider {
    locator: String,
    cache_root: PathBuf,
}

impl IndexRegistryProvider {
    #[must_use]
    pub fn new(locator: String, cache_root: PathBuf) -> Self {
        Self {
            locator,
            cache_root,
        }
    }

    pub fn fetch_artifact_to_cache(&self, artifact: &ArtifactRef) -> Result<PathBuf> {
        let uri = artifact
            .artifact_uri
            .as_ref()
            .ok_or_else(|| anyhow!("artifact URI missing for {}", artifact.id))?;

        let bytes = fetch_bytes_from_locator(uri)
            .with_context(|| format!("failed to fetch artifact from {uri}"))?;

        let actual_sha = to_hex_lower(&Sha256::digest(&bytes));
        if let Some(expected) = &artifact.artifact_sha256
            && !expected.eq_ignore_ascii_case(&actual_sha)
        {
            return Err(anyhow!(
                "artifact checksum mismatch (expected {expected}, actual {actual_sha})"
            ));
        }

        let target_dir = self.cache_root.join("artifacts");
        fs::create_dir_all(&target_dir)?;
        let target_path = target_dir.join(format!("{}-{}.tar.gz", artifact.id, artifact.version));
        atomic_write_bytes(&target_path, &bytes)?;
        Ok(target_path)
    }

    fn cache_index_path(&self) -> PathBuf {
        let digest = Sha256::digest(self.locator.as_bytes());
        let key = to_hex_lower(&digest);
        self.cache_root.join("index").join(format!("{key}.json"))
    }

    fn load_catalog(&self) -> Result<IndexCatalog> {
        match fetch_text_from_locator(&self.locator) {
            Ok(raw) => {
                let parsed: IndexCatalog = serde_json::from_str(&raw).with_context(|| {
                    format!("failed to parse index catalog from {}", self.locator)
                })?;
                self.cache_catalog(&raw)?;
                Ok(parsed)
            }
            Err(fetch_err) => {
                let cached = self.load_cached_catalog().with_context(|| {
                    format!(
                        "failed to fetch index from {} and no valid cache available",
                        self.locator
                    )
                })?;
                tracing::warn!(
                    locator = %self.locator,
                    error = %fetch_err,
                    "using cached index catalog"
                );
                Ok(cached)
            }
        }
    }

    fn cache_catalog(&self, raw: &str) -> Result<()> {
        let path = self.cache_index_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        atomic_write_bytes(&path, raw.as_bytes())
    }

    fn load_cached_catalog(&self) -> Result<IndexCatalog> {
        let path = self.cache_index_path();
        let raw = fs::read_to_string(&path)
            .with_context(|| format!("failed to read cached index at {}", path.display()))?;
        let parsed: IndexCatalog = serde_json::from_str(&raw)
            .with_context(|| format!("failed to parse cached index at {}", path.display()))?;
        Ok(parsed)
    }

    fn source_ref(&self) -> SourceRef {
        SourceRef {
            scheme: "index".to_string(),
            locator: self.locator.clone(),
        }
    }
}

impl RegistryProvider for IndexRegistryProvider {
    fn list(&self) -> Result<Vec<GameListing>> {
        let catalog = self.load_catalog()?;
        let source = self.source_ref();

        let mut listings = Vec::new();
        for game in catalog.games {
            let selected = select_version(&game, None)?.clone();
            validate_index_version_security_fields(&selected)?;
            let entry_type = selected
                .entry_type
                .or(game.entry_type)
                .unwrap_or(EntryType::Wasm);

            let permissions_summary = if game.permissions_summary.is_empty() {
                summarize_permissions(&selected.permissions)?
            } else {
                game.permissions_summary
            };

            let host_api_range = if game.host_api_range.trim().is_empty() {
                selected.host_api.unwrap_or_else(|| "^0.1".to_string())
            } else {
                game.host_api_range
            };

            listings.push(GameListing {
                id: game.id,
                name: game.name,
                description: game.description,
                tags: game.tags,
                author: game.author,
                source: source.clone(),
                permissions_summary,
                host_api_range,
                entry_type,
                verified: game.verified,
                publisher_id: game.publisher_id,
                collections: game.collections,
                compatibility: game.compatibility,
            });
        }

        Ok(listings)
    }

    fn resolve(&self, id: &str, version: Option<&str>) -> Result<ArtifactRef> {
        let catalog = self.load_catalog()?;
        let game = catalog
            .games
            .iter()
            .find(|item| item.id == id)
            .ok_or_else(|| anyhow!("unknown game id: {id}"))?;

        let selected = select_version(game, version)?;
        validate_index_version_security_fields(selected)?;
        let entry_type = selected
            .entry_type
            .or(game.entry_type)
            .unwrap_or(EntryType::Wasm);
        let artifact_uri = resolve_locator_relative(&self.locator, &selected.artifact);
        let signature_uri = selected
            .signature_uri
            .as_ref()
            .map(|value| resolve_locator_relative(&self.locator, value));
        let publisher_id = selected
            .publisher_id
            .clone()
            .or_else(|| game.publisher_id.clone());

        Ok(ArtifactRef {
            id: id.to_string(),
            version: selected.version.clone(),
            source: self.source_ref(),
            entry_type,
            artifact_uri: Some(artifact_uri),
            artifact_sha256: selected.artifact_sha256.clone(),
            signature_uri,
            publisher_id,
            signature_fingerprint: selected.signature_fingerprint.clone(),
            size_bytes: selected.size_bytes,
        })
    }
}

#[derive(Debug, Clone)]
pub struct GithubRegistryProvider {
    locator: String,
    cache_root: PathBuf,
    owner: String,
    repo: String,
    tag: String,
}

impl GithubRegistryProvider {
    pub fn new(locator: String, cache_root: PathBuf) -> Result<Self> {
        let (owner, repo, tag) = parse_github_locator(&locator)?;
        Ok(Self {
            locator,
            cache_root,
            owner,
            repo,
            tag,
        })
    }

    fn release_api_url(&self) -> String {
        format!(
            "https://api.github.com/repos/{}/{}/releases/tags/{}",
            self.owner, self.repo, self.tag
        )
    }

    fn cache_catalog_path(&self) -> PathBuf {
        let digest = Sha256::digest(self.locator.as_bytes());
        let key = to_hex_lower(&digest);
        self.cache_root.join("github").join(format!("{key}.json"))
    }

    fn cache_catalog(&self, cached: &CachedGithubCatalog) -> Result<()> {
        let path = self.cache_catalog_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let bytes = serde_json::to_vec_pretty(cached)?;
        atomic_write_bytes(&path, &bytes)
    }

    fn load_cached_catalog(&self) -> Result<(IndexCatalog, String)> {
        let path = self.cache_catalog_path();
        let raw = fs::read_to_string(&path)
            .with_context(|| format!("failed to read cached github catalog {}", path.display()))?;
        let cached: CachedGithubCatalog = serde_json::from_str(&raw)
            .with_context(|| format!("failed to parse cached github catalog {}", path.display()))?;
        let catalog: IndexCatalog = serde_json::from_str(&cached.catalog_raw)
            .context("failed to parse cached github index catalog payload")?;
        Ok((catalog, cached.index_asset_url))
    }

    fn load_catalog(&self) -> Result<(IndexCatalog, String)> {
        let release_url = self.release_api_url();
        let fetched = (|| {
            let release_raw = fetch_text_from_locator(&release_url)?;
            let release: GithubRelease = serde_json::from_str(&release_raw).with_context(|| {
                format!("failed to parse github release metadata from {release_url}")
            })?;
            let index_asset_url = release
                .assets
                .iter()
                .find(|asset| asset.name == "index.json")
                .map(|asset| asset.browser_download_url.clone())
                .ok_or_else(|| anyhow!("github release does not contain index.json asset"))?;
            let catalog_raw = fetch_text_from_locator(&index_asset_url)?;
            let catalog: IndexCatalog = serde_json::from_str(&catalog_raw)
                .with_context(|| format!("failed to parse index catalog from {index_asset_url}"))?;
            self.cache_catalog(&CachedGithubCatalog {
                index_asset_url: index_asset_url.clone(),
                catalog_raw,
            })?;
            Ok::<(IndexCatalog, String), anyhow::Error>((catalog, index_asset_url))
        })();

        match fetched {
            Ok(ok) => Ok(ok),
            Err(fetch_err) => {
                let cached = self.load_cached_catalog().with_context(|| {
                    format!(
                        "failed to fetch github catalog {} and no valid cache available",
                        self.locator
                    )
                })?;
                tracing::warn!(
                    locator = %self.locator,
                    error = %fetch_err,
                    "using cached github catalog"
                );
                Ok(cached)
            }
        }
    }

    fn source_ref(&self) -> SourceRef {
        SourceRef {
            scheme: "github".to_string(),
            locator: self.locator.clone(),
        }
    }
}

impl RegistryProvider for GithubRegistryProvider {
    fn list(&self) -> Result<Vec<GameListing>> {
        let (catalog, _index_asset_url) = self.load_catalog()?;
        let source = self.source_ref();

        let mut listings = Vec::new();
        for game in catalog.games {
            let selected = select_version(&game, None)?.clone();
            validate_index_version_security_fields(&selected)?;
            let entry_type = selected
                .entry_type
                .or(game.entry_type)
                .unwrap_or(EntryType::Wasm);

            let permissions_summary = if game.permissions_summary.is_empty() {
                summarize_permissions(&selected.permissions)?
            } else {
                game.permissions_summary
            };

            let host_api_range = if game.host_api_range.trim().is_empty() {
                selected.host_api.unwrap_or_else(|| "^0.1".to_string())
            } else {
                game.host_api_range
            };

            listings.push(GameListing {
                id: game.id,
                name: game.name,
                description: game.description,
                tags: game.tags,
                author: game.author,
                source: source.clone(),
                permissions_summary,
                host_api_range,
                entry_type,
                verified: game.verified,
                publisher_id: game.publisher_id,
                collections: game.collections,
                compatibility: game.compatibility,
            });
        }

        Ok(listings)
    }

    fn resolve(&self, id: &str, version: Option<&str>) -> Result<ArtifactRef> {
        let (catalog, index_asset_url) = self.load_catalog()?;
        let game = catalog
            .games
            .iter()
            .find(|item| item.id == id)
            .ok_or_else(|| anyhow!("unknown game id: {id}"))?;

        let selected = select_version(game, version)?;
        validate_index_version_security_fields(selected)?;
        let entry_type = selected
            .entry_type
            .or(game.entry_type)
            .unwrap_or(EntryType::Wasm);
        let artifact_uri = resolve_locator_relative(&index_asset_url, &selected.artifact);
        let signature_uri = selected
            .signature_uri
            .as_ref()
            .map(|value| resolve_locator_relative(&index_asset_url, value));
        let publisher_id = selected
            .publisher_id
            .clone()
            .or_else(|| game.publisher_id.clone());

        Ok(ArtifactRef {
            id: id.to_string(),
            version: selected.version.clone(),
            source: self.source_ref(),
            entry_type,
            artifact_uri: Some(artifact_uri),
            artifact_sha256: selected.artifact_sha256.clone(),
            signature_uri,
            publisher_id,
            signature_fingerprint: selected.signature_fingerprint.clone(),
            size_bytes: selected.size_bytes,
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
    pub permissions: Vec<CapabilityGrant>,
    pub controls: Option<String>,
    pub changelog_url: Option<String>,
    pub homepage: Option<String>,
    pub license: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawManifest {
    id: String,
    name: String,
    version: String,
    author: String,
    entry_type: EntryType,
    entry: String,
    host_api: String,
    #[serde(default)]
    permissions: Vec<serde_json::Value>,
    #[serde(default)]
    controls: Option<String>,
    #[serde(default)]
    changelog_url: Option<String>,
    #[serde(default)]
    homepage: Option<String>,
    #[serde(default)]
    license: Option<String>,
}

pub fn parse_manifest(input: &str) -> Result<Manifest> {
    let parsed: RawManifest = serde_json::from_str(input)?;
    if parsed.id.trim().is_empty() {
        return Err(anyhow!("manifest id cannot be empty"));
    }
    if parsed.name.trim().is_empty() {
        return Err(anyhow!("manifest name cannot be empty"));
    }

    let mut permissions = Vec::new();
    for item in parsed.permissions {
        permissions.push(normalize_permission(item)?);
    }

    Ok(Manifest {
        id: parsed.id,
        name: parsed.name,
        version: parsed.version,
        author: parsed.author,
        entry_type: parsed.entry_type,
        entry: parsed.entry,
        host_api: parsed.host_api,
        permissions,
        controls: parsed.controls,
        changelog_url: parsed.changelog_url,
        homepage: parsed.homepage,
        license: parsed.license,
    })
}

#[derive(Debug, Clone, Deserialize)]
struct RawPermissionObject {
    capability: String,
    #[serde(default)]
    scope: Option<serde_json::Value>,
    #[serde(default)]
    decision: Option<String>,
}

fn normalize_permission(value: serde_json::Value) -> Result<CapabilityGrant> {
    match value {
        serde_json::Value::String(label) => normalize_legacy_permission(&label),
        serde_json::Value::Object(_) => {
            let parsed: RawPermissionObject =
                serde_json::from_value(value).context("failed to parse permission object")?;
            let capability = parse_capability_label(&parsed.capability)?;
            let scope = match parsed.scope {
                Some(raw_scope) => parse_scope_value(capability, &raw_scope)?,
                None => default_scope(capability),
            };
            let decision = match parsed.decision {
                Some(raw) => parse_decision_label(&raw)?,
                None => Decision::Allow,
            };
            Ok(CapabilityGrant {
                capability,
                scope,
                decision,
            })
        }
        _ => Err(anyhow!("permission must be string or object")),
    }
}

fn normalize_legacy_permission(label: &str) -> Result<CapabilityGrant> {
    let mut parts = label.splitn(2, ':');
    let capability_label = parts.next().unwrap_or(label);
    let capability = parse_capability_label(capability_label)?;

    let scope = match parts
        .next()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        Some(raw_scope) => parse_scope_string(capability, raw_scope),
        None => default_scope(capability),
    };

    Ok(CapabilityGrant {
        capability,
        scope,
        decision: Decision::Allow,
    })
}

fn parse_capability_label(label: &str) -> Result<Capability> {
    match label.trim() {
        "fs.read" | "fs_read" => Ok(Capability::FsRead),
        "fs.write" | "fs_write" => Ok(Capability::FsWrite),
        "net" => Ok(Capability::Net),
        "open_url" | "open.url" | "open-url" => Ok(Capability::OpenUrl),
        "clipboard" => Ok(Capability::Clipboard),
        "clock" => Ok(Capability::Clock),
        "random" => Ok(Capability::Random),
        "terminal.raw_input" | "terminal_raw_input" => Ok(Capability::TerminalRawInput),
        other => Err(anyhow!("unknown capability label: {other}")),
    }
}

fn parse_decision_label(label: &str) -> Result<Decision> {
    match label.trim().to_lowercase().as_str() {
        "allow" => Ok(Decision::Allow),
        "deny" => Ok(Decision::Deny),
        other => Err(anyhow!("unknown decision label: {other}")),
    }
}

fn parse_scope_value(capability: Capability, value: &serde_json::Value) -> Result<Scope> {
    match value {
        serde_json::Value::Null => Ok(default_scope(capability)),
        serde_json::Value::String(label) => Ok(parse_scope_string(capability, label)),
        serde_json::Value::Array(items) => {
            let mut values = Vec::new();
            for item in items {
                let text = item
                    .as_str()
                    .ok_or_else(|| anyhow!("scope array values must be strings"))?;
                values.push(text.to_string());
            }

            Ok(match capability {
                Capability::FsRead | Capability::FsWrite => Scope::Paths(values),
                Capability::Net => Scope::Allowlist(values),
                _ => Scope::None,
            })
        }
        serde_json::Value::Object(map) => {
            if map.get("prompt").and_then(serde_json::Value::as_bool) == Some(true) {
                return Ok(Scope::Prompt);
            }

            if let Some(path) = map.get("path").and_then(serde_json::Value::as_str) {
                return Ok(Scope::Path(path.to_string()));
            }

            if let Some(paths) = map.get("paths").and_then(serde_json::Value::as_array) {
                let values = paths
                    .iter()
                    .map(|item| {
                        item.as_str()
                            .map(ToString::to_string)
                            .ok_or_else(|| anyhow!("scope.paths values must be strings"))
                    })
                    .collect::<Result<Vec<_>>>()?;
                return Ok(Scope::Paths(values));
            }

            if let Some(allowlist) = map.get("allowlist").and_then(serde_json::Value::as_array) {
                let values = allowlist
                    .iter()
                    .map(|item| {
                        item.as_str()
                            .map(ToString::to_string)
                            .ok_or_else(|| anyhow!("scope.allowlist values must be strings"))
                    })
                    .collect::<Result<Vec<_>>>()?;
                return Ok(Scope::Allowlist(values));
            }

            Err(anyhow!("unsupported scope object shape"))
        }
        _ => Err(anyhow!("invalid scope value")),
    }
}

fn parse_scope_string(capability: Capability, raw: &str) -> Scope {
    let trimmed = raw.trim();
    if trimmed.eq_ignore_ascii_case("none") {
        return Scope::None;
    }
    if trimmed.eq_ignore_ascii_case("prompt") {
        return Scope::Prompt;
    }

    match capability {
        Capability::FsRead | Capability::FsWrite => {
            if trimmed.contains(',') {
                Scope::Paths(
                    trimmed
                        .split(',')
                        .map(str::trim)
                        .filter(|item| !item.is_empty())
                        .map(ToString::to_string)
                        .collect(),
                )
            } else {
                Scope::Path(trimmed.to_string())
            }
        }
        Capability::Net => Scope::Allowlist(
            trimmed
                .split(',')
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .map(ToString::to_string)
                .collect(),
        ),
        Capability::OpenUrl | Capability::Clipboard => Scope::Prompt,
        Capability::Clock | Capability::Random | Capability::TerminalRawInput => Scope::None,
    }
}

fn default_scope(capability: Capability) -> Scope {
    match capability {
        Capability::FsRead | Capability::FsWrite => Scope::Paths(Vec::new()),
        Capability::Net => Scope::Allowlist(Vec::new()),
        Capability::OpenUrl | Capability::Clipboard => Scope::Prompt,
        Capability::Clock | Capability::Random | Capability::TerminalRawInput => Scope::None,
    }
}

fn capability_label(capability: Capability) -> &'static str {
    match capability {
        Capability::FsRead => "fs.read",
        Capability::FsWrite => "fs.write",
        Capability::Net => "net",
        Capability::OpenUrl => "open_url",
        Capability::Clipboard => "clipboard",
        Capability::Clock => "clock",
        Capability::Random => "random",
        Capability::TerminalRawInput => "terminal.raw_input",
    }
}

fn summarize_permissions(permissions: &[serde_json::Value]) -> Result<Vec<String>> {
    let mut out = Vec::new();
    for permission in permissions {
        let normalized = normalize_permission(permission.clone())?;
        let label = capability_label(normalized.capability).to_string();
        if !out.iter().any(|item| item == &label) {
            out.push(label);
        }
    }
    Ok(out)
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct IndexCatalog {
    #[serde(default)]
    schema_version: u32,
    #[serde(default)]
    games: Vec<IndexGame>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct IndexGame {
    id: String,
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    author: String,
    #[serde(default)]
    permissions_summary: Vec<String>,
    #[serde(default)]
    host_api_range: String,
    #[serde(default)]
    entry_type: Option<EntryType>,
    #[serde(default)]
    verified: bool,
    #[serde(default)]
    publisher_id: Option<String>,
    #[serde(default)]
    collections: Vec<String>,
    #[serde(default)]
    compatibility: Option<CompatibilityBadge>,
    #[serde(default)]
    versions: Vec<IndexVersion>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct IndexVersion {
    version: String,
    artifact: String,
    #[serde(default)]
    #[serde(alias = "checksum_sha256")]
    artifact_sha256: Option<String>,
    #[serde(default)]
    signature_uri: Option<String>,
    #[serde(default)]
    publisher_id: Option<String>,
    #[serde(default)]
    signature_fingerprint: Option<String>,
    #[serde(default)]
    size_bytes: Option<u64>,
    #[serde(default)]
    entry_type: Option<EntryType>,
    #[serde(default)]
    host_api: Option<String>,
    #[serde(default)]
    permissions: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedGithubCatalog {
    index_asset_url: String,
    catalog_raw: String,
}

#[derive(Debug, Clone, Deserialize)]
struct GithubReleaseAsset {
    name: String,
    browser_download_url: String,
}

#[derive(Debug, Clone, Deserialize)]
struct GithubRelease {
    assets: Vec<GithubReleaseAsset>,
}

fn parse_github_locator(locator: &str) -> Result<(String, String, String)> {
    let raw = locator
        .strip_prefix("github://")
        .ok_or_else(|| anyhow!("github locator must start with github://"))?;
    let (repo_part, tag_part) = raw
        .rsplit_once('@')
        .ok_or_else(|| anyhow!("github locator must be github://owner/repo@tag"))?;
    let (owner, repo) = repo_part
        .split_once('/')
        .ok_or_else(|| anyhow!("github locator must include owner/repo"))?;
    if owner.trim().is_empty() || repo.trim().is_empty() || tag_part.trim().is_empty() {
        return Err(anyhow!("github locator owner/repo/tag must not be empty"));
    }
    Ok((owner.to_string(), repo.to_string(), tag_part.to_string()))
}

fn select_version<'a>(game: &'a IndexGame, requested: Option<&str>) -> Result<&'a IndexVersion> {
    if game.versions.is_empty() {
        return Err(anyhow!("game '{}' has no versions", game.id));
    }

    if let Some(requested) = requested {
        return game
            .versions
            .iter()
            .find(|item| item.version == requested)
            .ok_or_else(|| anyhow!("version '{requested}' not found for game '{}'", game.id));
    }

    let mut indexed = game
        .versions
        .iter()
        .enumerate()
        .map(|(index, item)| (index, Version::parse(&item.version).ok()))
        .collect::<Vec<_>>();

    indexed.sort_by(|a, b| match (&a.1, &b.1) {
        (Some(a_ver), Some(b_ver)) => b_ver.cmp(a_ver),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => game.versions[b.0].version.cmp(&game.versions[a.0].version),
    });

    let selected_index = indexed
        .first()
        .map(|item| item.0)
        .ok_or_else(|| anyhow!("no version entries found for game '{}'", game.id))?;

    game.versions
        .get(selected_index)
        .ok_or_else(|| anyhow!("invalid version index for game '{}'", game.id))
}

fn validate_index_version_security_fields(version: &IndexVersion) -> Result<()> {
    if let Some(sha) = &version.artifact_sha256 {
        let valid_len = sha.len() == 64;
        let valid_hex = sha.chars().all(|ch| ch.is_ascii_hexdigit());
        if !valid_len || !valid_hex {
            return Err(anyhow!(
                "invalid artifact_sha256 '{}': expected 64 lowercase/uppercase hex chars",
                sha
            ));
        }
    }

    if let Some(signature_uri) = &version.signature_uri
        && signature_uri.trim().is_empty()
    {
        return Err(anyhow!("signature_uri must not be empty when present"));
    }

    if let Some(publisher_id) = &version.publisher_id
        && publisher_id.trim().is_empty()
    {
        return Err(anyhow!("publisher_id must not be empty when present"));
    }

    Ok(())
}

fn resolve_locator_relative(base_locator: &str, relative: &str) -> String {
    if relative.starts_with("http://")
        || relative.starts_with("https://")
        || relative.starts_with("file://")
    {
        return relative.to_string();
    }

    if base_locator.starts_with("http://") || base_locator.starts_with("https://") {
        let base_dir = base_locator
            .rsplit_once('/')
            .map(|(prefix, _)| prefix)
            .unwrap_or(base_locator);
        return format!("{base_dir}/{relative}");
    }

    let base_path = locator_to_path(base_locator);
    let joined = base_path
        .parent()
        .map(|parent| parent.join(relative))
        .unwrap_or_else(|| PathBuf::from(relative));

    joined.to_string_lossy().to_string()
}

fn locator_to_path(locator: &str) -> PathBuf {
    if let Some(rest) = locator.strip_prefix("file://") {
        return PathBuf::from(rest);
    }

    PathBuf::from(locator)
}

fn fetch_text_from_locator(locator: &str) -> Result<String> {
    if locator.starts_with("http://") || locator.starts_with("https://") {
        let bytes = fetch_http_bytes_with_retry(locator)?;
        return String::from_utf8(bytes)
            .map_err(|err| anyhow!("HTTP response from {locator} was not valid UTF-8: {err}"));
    }

    let path = locator_to_path(locator);
    fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))
}

fn fetch_bytes_from_locator(locator: &str) -> Result<Vec<u8>> {
    if locator.starts_with("http://") || locator.starts_with("https://") {
        return fetch_http_bytes_with_retry(locator);
    }

    let path = locator_to_path(locator);
    fs::read(&path).with_context(|| format!("failed to read {}", path.display()))
}

pub fn fetch_text(locator: &str) -> Result<String> {
    fetch_text_from_locator(locator)
}

pub fn fetch_bytes(locator: &str) -> Result<Vec<u8>> {
    fetch_bytes_from_locator(locator)
}

fn fetch_http_bytes_with_retry(locator: &str) -> Result<Vec<u8>> {
    let mut last_retryable_error = None::<String>;
    for attempt in 1..=HTTP_MAX_ATTEMPTS {
        match fetch_http_bytes_once(locator) {
            Ok(bytes) => return Ok(bytes),
            Err(HttpAttemptError::Status(status)) => {
                let retryable = is_retryable_http_status(status);
                let message = format!("HTTP status {status} from {locator}");
                if !retryable || attempt == HTTP_MAX_ATTEMPTS {
                    return Err(anyhow!(message));
                }
                last_retryable_error = Some(message);
            }
            Err(HttpAttemptError::Transport(err)) => {
                let message = format!("HTTP transport failure for {locator}: {err}");
                if attempt == HTTP_MAX_ATTEMPTS {
                    return Err(anyhow!(message));
                }
                last_retryable_error = Some(message);
            }
            Err(HttpAttemptError::Read(err)) => {
                let message = format!("HTTP read failure for {locator}: {err}");
                if attempt == HTTP_MAX_ATTEMPTS {
                    return Err(anyhow!(message));
                }
                last_retryable_error = Some(message);
            }
        }

        std::thread::sleep(Duration::from_millis(
            HTTP_RETRY_BACKOFF_BASE_MS * u64::try_from(attempt).unwrap_or(1),
        ));
    }

    Err(anyhow!(
        "{}",
        last_retryable_error.unwrap_or_else(|| format!("HTTP fetch failed for {locator}"))
    ))
}

fn fetch_http_bytes_once(locator: &str) -> std::result::Result<Vec<u8>, HttpAttemptError> {
    #[cfg(test)]
    if let Some(step) = pop_http_mock_step(locator) {
        return match step {
            MockHttpStep::Bytes(bytes) => Ok(bytes),
            MockHttpStep::Status(status) => Err(HttpAttemptError::Status(status)),
            MockHttpStep::Transport(err) => Err(HttpAttemptError::Transport(err)),
        };
    }

    let response = reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_millis(HTTP_CONNECT_TIMEOUT_MS))
        .timeout(Duration::from_millis(HTTP_READ_TIMEOUT_MS))
        .user_agent("dark-forest/2.0")
        .build()
        .map_err(|err| HttpAttemptError::Transport(err.to_string()))?
        .get(locator)
        .send()
        .map_err(|err| HttpAttemptError::Transport(err.to_string()))?;

    let status = response.status();
    if !status.is_success() {
        return Err(HttpAttemptError::Status(status.as_u16()));
    }

    let bytes = response
        .bytes()
        .map_err(|err| HttpAttemptError::Read(err.to_string()))?;
    Ok(bytes.to_vec())
}

fn is_retryable_http_status(status: u16) -> bool {
    matches!(status, 408 | 429 | 500 | 502 | 503 | 504)
}

fn atomic_write_bytes(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let tmp = path.with_extension("tmp");
    let mut file = File::create(&tmp)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);

    fs::rename(&tmp, path)?;

    if let Some(parent) = path.parent()
        && let Ok(dir) = File::open(parent)
    {
        let _ = dir.sync_all();
    }

    Ok(())
}

pub fn unpack_tarball_to_dir(tarball_path: &Path, destination_dir: &Path) -> Result<()> {
    fs::create_dir_all(destination_dir)?;
    let file = File::open(tarball_path)
        .with_context(|| format!("failed to open tarball {}", tarball_path.display()))?;
    let decoder = GzDecoder::new(file);
    let mut archive = Archive::new(decoder);
    archive
        .unpack(destination_dir)
        .with_context(|| format!("failed to unpack tarball {}", tarball_path.display()))?;
    Ok(())
}

fn to_hex_lower(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{
        EntryType, IndexRegistryProvider, MockHttpStep, RegistryProvider, parse_manifest,
        register_http_mock, to_hex_lower, unpack_tarball_to_dir,
    };
    use anyhow::Result;
    use proptest::prelude::*;
    use sha2::{Digest, Sha256};
    use std::fs;
    use std::path::Path;
    use tar::Builder;

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
            "permissions":["terminal.raw_input"]
        }"#;

        let manifest = parse_manifest(payload)?;
        assert_eq!(manifest.id, "snake");
        assert_eq!(manifest.entry_type, EntryType::Native);
        assert_eq!(manifest.permissions.len(), 1);
        Ok(())
    }

    #[test]
    fn manifest_parser_accepts_typed_permissions() -> Result<()> {
        let payload = r#"{
            "id":"wasm-game",
            "name":"Wasm Game",
            "version":"0.2.0",
            "author":"Dark Forest",
            "entry_type":"wasm",
            "entry":"main.wasm",
            "host_api":"^0.1",
            "permissions":[
                {"capability":"fs.read","scope":{"paths":["/tmp"]},"decision":"allow"}
            ]
        }"#;

        let manifest = parse_manifest(payload)?;
        assert_eq!(manifest.permissions.len(), 1);
        Ok(())
    }

    #[test]
    fn index_list_returns_catalog() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let root = temp.path();
        let index_path = root.join("index.json");
        let artifact = root.join("snake-0.2.0.tar.gz");
        create_tarball(&artifact, "snake-plus", "0.2.0")?;

        fs::write(&index_path, sample_index_json(&artifact, "0.2.0")?)?;

        let provider = IndexRegistryProvider::new(
            index_path.to_string_lossy().to_string(),
            root.join("cache"),
        );

        let listings = provider.list()?;
        assert_eq!(listings.len(), 1);
        assert_eq!(listings[0].id, "snake-plus");
        Ok(())
    }

    #[test]
    fn index_resolve_prefers_latest_semver() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let root = temp.path();
        let index_path = root.join("index.json");
        let artifact = root.join("snake-0.2.0.tar.gz");
        create_tarball(&artifact, "snake-plus", "0.2.0")?;

        fs::write(
            &index_path,
            sample_index_json_with_versions(&artifact, &["0.1.0", "0.2.0", "0.10.0"])?,
        )?;

        let provider = IndexRegistryProvider::new(
            index_path.to_string_lossy().to_string(),
            root.join("cache"),
        );

        let resolved = provider.resolve("snake-plus", None)?;
        assert_eq!(resolved.version, "0.10.0");
        Ok(())
    }

    #[test]
    fn index_fetch_uses_cache_on_network_error() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let root = temp.path();
        let index_path = root.join("index.json");
        let artifact = root.join("snake-0.2.0.tar.gz");
        create_tarball(&artifact, "snake-plus", "0.2.0")?;

        fs::write(&index_path, sample_index_json(&artifact, "0.2.0")?)?;

        let provider = IndexRegistryProvider::new(
            index_path.to_string_lossy().to_string(),
            root.join("cache"),
        );

        let first = provider.list()?;
        assert_eq!(first.len(), 1);

        fs::remove_file(&index_path)?;
        let second = provider.list()?;
        assert_eq!(second.len(), 1);
        Ok(())
    }

    #[test]
    fn index_http_retry_then_success_and_caches() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let root = temp.path();
        let artifact = root.join("snake-0.2.0.tar.gz");
        create_tarball(&artifact, "snake-plus", "0.2.0")?;
        let index_body = sample_index_json(&artifact, "0.2.0")?;

        let locator = "http://mock.local/index.json".to_string();
        register_http_mock(
            &locator,
            vec![
                MockHttpStep::Status(503_u16),
                MockHttpStep::Bytes(index_body.into_bytes()),
            ],
        );

        let provider = IndexRegistryProvider::new(locator.clone(), root.join("cache"));
        let listings = provider.list()?;
        assert_eq!(listings.len(), 1);

        let cache_path = index_cache_path(root.join("cache").as_path(), &locator);
        assert!(cache_path.exists(), "expected catalog cache to be written");
        Ok(())
    }

    #[test]
    fn index_http_falls_back_to_cache_on_hard_failure() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let root = temp.path();
        let artifact = root.join("snake-0.2.0.tar.gz");
        create_tarball(&artifact, "snake-plus", "0.2.0")?;
        let index_body = sample_index_json(&artifact, "0.2.0")?;

        let locator = "http://mock.local/fallback-index.json".to_string();
        register_http_mock(&locator, vec![MockHttpStep::Bytes(index_body.into_bytes())]);

        let provider = IndexRegistryProvider::new(locator.clone(), root.join("cache"));
        let first = provider.list()?;
        assert_eq!(first.len(), 1);

        register_http_mock(
            &locator,
            vec![MockHttpStep::Transport("offline".to_string())],
        );

        let second = provider.list()?;
        assert_eq!(second.len(), 1);
        Ok(())
    }

    #[test]
    fn corrupt_cached_http_index_returns_deterministic_error() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let root = temp.path();
        let artifact = root.join("snake-0.2.0.tar.gz");
        create_tarball(&artifact, "snake-plus", "0.2.0")?;
        let index_body = sample_index_json(&artifact, "0.2.0")?;

        let locator = "http://mock.local/corrupt-index.json".to_string();
        register_http_mock(&locator, vec![MockHttpStep::Bytes(index_body.into_bytes())]);

        let provider = IndexRegistryProvider::new(locator.clone(), root.join("cache"));
        let first = provider.list()?;
        assert_eq!(first.len(), 1);

        let cache_path = index_cache_path(root.join("cache").as_path(), &locator);
        fs::write(&cache_path, "{not-json")?;

        register_http_mock(
            &locator,
            vec![MockHttpStep::Transport("offline".to_string())],
        );

        let err = provider
            .list()
            .expect_err("expected corrupt cache to produce deterministic error");
        let message = err.to_string();
        assert!(message.contains("no valid cache available"));
        Ok(())
    }

    #[test]
    fn artifact_checksum_mismatch_fails() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let root = temp.path();
        let artifact_path = root.join("snake-0.2.0.tar.gz");
        create_tarball(&artifact_path, "snake-plus", "0.2.0")?;

        let provider = IndexRegistryProvider::new("unused".to_string(), root.join("cache"));
        let artifact = super::ArtifactRef {
            id: "snake-plus".to_string(),
            version: "0.2.0".to_string(),
            source: super::SourceRef {
                scheme: "index".to_string(),
                locator: "unused".to_string(),
            },
            entry_type: EntryType::Wasm,
            artifact_uri: Some(artifact_path.to_string_lossy().to_string()),
            artifact_sha256: Some("deadbeef".to_string()),
            signature_uri: None,
            publisher_id: None,
            signature_fingerprint: None,
            size_bytes: None,
        };

        let result = provider.fetch_artifact_to_cache(&artifact);
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn artifact_unpack_produces_valid_game_json() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let root = temp.path();
        let artifact_path = root.join("snake-0.2.0.tar.gz");
        create_tarball(&artifact_path, "snake-plus", "0.2.0")?;

        let unpacked = root.join("unpacked");
        unpack_tarball_to_dir(&artifact_path, &unpacked)?;

        let manifest_raw = fs::read_to_string(unpacked.join("game.json"))?;
        let manifest = parse_manifest(&manifest_raw)?;
        assert_eq!(manifest.id, "snake-plus");
        Ok(())
    }

    #[test]
    fn github_provider_lists_and_resolves_release_assets() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let root = temp.path();
        let locator = "github://dark-forest/example-games@v1.2.3-a".to_string();
        let release_url =
            "https://api.github.com/repos/dark-forest/example-games/releases/tags/v1.2.3-a";
        let index_asset_url = "https://mock.local/releases/v1.2.3-a/index.json";

        let index_json = serde_json::json!({
            "schema_version": 2,
            "games": [
                {
                    "id": "snake-plus",
                    "name": "Snake+",
                    "description": "arcade",
                    "author": "Dark Forest",
                    "verified": true,
                    "publisher_id": "dark-forest",
                    "collections": ["Featured"],
                    "compatibility": {"host_api":"compatible","permissions":"low-risk"},
                    "permissions_summary": ["terminal.raw_input"],
                    "host_api_range": "^0.1",
                    "versions": [
                        {
                            "version": "0.2.0",
                            "artifact": "snake-plus-0.2.0.tar.gz",
                            "artifact_sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                            "signature_uri": "snake-plus-0.2.0.sig",
                            "signature_fingerprint": "pubkey-1"
                        }
                    ]
                }
            ]
        });
        let release_json = serde_json::json!({
            "assets": [
                {"name": "index.json", "browser_download_url": index_asset_url}
            ]
        });

        register_http_mock(
            release_url,
            vec![MockHttpStep::Bytes(release_json.to_string().into())],
        );
        register_http_mock(
            index_asset_url,
            vec![MockHttpStep::Bytes(index_json.to_string().into())],
        );

        let provider = super::GithubRegistryProvider::new(locator.clone(), root.join("cache"))?;
        let listings = provider.list()?;
        assert_eq!(listings.len(), 1);
        assert_eq!(listings[0].source.scheme, "github");
        assert!(listings[0].verified);
        assert_eq!(listings[0].publisher_id.as_deref(), Some("dark-forest"));

        let resolved = provider.resolve("snake-plus", None)?;
        assert_eq!(
            resolved.artifact_uri.as_deref(),
            Some("https://mock.local/releases/v1.2.3-a/snake-plus-0.2.0.tar.gz")
        );
        assert_eq!(
            resolved.signature_uri.as_deref(),
            Some("https://mock.local/releases/v1.2.3-a/snake-plus-0.2.0.sig")
        );
        assert_eq!(resolved.publisher_id.as_deref(), Some("dark-forest"));
        Ok(())
    }

    #[test]
    fn github_provider_uses_cache_when_release_fetch_fails() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let root = temp.path();
        let locator = "github://dark-forest/example-games@v1.2.3-b".to_string();
        let release_url =
            "https://api.github.com/repos/dark-forest/example-games/releases/tags/v1.2.3-b";
        let index_asset_url = "https://mock.local/releases/v1.2.3-b/index.json";

        let index_json = serde_json::json!({
            "schema_version": 2,
            "games": [
                {
                    "id": "snake-plus",
                    "name": "Snake+",
                    "description": "arcade",
                    "author": "Dark Forest",
                    "versions": [
                        {
                            "version": "0.2.0",
                            "artifact": "snake-plus-0.2.0.tar.gz",
                            "artifact_sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                            "permissions": ["terminal.raw_input"]
                        }
                    ]
                }
            ]
        });
        let release_json = serde_json::json!({
            "assets": [
                {"name": "index.json", "browser_download_url": index_asset_url}
            ]
        });

        register_http_mock(
            release_url,
            vec![MockHttpStep::Bytes(release_json.to_string().into())],
        );
        register_http_mock(
            index_asset_url,
            vec![MockHttpStep::Bytes(index_json.to_string().into())],
        );

        let provider = super::GithubRegistryProvider::new(locator.clone(), root.join("cache"))?;
        let first = provider.list()?;
        assert_eq!(first.len(), 1);

        register_http_mock(
            release_url,
            vec![MockHttpStep::Transport("offline".to_string())],
        );
        let second = provider.list()?;
        assert_eq!(second.len(), 1);
        Ok(())
    }

    #[test]
    fn index_rejects_malformed_signature_fields() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let root = temp.path();
        let locator = root.join("index.json");
        let payload = serde_json::json!({
            "schema_version": 2,
            "games": [
                {
                    "id": "snake-plus",
                    "name": "Snake+",
                    "author": "Dark Forest",
                    "versions": [
                        {
                            "version": "0.2.0",
                            "artifact": "snake-plus-0.2.0.tar.gz",
                            "artifact_sha256": "not-hex",
                            "signature_uri": "",
                            "permissions": ["terminal.raw_input"]
                        }
                    ]
                }
            ]
        });
        fs::write(&locator, serde_json::to_vec_pretty(&payload)?)?;

        let provider =
            IndexRegistryProvider::new(locator.to_string_lossy().to_string(), root.join("cache"));
        let err = provider
            .list()
            .expect_err("expected malformed signature fields to fail");
        assert!(err.to_string().contains("invalid artifact_sha256"));
        Ok(())
    }

    #[test]
    fn provider_factory_routes_supported_schemes() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let root = temp.path();
        let index = super::provider_from_locator("file:///tmp/index.json", root.join("cache"))?;
        assert!(matches!(index, super::AnyRegistryProvider::Index(_)));
        let github =
            super::provider_from_locator("github://owner/repo@v1.0.0", root.join("cache"))?;
        assert!(matches!(github, super::AnyRegistryProvider::Github(_)));
        Ok(())
    }

    fn sample_index_json(artifact_path: &Path, version: &str) -> Result<String> {
        sample_index_json_with_versions(artifact_path, &[version])
    }

    fn sample_index_json_with_versions(artifact_path: &Path, versions: &[&str]) -> Result<String> {
        let artifact_bytes = fs::read(artifact_path)?;
        let checksum = to_hex_lower(&Sha256::digest(&artifact_bytes));
        let version_items = versions
            .iter()
            .map(|version| {
                serde_json::json!({
                    "version": version,
                    "artifact": artifact_path.to_string_lossy(),
                    "checksum_sha256": checksum,
                    "entry_type": "wasm",
                    "host_api": "^0.1",
                    "permissions": ["terminal.raw_input"]
                })
            })
            .collect::<Vec<_>>();

        let payload = serde_json::json!({
            "schema_version": 1,
            "games": [
                {
                    "id": "snake-plus",
                    "name": "Snake+",
                    "description": "Arcade loop",
                    "tags": ["arcade", "builtin"],
                    "author": "Dark Forest",
                    "permissions_summary": ["terminal.raw_input"],
                    "host_api_range": "^0.1",
                    "entry_type": "wasm",
                    "versions": version_items
                }
            ]
        });

        Ok(serde_json::to_string_pretty(&payload)?)
    }

    fn create_tarball(path: &Path, id: &str, version: &str) -> Result<()> {
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        {
            let mut tar = Builder::new(&mut encoder);
            let manifest = serde_json::json!({
                "id": id,
                "name": "Snake+",
                "version": version,
                "author": "Dark Forest",
                "entry_type": "wasm",
                "entry": "main.wasm",
                "host_api": "^0.1",
                "permissions": ["terminal.raw_input"]
            });
            let manifest_data = serde_json::to_vec_pretty(&manifest)?;

            let mut header = tar::Header::new_gnu();
            header.set_path("game.json")?;
            header.set_size(manifest_data.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            tar.append(&header, manifest_data.as_slice())?;

            let payload = b"wasm bytes";
            let mut payload_header = tar::Header::new_gnu();
            payload_header.set_path("main.wasm")?;
            payload_header.set_size(payload.len() as u64);
            payload_header.set_mode(0o644);
            payload_header.set_cksum();
            tar.append(&payload_header, payload.as_slice())?;
            tar.finish()?;
        }
        let bytes = encoder.finish()?;
        fs::write(path, bytes)?;
        Ok(())
    }

    fn index_cache_path(cache_root: &Path, locator: &str) -> std::path::PathBuf {
        let digest = Sha256::digest(locator.as_bytes());
        let key = to_hex_lower(&digest);
        cache_root.join("index").join(format!("{key}.json"))
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
