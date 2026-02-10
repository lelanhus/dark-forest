use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use chrono::{DateTime, Utc};
use directories::BaseDirs;
use plugin_host::{Capability, CapabilityGrant, Decision, Scope};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

pub const CURRENT_SCHEMA_VERSION: u32 = 2;

fn default_registry_scheme() -> String {
    "index".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RegistryConfig {
    #[serde(default = "default_registry_scheme")]
    pub scheme: String,
    pub locator: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub schema_version: u32,
    pub theme: String,
    pub performance_mode: String,
    pub keymap_profile: String,
    #[serde(default)]
    pub registries: Vec<RegistryConfig>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            schema_version: CURRENT_SCHEMA_VERSION,
            theme: "forge".to_string(),
            performance_mode: "auto".to_string(),
            keymap_profile: "default".to_string(),
            registries: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PlayHistoryRecord {
    pub game_id: String,
    pub last_played_at: Option<DateTime<Utc>>,
    pub play_count: u64,
    pub total_play_time_seconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PlayHistoryFile {
    pub schema_version: u32,
    pub games: BTreeMap<String, PlayHistoryRecord>,
}

pub type PlayHistoryMap = BTreeMap<String, PlayHistoryRecord>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoreEntry {
    pub score: i64,
    pub recorded_at: DateTime<Utc>,
    pub metadata: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HighScores {
    pub schema_version: u32,
    pub game_id: String,
    pub higher_is_better: bool,
    pub entries: Vec<ScoreEntry>,
}

impl HighScores {
    #[must_use]
    pub fn empty(game_id: &str) -> Self {
        Self {
            schema_version: CURRENT_SCHEMA_VERSION,
            game_id: game_id.to_string(),
            higher_is_better: true,
            entries: Vec::new(),
        }
    }

    pub fn push_score(&mut self, score: i64) {
        self.entries.push(ScoreEntry {
            score,
            recorded_at: Utc::now(),
            metadata: BTreeMap::new(),
        });

        if self.higher_is_better {
            self.entries.sort_by(|a, b| b.score.cmp(&a.score));
        } else {
            self.entries.sort_by(|a, b| a.score.cmp(&b.score));
        }

        self.entries.truncate(20);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledFile {
    pub schema_version: u32,
    pub installed: Vec<InstalledRecord>,
}

impl Default for InstalledFile {
    fn default() -> Self {
        Self {
            schema_version: CURRENT_SCHEMA_VERSION,
            installed: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledRecord {
    pub id: String,
    pub source: String,
    pub current_version: String,
    #[serde(default)]
    pub installed_versions: Vec<String>,
    #[serde(default)]
    pub version_checksums: BTreeMap<String, String>,
}

impl InstalledRecord {
    fn normalize(&mut self) {
        if self.installed_versions.is_empty() && !self.current_version.is_empty() {
            self.installed_versions.push(self.current_version.clone());
        }

        if !self
            .installed_versions
            .iter()
            .any(|item| item == &self.current_version)
            && !self.current_version.is_empty()
        {
            self.installed_versions.push(self.current_version.clone());
        }

        let mut deduped = Vec::with_capacity(self.installed_versions.len());
        for version in &self.installed_versions {
            if !deduped.contains(version) {
                deduped.push(version.clone());
            }
        }
        self.installed_versions = deduped;
    }

    fn push_version(&mut self, version: &str) {
        if !self.installed_versions.iter().any(|item| item == version) {
            self.installed_versions.push(version.to_string());
        }
        self.current_version = version.to_string();
        self.normalize();
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PermissionsFile {
    pub schema_version: u32,
    pub grants: BTreeMap<String, Vec<PermissionGrant>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionGrant {
    pub capability: Capability,
    pub scope: Scope,
    pub decision: Decision,
    pub remembered: bool,
    pub granted_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct InstallRequest {
    pub game_id: String,
    pub version: String,
    pub source: String,
    pub artifact_dir: PathBuf,
    pub expected_sha256: Option<String>,
}

#[derive(Debug, Clone)]
pub struct InstallOutcome {
    pub game_id: String,
    pub version: String,
    pub current_pointer: String,
    pub checksum_sha256: String,
}

#[derive(Debug, Clone)]
pub struct RollbackOutcome {
    pub game_id: String,
    pub from_version: String,
    pub to_version: String,
}

#[derive(Debug, Clone)]
pub struct VerifyOutcome {
    pub game_id: String,
    pub version: String,
    pub verified: bool,
    pub expected_sha256: Option<String>,
    pub actual_sha256: String,
}

#[derive(Debug, Clone)]
pub struct RemoveOutcome {
    pub game_id: String,
    pub removed_version: String,
    pub had_versions_remaining: bool,
}

#[derive(Debug, Error)]
pub enum ContentTransactionError {
    #[error("invalid artifact directory: {0}")]
    InvalidArtifact(PathBuf),
    #[error("artifact manifest is missing required field '{0}'")]
    InvalidManifestField(String),
    #[error("artifact manifest id/version mismatch (expected {expected_id}@{expected_version})")]
    ManifestMismatch {
        expected_id: String,
        expected_version: String,
    },
    #[error("checksum mismatch (expected {expected}, actual {actual})")]
    ChecksumMismatch { expected: String, actual: String },
    #[error("game is not installed: {0}")]
    GameNotInstalled(String),
    #[error("no rollback target exists for game: {0}")]
    NoRollbackTarget(String),
    #[error("installed version directory is missing for {game_id}@{version}")]
    MissingInstalledVersion { game_id: String, version: String },
    #[error("cannot remove protected source for {game_id}: {source_ref}")]
    ProtectedSource { game_id: String, source_ref: String },
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

pub trait ContentStore {
    fn load_settings(&self) -> Result<Settings>;
    fn save_settings(&self, settings: &Settings) -> Result<()>;
    fn load_play_history(&self) -> Result<PlayHistoryMap>;
    fn save_play_history(&self, history: &PlayHistoryMap) -> Result<()>;
    fn load_installed(&self) -> Result<InstalledFile>;
    fn save_installed(&self, installed: &InstalledFile) -> Result<()>;
    fn load_permissions(&self) -> Result<PermissionsFile>;
    fn save_permissions(&self, permissions: &PermissionsFile) -> Result<()>;
    fn load_high_scores(&self, game_id: &str) -> Result<HighScores>;
    fn save_high_scores(&self, game_id: &str, scores: &HighScores) -> Result<()>;
}

#[derive(Debug, Clone)]
pub struct JsonContentStore {
    root: PathBuf,
}

impl JsonContentStore {
    #[must_use]
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn create_with_default_root() -> Result<Self> {
        let base = BaseDirs::new().context("failed to resolve base directories")?;
        let root = base.home_dir().join(".dark-forest");
        let store = Self::new(root);
        store.ensure_layout()?;
        Ok(store)
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    #[must_use]
    pub fn cache_dir_path(&self) -> PathBuf {
        self.cache_dir()
    }

    pub fn ensure_layout(&self) -> Result<()> {
        fs::create_dir_all(&self.root)?;
        fs::create_dir_all(self.high_scores_dir())?;
        fs::create_dir_all(self.quarantine_dir())?;
        fs::create_dir_all(self.games_dir())?;
        fs::create_dir_all(self.tmp_dir())?;
        fs::create_dir_all(self.cache_dir())?;
        self.ensure_seed_files()?;
        Ok(())
    }

    pub fn install_from_directory(
        &self,
        request: &InstallRequest,
    ) -> std::result::Result<InstallOutcome, ContentTransactionError> {
        self.ensure_layout()
            .map_err(ContentTransactionError::Other)?;

        validate_artifact_manifest(&request.artifact_dir, &request.game_id, &request.version)?;
        let declared_permissions = load_manifest_permissions(&request.artifact_dir)?;

        let stage_dir = self
            .tmp_dir()
            .join(format!("install-{}-{}", request.game_id, request.version));

        if stage_dir.exists() {
            fs::remove_dir_all(&stage_dir).map_err(|err| {
                ContentTransactionError::Other(anyhow!(
                    "failed to clean stale stage dir {}: {err}",
                    stage_dir.display()
                ))
            })?;
        }

        copy_dir_recursive(&request.artifact_dir, &stage_dir)
            .map_err(ContentTransactionError::Other)?;
        let checksum = hash_directory_sha256(&stage_dir).map_err(ContentTransactionError::Other)?;

        if let Some(expected) = &request.expected_sha256
            && !expected.eq_ignore_ascii_case(&checksum)
        {
            let _ = fs::remove_dir_all(&stage_dir);
            return Err(ContentTransactionError::ChecksumMismatch {
                expected: expected.clone(),
                actual: checksum,
            });
        }

        let final_dir = self.game_version_dir(&request.game_id, &request.version);
        fs::create_dir_all(self.game_dir(&request.game_id))
            .map_err(|err| ContentTransactionError::Other(anyhow!(err)))?;
        if final_dir.exists() {
            let _ = fs::remove_dir_all(&stage_dir);
        } else {
            fs::rename(&stage_dir, &final_dir).map_err(|err| {
                ContentTransactionError::Other(anyhow!(
                    "failed to atomically move staged artifact to {}: {err}",
                    final_dir.display()
                ))
            })?;
        }

        let mut installed = self
            .load_installed()
            .map_err(ContentTransactionError::Other)?;
        let previous_installed = installed.clone();
        let prior_current = installed
            .installed
            .iter()
            .find(|record| record.id == request.game_id)
            .map(|record| record.current_version.clone());
        let mut next_permissions = self
            .load_permissions()
            .map_err(ContentTransactionError::Other)?;
        reconcile_permissions_for_manifest(
            &mut next_permissions,
            &request.game_id,
            &declared_permissions,
        );

        let record = upsert_installed_record(&mut installed, &request.game_id, &request.source);
        record.push_version(&request.version);
        record
            .version_checksums
            .insert(request.version.clone(), checksum.clone());

        self.write_current_pointer(&request.game_id, &request.version)
            .map_err(ContentTransactionError::Other)?;

        if let Err(err) = self.save_installed(&installed) {
            if let Some(previous) = prior_current.clone() {
                let _ = self.write_current_pointer(&request.game_id, &previous);
            }
            return Err(ContentTransactionError::Other(err));
        }

        if let Err(err) = self.save_permissions(&next_permissions) {
            let _ = self.save_installed(&previous_installed);
            if let Some(previous) = prior_current.clone() {
                let _ = self.write_current_pointer(&request.game_id, &previous);
            }
            return Err(ContentTransactionError::Other(err));
        }

        Ok(InstallOutcome {
            game_id: request.game_id.clone(),
            version: request.version.clone(),
            current_pointer: request.version.clone(),
            checksum_sha256: checksum,
        })
    }

    pub fn rollback_game(
        &self,
        game_id: &str,
    ) -> std::result::Result<RollbackOutcome, ContentTransactionError> {
        let mut installed = self
            .load_installed()
            .map_err(ContentTransactionError::Other)?;

        let record = installed
            .installed
            .iter_mut()
            .find(|entry| entry.id == game_id)
            .ok_or_else(|| ContentTransactionError::GameNotInstalled(game_id.to_string()))?;

        let from_version = record.current_version.clone();
        let current_idx = record
            .installed_versions
            .iter()
            .position(|item| item == &record.current_version)
            .unwrap_or_else(|| record.installed_versions.len().saturating_sub(1));

        if current_idx == 0 || record.installed_versions.len() < 2 {
            return Err(ContentTransactionError::NoRollbackTarget(
                game_id.to_string(),
            ));
        }

        let to_version = record.installed_versions[current_idx - 1].clone();
        if !self.game_version_dir(game_id, &to_version).exists() {
            return Err(ContentTransactionError::MissingInstalledVersion {
                game_id: game_id.to_string(),
                version: to_version,
            });
        }

        record.current_version = to_version.clone();
        record.normalize();

        self.write_current_pointer(game_id, &to_version)
            .map_err(ContentTransactionError::Other)?;

        if let Err(err) = self.save_installed(&installed) {
            let _ = self.write_current_pointer(game_id, &from_version);
            return Err(ContentTransactionError::Other(err));
        }

        Ok(RollbackOutcome {
            game_id: game_id.to_string(),
            from_version,
            to_version,
        })
    }

    pub fn verify_game(
        &self,
        game_id: &str,
    ) -> std::result::Result<VerifyOutcome, ContentTransactionError> {
        let installed = self
            .load_installed()
            .map_err(ContentTransactionError::Other)?;

        let record = installed
            .installed
            .iter()
            .find(|entry| entry.id == game_id)
            .ok_or_else(|| ContentTransactionError::GameNotInstalled(game_id.to_string()))?;

        let version_dir = self.game_version_dir(game_id, &record.current_version);
        if !version_dir.exists() {
            return Err(ContentTransactionError::MissingInstalledVersion {
                game_id: game_id.to_string(),
                version: record.current_version.clone(),
            });
        }

        let actual_sha256 =
            hash_directory_sha256(&version_dir).map_err(ContentTransactionError::Other)?;
        let expected_sha256 = record
            .version_checksums
            .get(&record.current_version)
            .cloned();

        let verified = expected_sha256
            .as_ref()
            .is_none_or(|expected| expected.eq_ignore_ascii_case(&actual_sha256));

        Ok(VerifyOutcome {
            game_id: game_id.to_string(),
            version: record.current_version.clone(),
            verified,
            expected_sha256,
            actual_sha256,
        })
    }

    pub fn remove_game(
        &self,
        game_id: &str,
    ) -> std::result::Result<RemoveOutcome, ContentTransactionError> {
        self.ensure_layout()
            .map_err(ContentTransactionError::Other)?;

        let installed = self
            .load_installed()
            .map_err(ContentTransactionError::Other)?;
        let index = installed
            .installed
            .iter()
            .position(|entry| entry.id == game_id)
            .ok_or_else(|| ContentTransactionError::GameNotInstalled(game_id.to_string()))?;
        let record = installed.installed[index].clone();

        if record.source.starts_with("builtin://") {
            return Err(ContentTransactionError::ProtectedSource {
                game_id: game_id.to_string(),
                source_ref: record.source,
            });
        }

        let mut next_installed = installed.clone();
        let removed = next_installed.installed.remove(index);

        let permissions = self
            .load_permissions()
            .map_err(ContentTransactionError::Other)?;
        let mut next_permissions = permissions.clone();
        next_permissions.grants.remove(game_id);

        self.save_installed(&next_installed)
            .map_err(ContentTransactionError::Other)?;

        if let Err(err) = self.save_permissions(&next_permissions) {
            let _ = self.save_installed(&installed);
            return Err(ContentTransactionError::Other(err));
        }

        let game_dir = self.game_dir(game_id);
        if game_dir.exists() {
            fs::remove_dir_all(&game_dir).map_err(|err| {
                ContentTransactionError::Other(anyhow!(
                    "failed to remove game directory {}: {err}",
                    game_dir.display()
                ))
            })?;
        }

        Ok(RemoveOutcome {
            game_id: game_id.to_string(),
            removed_version: removed.current_version,
            had_versions_remaining: removed.installed_versions.len() > 1,
        })
    }

    #[must_use]
    pub fn read_current_pointer(&self, game_id: &str) -> Option<String> {
        let path = self.current_pointer_path(game_id);
        let value = fs::read_to_string(path).ok()?;
        let trimmed = value.trim().to_string();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    }

    fn settings_path(&self) -> PathBuf {
        self.root.join("settings.json")
    }

    fn play_history_path(&self) -> PathBuf {
        self.root.join("play_history.json")
    }

    fn high_scores_dir(&self) -> PathBuf {
        self.root.join("high_scores")
    }

    fn high_score_path(&self, game_id: &str) -> PathBuf {
        self.high_scores_dir().join(format!("{game_id}.json"))
    }

    fn quarantine_dir(&self) -> PathBuf {
        self.root.join("quarantine")
    }

    fn games_dir(&self) -> PathBuf {
        self.root.join("games")
    }

    fn game_dir(&self, game_id: &str) -> PathBuf {
        self.games_dir().join(game_id)
    }

    fn game_version_dir(&self, game_id: &str, version: &str) -> PathBuf {
        self.game_dir(game_id).join(version)
    }

    fn current_pointer_path(&self, game_id: &str) -> PathBuf {
        self.game_dir(game_id).join("current")
    }

    fn tmp_dir(&self) -> PathBuf {
        self.root.join("tmp")
    }

    fn cache_dir(&self) -> PathBuf {
        self.root.join("cache")
    }

    fn installed_path(&self) -> PathBuf {
        self.root.join("installed.json")
    }

    fn permissions_path(&self) -> PathBuf {
        self.root.join("permissions.json")
    }

    fn ensure_seed_files(&self) -> Result<()> {
        if !self.settings_path().exists() {
            self.atomic_write_json(&self.settings_path(), &Settings::default())?;
        }

        if !self.play_history_path().exists() {
            self.atomic_write_json(
                &self.play_history_path(),
                &PlayHistoryFile {
                    schema_version: CURRENT_SCHEMA_VERSION,
                    games: BTreeMap::new(),
                },
            )?;
        }

        if !self.installed_path().exists() {
            self.atomic_write_json(&self.installed_path(), &InstalledFile::default())?;
        }

        if !self.permissions_path().exists() {
            self.atomic_write_json(
                &self.permissions_path(),
                &PermissionsFile {
                    schema_version: CURRENT_SCHEMA_VERSION,
                    grants: BTreeMap::new(),
                },
            )?;
        }

        Ok(())
    }

    fn read_json_or_quarantine<T>(&self, path: &Path) -> Result<Option<T>>
    where
        T: for<'de> Deserialize<'de>,
    {
        if !path.exists() {
            return Ok(None);
        }

        let data = fs::read_to_string(path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        match serde_json::from_str::<T>(&data) {
            Ok(parsed) => Ok(Some(parsed)),
            Err(err) => {
                self.quarantine(path)?;
                tracing::warn!(
                    path = %path.display(),
                    error = %err,
                    "invalid JSON moved to quarantine"
                );
                Ok(None)
            }
        }
    }

    fn quarantine(&self, path: &Path) -> Result<()> {
        if !path.exists() {
            return Ok(());
        }

        let timestamp = Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
        let target_dir = self.quarantine_dir().join(timestamp);
        fs::create_dir_all(&target_dir)?;
        let file_name = path
            .file_name()
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("unknown.json"));
        let target = target_dir.join(file_name);
        fs::rename(path, target)?;
        Ok(())
    }

    fn atomic_write_json<T>(&self, path: &Path, value: &T) -> Result<()>
    where
        T: Serialize,
    {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let tmp_path = path.with_extension("json.tmp");
        let json = serde_json::to_vec_pretty(value)?;

        let mut file = File::create(&tmp_path)?;
        file.write_all(&json)?;
        file.sync_all()?;
        drop(file);

        fs::rename(&tmp_path, path)?;

        if let Some(parent) = path.parent()
            && let Ok(dir) = File::open(parent)
        {
            let _ = dir.sync_all();
        }

        Ok(())
    }

    fn write_current_pointer(&self, game_id: &str, version: &str) -> Result<()> {
        let pointer_path = self.current_pointer_path(game_id);
        if let Some(parent) = pointer_path.parent() {
            fs::create_dir_all(parent)?;
        }

        let tmp_path = pointer_path.with_extension("tmp");
        fs::write(&tmp_path, format!("{version}\n"))?;
        if let Ok(file) = File::open(&tmp_path) {
            let _ = file.sync_all();
        }

        fs::rename(&tmp_path, &pointer_path)?;

        if let Some(parent) = pointer_path.parent()
            && let Ok(dir) = File::open(parent)
        {
            let _ = dir.sync_all();
        }

        Ok(())
    }
}

impl ContentStore for JsonContentStore {
    fn load_settings(&self) -> Result<Settings> {
        self.ensure_layout()?;
        let path = self.settings_path();
        let loaded = self.read_json_or_quarantine::<Settings>(&path)?;
        Ok(loaded.unwrap_or_default())
    }

    fn save_settings(&self, settings: &Settings) -> Result<()> {
        self.atomic_write_json(&self.settings_path(), settings)
    }

    fn load_play_history(&self) -> Result<PlayHistoryMap> {
        self.ensure_layout()?;
        let path = self.play_history_path();
        let loaded = self.read_json_or_quarantine::<PlayHistoryFile>(&path)?;
        Ok(loaded.unwrap_or_default().games)
    }

    fn save_play_history(&self, history: &PlayHistoryMap) -> Result<()> {
        let file = PlayHistoryFile {
            schema_version: CURRENT_SCHEMA_VERSION,
            games: history.clone(),
        };
        self.atomic_write_json(&self.play_history_path(), &file)
    }

    fn load_installed(&self) -> Result<InstalledFile> {
        self.ensure_layout()?;
        let path = self.installed_path();
        let loaded = self.read_json_or_quarantine::<InstalledFile>(&path)?;
        Ok(normalize_installed_file(loaded.unwrap_or_default()))
    }

    fn save_installed(&self, installed: &InstalledFile) -> Result<()> {
        let normalized = normalize_installed_file(installed.clone());
        self.atomic_write_json(&self.installed_path(), &normalized)
    }

    fn load_permissions(&self) -> Result<PermissionsFile> {
        self.ensure_layout()?;
        let path = self.permissions_path();
        let loaded = self.read_json_or_quarantine::<PermissionsFile>(&path)?;
        Ok(loaded.unwrap_or(PermissionsFile {
            schema_version: CURRENT_SCHEMA_VERSION,
            grants: BTreeMap::new(),
        }))
    }

    fn save_permissions(&self, permissions: &PermissionsFile) -> Result<()> {
        self.atomic_write_json(&self.permissions_path(), permissions)
    }

    fn load_high_scores(&self, game_id: &str) -> Result<HighScores> {
        self.ensure_layout()?;
        let path = self.high_score_path(game_id);
        let loaded = self.read_json_or_quarantine::<HighScores>(&path)?;
        Ok(loaded.unwrap_or_else(|| HighScores::empty(game_id)))
    }

    fn save_high_scores(&self, game_id: &str, scores: &HighScores) -> Result<()> {
        self.atomic_write_json(&self.high_score_path(game_id), scores)
    }
}

fn upsert_installed_record<'a>(
    installed: &'a mut InstalledFile,
    game_id: &str,
    source: &str,
) -> &'a mut InstalledRecord {
    if let Some(index) = installed
        .installed
        .iter()
        .position(|record| record.id == game_id)
    {
        let existing = &mut installed.installed[index];
        existing.source = source.to_string();
        return existing;
    }

    installed.installed.push(InstalledRecord {
        id: game_id.to_string(),
        source: source.to_string(),
        current_version: String::new(),
        installed_versions: Vec::new(),
        version_checksums: BTreeMap::new(),
    });

    installed.installed.last_mut().expect("record was inserted")
}

fn normalize_installed_file(mut installed: InstalledFile) -> InstalledFile {
    installed.schema_version = CURRENT_SCHEMA_VERSION;
    for record in &mut installed.installed {
        record.normalize();
    }
    installed
}

fn validate_artifact_manifest(
    artifact_dir: &Path,
    expected_id: &str,
    expected_version: &str,
) -> std::result::Result<(), ContentTransactionError> {
    if !artifact_dir.exists() || !artifact_dir.is_dir() {
        return Err(ContentTransactionError::InvalidArtifact(
            artifact_dir.to_path_buf(),
        ));
    }

    let manifest_path = artifact_dir.join("game.json");
    if !manifest_path.exists() {
        return Err(ContentTransactionError::InvalidArtifact(manifest_path));
    }

    let raw = fs::read_to_string(&manifest_path).map_err(|err| {
        ContentTransactionError::Other(anyhow!(
            "failed to read artifact manifest {}: {err}",
            manifest_path.display()
        ))
    })?;

    let json: serde_json::Value = serde_json::from_str(&raw).map_err(|err| {
        ContentTransactionError::Other(anyhow!(
            "failed to parse artifact manifest {}: {err}",
            manifest_path.display()
        ))
    })?;

    let id = json
        .get("id")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| ContentTransactionError::InvalidManifestField("id".to_string()))?;
    let version = json
        .get("version")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| ContentTransactionError::InvalidManifestField("version".to_string()))?;

    if id != expected_id || version != expected_version {
        return Err(ContentTransactionError::ManifestMismatch {
            expected_id: expected_id.to_string(),
            expected_version: expected_version.to_string(),
        });
    }

    Ok(())
}

fn load_manifest_permissions(
    artifact_dir: &Path,
) -> std::result::Result<Vec<CapabilityGrant>, ContentTransactionError> {
    let manifest_path = artifact_dir.join("game.json");
    let raw = fs::read_to_string(&manifest_path).map_err(|err| {
        ContentTransactionError::Other(anyhow!(
            "failed to read artifact manifest {}: {err}",
            manifest_path.display()
        ))
    })?;

    let parsed = registry::parse_manifest(&raw).map_err(|err| {
        ContentTransactionError::Other(anyhow!(
            "failed to parse artifact permissions from {}: {err}",
            manifest_path.display()
        ))
    })?;
    Ok(parsed.permissions)
}

fn reconcile_permissions_for_manifest(
    permissions: &mut PermissionsFile,
    game_id: &str,
    declared_permissions: &[CapabilityGrant],
) {
    let Some(existing) = permissions.grants.get_mut(game_id) else {
        return;
    };

    let declared_by_capability = declared_permissions
        .iter()
        .map(|grant| (grant.capability, grant.scope.clone()))
        .collect::<BTreeMap<_, _>>();

    existing.retain(|grant| {
        let Some(declared_scope) = declared_by_capability.get(&grant.capability) else {
            return false;
        };
        is_scope_compatible(&grant.scope, declared_scope)
    });

    if existing.is_empty() {
        permissions.grants.remove(game_id);
    }
}

fn is_scope_compatible(granted: &Scope, declared: &Scope) -> bool {
    match (granted, declared) {
        (Scope::None, Scope::None) => true,
        (Scope::Prompt, Scope::Prompt) => true,
        (Scope::Path(path), Scope::Path(expected)) => path == expected,
        (Scope::Path(path), Scope::Paths(declared_paths)) => {
            declared_paths.iter().any(|item| item == path)
        }
        (Scope::Paths(granted_paths), Scope::Path(declared_path)) => {
            granted_paths.iter().all(|path| path == declared_path)
        }
        (Scope::Paths(granted_paths), Scope::Paths(declared_paths)) => granted_paths
            .iter()
            .all(|path| declared_paths.iter().any(|allowed| allowed == path)),
        (Scope::Allowlist(granted_allowlist), Scope::Allowlist(declared_allowlist)) => {
            granted_allowlist
                .iter()
                .all(|entry| declared_allowlist.iter().any(|allowed| allowed == entry))
        }
        _ => false,
    }
}

fn copy_dir_recursive(source: &Path, dest: &Path) -> Result<()> {
    if !source.exists() || !source.is_dir() {
        return Err(anyhow!("source directory is invalid: {}", source.display()));
    }

    fs::create_dir_all(dest)?;

    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let path = entry.path();
        let file_name = entry.file_name();
        let target = dest.join(&file_name);

        if path.is_dir() {
            copy_dir_recursive(&path, &target)?;
            continue;
        }

        if path.is_file() {
            fs::copy(&path, &target)
                .with_context(|| format!("failed to copy {}", path.display()))?;
        }
    }

    Ok(())
}

fn hash_directory_sha256(dir: &Path) -> Result<String> {
    let mut files = Vec::new();
    collect_files(dir, dir, &mut files)?;
    files.sort();

    let mut hasher = Sha256::new();
    for relative in files {
        let absolute = dir.join(&relative);
        hasher.update(relative.as_os_str().to_string_lossy().as_bytes());
        hasher.update([0_u8]);
        let bytes = fs::read(&absolute)
            .with_context(|| format!("failed to read {} for hashing", absolute.display()))?;
        hasher.update(bytes);
    }

    Ok(to_hex_lower(&hasher.finalize()))
}

fn collect_files(root: &Path, current: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(current)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_files(root, &path, files)?;
        } else if path.is_file() {
            let relative = path
                .strip_prefix(root)
                .map(Path::to_path_buf)
                .map_err(|err| anyhow!("failed to strip prefix for {}: {err}", path.display()))?;
            if relative.file_name() == Some(OsStr::new("current")) {
                continue;
            }
            files.push(relative);
        }
    }

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
        ContentStore, Decision, InstallRequest, JsonContentStore, PermissionGrant,
        PlayHistoryRecord, Scope,
    };
    use anyhow::{Result, anyhow};
    use plugin_host::Capability;
    use std::collections::BTreeMap;
    use std::path::{Path, PathBuf};

    #[test]
    fn saves_and_loads_settings() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let store = JsonContentStore::new(PathBuf::from(temp.path()));
        let mut settings = store.load_settings()?;
        settings.performance_mode = "60".to_string();
        settings.registries = vec![super::RegistryConfig {
            scheme: "index".to_string(),
            locator: "file:///tmp/index.json".to_string(),
        }];
        store.save_settings(&settings)?;

        let reloaded = store.load_settings()?;
        assert_eq!(reloaded.performance_mode, "60");
        assert_eq!(reloaded.registries, settings.registries);
        Ok(())
    }

    #[test]
    fn loads_legacy_settings_without_registries() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let root = PathBuf::from(temp.path());
        let store = JsonContentStore::new(root.clone());
        store.ensure_layout()?;

        std::fs::write(
            root.join("settings.json"),
            r#"{
                "schema_version": 2,
                "theme": "forge",
                "performance_mode": "auto",
                "keymap_profile": "default"
            }"#,
        )?;

        let settings = store.load_settings()?;
        assert!(settings.registries.is_empty());
        Ok(())
    }

    #[test]
    fn saves_and_loads_play_history() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let store = JsonContentStore::new(PathBuf::from(temp.path()));

        let mut map = BTreeMap::new();
        map.insert(
            "snake".to_string(),
            PlayHistoryRecord {
                game_id: "snake".to_string(),
                play_count: 3,
                ..PlayHistoryRecord::default()
            },
        );

        store.save_play_history(&map)?;
        let loaded = store.load_play_history()?;
        let snake = loaded
            .get("snake")
            .ok_or_else(|| anyhow!("missing snake play history"))?;
        assert_eq!(snake.play_count, 3);
        Ok(())
    }

    #[test]
    fn quarantines_invalid_json_and_returns_default() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let root = PathBuf::from(temp.path());
        let store = JsonContentStore::new(root.clone());
        store.ensure_layout()?;

        std::fs::write(root.join("settings.json"), "not json")?;
        let settings = store.load_settings()?;
        assert_eq!(settings.theme, "forge");

        let quarantine_dir = root.join("quarantine");
        assert!(quarantine_dir.exists());
        Ok(())
    }

    #[test]
    fn saves_and_loads_installed_records() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let store = JsonContentStore::new(PathBuf::from(temp.path()));
        let mut installed = store.load_installed()?;
        installed.installed.push(super::InstalledRecord {
            id: "snake-plus".to_string(),
            source: "builtin://dark-forest".to_string(),
            current_version: "0.1.0".to_string(),
            installed_versions: vec!["0.1.0".to_string()],
            version_checksums: BTreeMap::new(),
        });
        store.save_installed(&installed)?;

        let reloaded = store.load_installed()?;
        assert_eq!(reloaded.installed.len(), 1);
        assert_eq!(reloaded.installed[0].id, "snake-plus");
        Ok(())
    }

    #[test]
    fn install_commits_current_atomically() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let root = PathBuf::from(temp.path());
        let store = JsonContentStore::new(root.clone());

        let artifact = create_artifact_dir(root.as_path(), "snake-plus", "0.2.0")?;
        let outcome = store
            .install_from_directory(&InstallRequest {
                game_id: "snake-plus".to_string(),
                version: "0.2.0".to_string(),
                source: "local://fixtures".to_string(),
                artifact_dir: artifact,
                expected_sha256: None,
            })
            .map_err(|err| anyhow!(err.to_string()))?;

        assert_eq!(outcome.current_pointer, "0.2.0");
        assert_eq!(
            store.read_current_pointer("snake-plus"),
            Some("0.2.0".to_string())
        );

        let installed = store.load_installed()?;
        let record = installed
            .installed
            .iter()
            .find(|entry| entry.id == "snake-plus")
            .ok_or_else(|| anyhow!("missing installed record"))?;
        assert_eq!(record.current_version, "0.2.0");
        assert!(record.installed_versions.iter().any(|item| item == "0.2.0"));
        Ok(())
    }

    #[test]
    fn failed_install_never_switches_current() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let root = PathBuf::from(temp.path());
        let store = JsonContentStore::new(root.clone());

        let first = create_artifact_dir(root.as_path(), "snake-plus", "0.1.0")?;
        store
            .install_from_directory(&InstallRequest {
                game_id: "snake-plus".to_string(),
                version: "0.1.0".to_string(),
                source: "local://fixtures".to_string(),
                artifact_dir: first,
                expected_sha256: None,
            })
            .map_err(|err| anyhow!(err.to_string()))?;

        let second = create_artifact_dir(root.as_path(), "snake-plus", "0.2.0")?;
        let result = store.install_from_directory(&InstallRequest {
            game_id: "snake-plus".to_string(),
            version: "0.2.0".to_string(),
            source: "local://fixtures".to_string(),
            artifact_dir: second,
            expected_sha256: Some("deadbeef".to_string()),
        });
        assert!(result.is_err());

        assert_eq!(
            store.read_current_pointer("snake-plus"),
            Some("0.1.0".to_string())
        );
        Ok(())
    }

    #[test]
    fn rollback_repoints_current_to_previous() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let root = PathBuf::from(temp.path());
        let store = JsonContentStore::new(root.clone());

        let v1 = create_artifact_dir(root.as_path(), "snake-plus", "0.1.0")?;
        let v2 = create_artifact_dir(root.as_path(), "snake-plus", "0.2.0")?;

        store
            .install_from_directory(&InstallRequest {
                game_id: "snake-plus".to_string(),
                version: "0.1.0".to_string(),
                source: "local://fixtures".to_string(),
                artifact_dir: v1,
                expected_sha256: None,
            })
            .map_err(|err| anyhow!(err.to_string()))?;

        store
            .install_from_directory(&InstallRequest {
                game_id: "snake-plus".to_string(),
                version: "0.2.0".to_string(),
                source: "local://fixtures".to_string(),
                artifact_dir: v2,
                expected_sha256: None,
            })
            .map_err(|err| anyhow!(err.to_string()))?;

        let outcome = store
            .rollback_game("snake-plus")
            .map_err(|err| anyhow!(err.to_string()))?;

        assert_eq!(outcome.from_version, "0.2.0");
        assert_eq!(outcome.to_version, "0.1.0");
        assert_eq!(
            store.read_current_pointer("snake-plus"),
            Some("0.1.0".to_string())
        );
        Ok(())
    }

    #[test]
    fn verify_detects_checksum_mismatch() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let root = PathBuf::from(temp.path());
        let store = JsonContentStore::new(root.clone());

        let artifact = create_artifact_dir(root.as_path(), "snake-plus", "0.1.0")?;
        store
            .install_from_directory(&InstallRequest {
                game_id: "snake-plus".to_string(),
                version: "0.1.0".to_string(),
                source: "local://fixtures".to_string(),
                artifact_dir: artifact,
                expected_sha256: None,
            })
            .map_err(|err| anyhow!(err.to_string()))?;

        std::fs::write(
            root.join("games/snake-plus/0.1.0/payload.txt"),
            "tampered content",
        )?;

        let outcome = store
            .verify_game("snake-plus")
            .map_err(|err| anyhow!(err.to_string()))?;
        assert!(!outcome.verified);
        Ok(())
    }

    #[test]
    fn remove_uninstalls_game_and_cleans_permissions() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let root = PathBuf::from(temp.path());
        let store = JsonContentStore::new(root.clone());

        let artifact = create_artifact_dir(root.as_path(), "snake-plus", "0.1.0")?;
        store
            .install_from_directory(&InstallRequest {
                game_id: "snake-plus".to_string(),
                version: "0.1.0".to_string(),
                source: "local://fixtures".to_string(),
                artifact_dir: artifact,
                expected_sha256: None,
            })
            .map_err(|err| anyhow!(err.to_string()))?;

        let mut permissions = store.load_permissions()?;
        permissions.grants.insert(
            "snake-plus".to_string(),
            vec![PermissionGrant {
                capability: Capability::TerminalRawInput,
                scope: Scope::None,
                decision: Decision::Allow,
                remembered: true,
                granted_at: chrono::Utc::now(),
            }],
        );
        store.save_permissions(&permissions)?;

        let outcome = store
            .remove_game("snake-plus")
            .map_err(|err| anyhow!(err.to_string()))?;

        assert_eq!(outcome.game_id, "snake-plus");
        assert_eq!(outcome.removed_version, "0.1.0");
        assert!(!outcome.had_versions_remaining);
        assert_eq!(store.read_current_pointer("snake-plus"), None);
        assert!(!root.join("games/snake-plus").exists());

        let installed = store.load_installed()?;
        assert!(
            !installed
                .installed
                .iter()
                .any(|item| item.id == "snake-plus")
        );

        let permissions = store.load_permissions()?;
        assert!(!permissions.grants.contains_key("snake-plus"));
        Ok(())
    }

    #[test]
    fn remove_rejects_builtin_game_records() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let store = JsonContentStore::new(PathBuf::from(temp.path()));

        let mut installed = store.load_installed()?;
        installed.installed.push(super::InstalledRecord {
            id: "snake-plus".to_string(),
            source: "builtin://dark-forest".to_string(),
            current_version: "0.1.0".to_string(),
            installed_versions: vec!["0.1.0".to_string()],
            version_checksums: BTreeMap::new(),
        });
        store.save_installed(&installed)?;

        let result = store.remove_game("snake-plus");
        assert!(matches!(
            result,
            Err(super::ContentTransactionError::ProtectedSource { .. })
        ));
        Ok(())
    }

    #[test]
    fn legacy_installed_record_is_upgraded_in_memory() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let root = PathBuf::from(temp.path());
        let store = JsonContentStore::new(root.clone());
        store.ensure_layout()?;

        let legacy = serde_json::json!({
            "schema_version": 1,
            "installed": [
                {
                    "id": "snake-plus",
                    "source": "builtin://dark-forest",
                    "current_version": "0.1.0"
                }
            ]
        });
        std::fs::write(
            root.join("installed.json"),
            serde_json::to_vec_pretty(&legacy)?,
        )?;

        let installed = store.load_installed()?;
        let record = installed
            .installed
            .iter()
            .find(|item| item.id == "snake-plus")
            .ok_or_else(|| anyhow!("missing migrated record"))?;

        assert!(record.installed_versions.iter().any(|item| item == "0.1.0"));
        Ok(())
    }

    #[test]
    fn permissions_store_round_trip() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let store = JsonContentStore::new(PathBuf::from(temp.path()));

        let mut permissions = store.load_permissions()?;
        permissions.grants.insert(
            "snake-plus".to_string(),
            vec![PermissionGrant {
                capability: Capability::TerminalRawInput,
                scope: Scope::None,
                decision: Decision::Allow,
                remembered: true,
                granted_at: chrono::Utc::now(),
            }],
        );

        store.save_permissions(&permissions)?;
        let reloaded = store.load_permissions()?;

        let grants = reloaded
            .grants
            .get("snake-plus")
            .ok_or_else(|| anyhow!("missing grants"))?;
        assert_eq!(grants.len(), 1);
        assert_eq!(grants[0].capability, Capability::TerminalRawInput);
        Ok(())
    }

    #[test]
    fn update_drops_grants_for_removed_capabilities() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let store = JsonContentStore::new(temp.path().to_path_buf());
        store.ensure_layout()?;

        let artifact_v1 = create_artifact_dir_with_permissions(
            temp.path(),
            "remote-wasm",
            "0.1.0",
            serde_json::json!(["terminal.raw_input", "net"]),
        )?;
        let _ = store.install_from_directory(&InstallRequest {
            game_id: "remote-wasm".to_string(),
            version: "0.1.0".to_string(),
            source: "index://fixture".to_string(),
            artifact_dir: artifact_v1,
            expected_sha256: None,
        })?;

        let mut permissions = store.load_permissions()?;
        permissions.grants.insert(
            "remote-wasm".to_string(),
            vec![
                PermissionGrant {
                    capability: Capability::TerminalRawInput,
                    scope: Scope::None,
                    decision: Decision::Allow,
                    remembered: true,
                    granted_at: chrono::Utc::now(),
                },
                PermissionGrant {
                    capability: Capability::Net,
                    scope: Scope::Allowlist(Vec::new()),
                    decision: Decision::Allow,
                    remembered: true,
                    granted_at: chrono::Utc::now(),
                },
            ],
        );
        store.save_permissions(&permissions)?;

        let artifact_v2 = create_artifact_dir_with_permissions(
            temp.path(),
            "remote-wasm",
            "0.2.0",
            serde_json::json!(["terminal.raw_input"]),
        )?;
        let _ = store.install_from_directory(&InstallRequest {
            game_id: "remote-wasm".to_string(),
            version: "0.2.0".to_string(),
            source: "index://fixture".to_string(),
            artifact_dir: artifact_v2,
            expected_sha256: None,
        })?;

        let reloaded = store.load_permissions()?;
        let grants = reloaded
            .grants
            .get("remote-wasm")
            .ok_or_else(|| anyhow!("missing grants"))?;
        assert_eq!(grants.len(), 1);
        assert_eq!(grants[0].capability, Capability::TerminalRawInput);
        Ok(())
    }

    #[test]
    fn update_preserves_grants_with_compatible_scope() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let store = JsonContentStore::new(temp.path().to_path_buf());
        store.ensure_layout()?;

        let declared_permission = serde_json::json!([
            {"capability":"net","scope":["api.example.com"]}
        ]);
        let artifact_v1 = create_artifact_dir_with_permissions(
            temp.path(),
            "remote-wasm",
            "0.1.0",
            declared_permission.clone(),
        )?;
        let _ = store.install_from_directory(&InstallRequest {
            game_id: "remote-wasm".to_string(),
            version: "0.1.0".to_string(),
            source: "index://fixture".to_string(),
            artifact_dir: artifact_v1,
            expected_sha256: None,
        })?;

        let mut permissions = store.load_permissions()?;
        permissions.grants.insert(
            "remote-wasm".to_string(),
            vec![PermissionGrant {
                capability: Capability::Net,
                scope: Scope::Allowlist(vec!["api.example.com".to_string()]),
                decision: Decision::Allow,
                remembered: true,
                granted_at: chrono::Utc::now(),
            }],
        );
        store.save_permissions(&permissions)?;

        let artifact_v2 = create_artifact_dir_with_permissions(
            temp.path(),
            "remote-wasm",
            "0.2.0",
            declared_permission,
        )?;
        let _ = store.install_from_directory(&InstallRequest {
            game_id: "remote-wasm".to_string(),
            version: "0.2.0".to_string(),
            source: "index://fixture".to_string(),
            artifact_dir: artifact_v2,
            expected_sha256: None,
        })?;

        let reloaded = store.load_permissions()?;
        let grants = reloaded
            .grants
            .get("remote-wasm")
            .ok_or_else(|| anyhow!("missing grants"))?;
        assert_eq!(grants.len(), 1);
        assert_eq!(grants[0].capability, Capability::Net);
        assert_eq!(
            grants[0].scope,
            Scope::Allowlist(vec!["api.example.com".to_string()])
        );
        Ok(())
    }

    fn create_artifact_dir(root: &Path, id: &str, version: &str) -> Result<PathBuf> {
        create_artifact_dir_with_permissions(
            root,
            id,
            version,
            serde_json::json!(["terminal.raw_input"]),
        )
    }

    fn create_artifact_dir_with_permissions(
        root: &Path,
        id: &str,
        version: &str,
        permissions: serde_json::Value,
    ) -> Result<PathBuf> {
        let dir = root.join(format!("artifact-{id}-{version}"));
        std::fs::create_dir_all(&dir)?;

        let manifest = serde_json::json!({
            "id": id,
            "name": "Test Game",
            "version": version,
            "author": "Dark Forest",
            "entry_type": "wasm",
            "entry": "main.wasm",
            "host_api": "^0.1",
            "permissions": permissions
        });

        std::fs::write(dir.join("game.json"), serde_json::to_vec_pretty(&manifest)?)?;
        std::fs::write(dir.join("payload.txt"), format!("payload-{version}"))?;
        Ok(dir)
    }
}
