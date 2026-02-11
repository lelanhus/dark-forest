use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use base64::Engine;
use chrono::{DateTime, Utc};
use flate2::{Compression, GzBuilder};
use plugin_host::{Capability, Decision, EntryType, Scope};
use registry::{parse_manifest, unpack_tarball_to_dir};
use ring::rand::SystemRandom;
use ring::signature::{self, Ed25519KeyPair, KeyPair};
use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tar::{Builder, EntryType as TarEntryType, Header};

pub const CREATOR_METADATA_SCHEMA_VERSION: u32 = 2;
pub const CURRENT_HOST_API_VERSION: &str = "0.1.0";
const WASM_BASIC_TEMPLATE_README: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../templates/wasm-basic/README.md"
));
const WASM_BASIC_TEMPLATE_WASM: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../templates/wasm-basic/main.wasm"
));

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackRequest {
    pub game_dir: PathBuf,
    pub out: Option<PathBuf>,
    pub metadata_out: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackOutcome {
    pub artifact_path: PathBuf,
    pub metadata_path: PathBuf,
    pub metadata: PackageMetadata,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifyRequest {
    pub artifact_path: PathBuf,
    pub metadata_path: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifyOutcome {
    pub artifact_path: PathBuf,
    pub metadata_path: PathBuf,
    pub game_id: String,
    pub version: String,
    pub artifact_sha256: String,
    pub artifact_size_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishRequest {
    pub artifact_path: PathBuf,
    pub metadata_path: Option<PathBuf>,
    pub index_locator: String,
    pub dry_run: bool,
    pub replace_existing: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishOutcome {
    pub index_path: PathBuf,
    pub game_id: String,
    pub version: String,
    pub artifact_path: PathBuf,
    pub created_game: bool,
    pub created_version: bool,
    pub replaced_existing_version: bool,
    pub dry_run: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DevRequest {
    pub game_dir: PathBuf,
    pub out: Option<PathBuf>,
    pub metadata_out: Option<PathBuf>,
    pub index_locator: String,
    pub dry_run_publish: bool,
    pub replace_existing: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DevCycleOutcome {
    pub pack: PackOutcome,
    pub verify: VerifyOutcome,
    pub publish: PublishOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateInitRequest {
    pub game_dir: PathBuf,
    pub game_id: Option<String>,
    pub name: Option<String>,
    pub author: Option<String>,
    pub version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateInitOutcome {
    pub game_dir: PathBuf,
    pub manifest_path: PathBuf,
    pub entry_path: PathBuf,
    pub readme_path: PathBuf,
    pub game_id: String,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeygenRequest {
    pub publisher_id: String,
    pub out_dir: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeygenOutcome {
    pub publisher_id: String,
    pub private_key_path: PathBuf,
    pub public_key_base64: String,
    pub public_key_fingerprint: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignRequest {
    pub artifact_path: PathBuf,
    pub metadata_path: Option<PathBuf>,
    pub publisher_id: String,
    pub private_key_path: PathBuf,
    pub signature_out: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignOutcome {
    pub artifact_path: PathBuf,
    pub metadata_path: PathBuf,
    pub signature_path: PathBuf,
    pub publisher_id: String,
    pub signature_base64: String,
    pub public_key_fingerprint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PackageMetadata {
    pub schema_version: u32,
    pub game_id: String,
    pub version: String,
    pub entry_type: String,
    pub host_api: String,
    pub artifact_file: String,
    pub artifact_sha256: String,
    pub artifact_size_bytes: u64,
    pub generated_at: DateTime<Utc>,
    #[serde(default)]
    pub signature_alg: Option<String>,
    #[serde(default)]
    pub signature: Option<String>,
    #[serde(default)]
    pub publisher_id: Option<String>,
    #[serde(default)]
    pub public_key_fingerprint: Option<String>,
}

pub fn pack_game(request: &PackRequest) -> Result<PackOutcome> {
    if !request.game_dir.exists() || !request.game_dir.is_dir() {
        return Err(anyhow!(
            "game directory is invalid: {}",
            request.game_dir.display()
        ));
    }

    let manifest = load_manifest(&request.game_dir)?;
    let artifact_path = request
        .out
        .clone()
        .unwrap_or_else(|| default_artifact_path(&manifest.id, &manifest.version));
    let metadata_path = request
        .metadata_out
        .clone()
        .map(Ok)
        .unwrap_or_else(|| default_metadata_path(&artifact_path))?;

    let tarball_bytes = build_deterministic_tarball(&request.game_dir)?;
    let artifact_sha256 = to_hex_lower(&Sha256::digest(&tarball_bytes));
    let artifact_size_bytes = u64::try_from(tarball_bytes.len())
        .map_err(|_| anyhow!("artifact is too large to represent size as u64"))?;

    atomic_write_bytes(&artifact_path, &tarball_bytes)?;

    let artifact_file = artifact_path
        .file_name()
        .map(|value| value.to_string_lossy().to_string())
        .ok_or_else(|| {
            anyhow!(
                "artifact path has no file name: {}",
                artifact_path.display()
            )
        })?;
    let metadata = PackageMetadata {
        schema_version: CREATOR_METADATA_SCHEMA_VERSION,
        game_id: manifest.id,
        version: manifest.version,
        entry_type: entry_type_label(manifest.entry_type)?,
        host_api: manifest.host_api,
        artifact_file,
        artifact_sha256,
        artifact_size_bytes,
        generated_at: Utc::now(),
        signature_alg: None,
        signature: None,
        publisher_id: None,
        public_key_fingerprint: None,
    };

    atomic_write_json(&metadata_path, &metadata)?;

    Ok(PackOutcome {
        artifact_path,
        metadata_path,
        metadata,
    })
}

pub fn verify_artifact(request: &VerifyRequest) -> Result<VerifyOutcome> {
    let metadata_path = request
        .metadata_path
        .clone()
        .map(Ok)
        .unwrap_or_else(|| default_metadata_path(&request.artifact_path))?;

    let metadata_raw = fs::read(&metadata_path)
        .with_context(|| format!("failed to read metadata {}", metadata_path.display()))?;
    let metadata: PackageMetadata = serde_json::from_slice(&metadata_raw)
        .with_context(|| format!("failed to parse metadata {}", metadata_path.display()))?;

    if metadata.schema_version != CREATOR_METADATA_SCHEMA_VERSION {
        return Err(anyhow!(
            "unsupported metadata schema version {} (expected {})",
            metadata.schema_version,
            CREATOR_METADATA_SCHEMA_VERSION
        ));
    }

    let artifact_file = request
        .artifact_path
        .file_name()
        .map(|value| value.to_string_lossy().to_string())
        .ok_or_else(|| {
            anyhow!(
                "artifact path has no file name: {}",
                request.artifact_path.display()
            )
        })?;
    if artifact_file != metadata.artifact_file {
        return Err(anyhow!(
            "artifact file mismatch (metadata={}, actual={artifact_file})",
            metadata.artifact_file
        ));
    }

    let artifact_bytes = fs::read(&request.artifact_path).with_context(|| {
        format!(
            "failed to read artifact for verification {}",
            request.artifact_path.display()
        )
    })?;
    let actual_sha256 = to_hex_lower(&Sha256::digest(&artifact_bytes));
    let actual_size = u64::try_from(artifact_bytes.len())
        .map_err(|_| anyhow!("artifact is too large to represent size as u64"))?;

    if actual_sha256 != metadata.artifact_sha256 {
        return Err(anyhow!(
            "artifact checksum mismatch (expected {}, actual {})",
            metadata.artifact_sha256,
            actual_sha256
        ));
    }
    if actual_size != metadata.artifact_size_bytes {
        return Err(anyhow!(
            "artifact size mismatch (expected {}, actual {})",
            metadata.artifact_size_bytes,
            actual_size
        ));
    }

    let temp = tempfile::tempdir().context("failed to create temporary verification directory")?;
    unpack_tarball_to_dir(&request.artifact_path, temp.path())
        .with_context(|| format!("failed to unpack {}", request.artifact_path.display()))?;

    let manifest = load_manifest(temp.path())?;
    let manifest_entry_type = entry_type_label(manifest.entry_type)?;
    if manifest.id != metadata.game_id {
        return Err(anyhow!(
            "manifest id mismatch (metadata={}, manifest={})",
            metadata.game_id,
            manifest.id
        ));
    }
    if manifest.version != metadata.version {
        return Err(anyhow!(
            "manifest version mismatch (metadata={}, manifest={})",
            metadata.version,
            manifest.version
        ));
    }
    if manifest_entry_type != metadata.entry_type {
        return Err(anyhow!(
            "manifest entry_type mismatch (metadata={}, manifest={})",
            metadata.entry_type,
            manifest_entry_type
        ));
    }
    if manifest.host_api != metadata.host_api {
        return Err(anyhow!(
            "manifest host_api mismatch (metadata={}, manifest={})",
            metadata.host_api,
            manifest.host_api
        ));
    }

    Ok(VerifyOutcome {
        artifact_path: request.artifact_path.clone(),
        metadata_path,
        game_id: metadata.game_id,
        version: metadata.version,
        artifact_sha256: actual_sha256,
        artifact_size_bytes: actual_size,
    })
}

pub fn publish_to_index(request: &PublishRequest) -> Result<PublishOutcome> {
    if request.index_locator.starts_with("http://") || request.index_locator.starts_with("https://")
    {
        return Err(anyhow!(
            "publish currently supports only local index locators (file:// or path)"
        ));
    }

    let verify_outcome = verify_artifact(&VerifyRequest {
        artifact_path: request.artifact_path.clone(),
        metadata_path: request.metadata_path.clone(),
    })?;
    let metadata_raw = fs::read(&verify_outcome.metadata_path).with_context(|| {
        format!(
            "failed to read metadata {}",
            verify_outcome.metadata_path.display()
        )
    })?;
    let metadata: PackageMetadata = serde_json::from_slice(&metadata_raw).with_context(|| {
        format!(
            "failed to parse metadata {}",
            verify_outcome.metadata_path.display()
        )
    })?;
    let publisher_id = metadata
        .publisher_id
        .clone()
        .ok_or_else(|| anyhow!("publish requires signed metadata with publisher_id"))?;
    let signature = metadata
        .signature
        .clone()
        .ok_or_else(|| anyhow!("publish requires signed metadata with signature"))?;
    let signature_fingerprint = metadata
        .public_key_fingerprint
        .clone()
        .ok_or_else(|| anyhow!("publish requires signed metadata with public_key_fingerprint"))?;
    let signature_alg = metadata
        .signature_alg
        .clone()
        .ok_or_else(|| anyhow!("publish requires signed metadata with signature_alg"))?;
    if signature_alg != "ed25519" {
        return Err(anyhow!(
            "publish requires ed25519 signature_alg (found {signature_alg})"
        ));
    }

    let temp = tempfile::tempdir().context("failed to create temporary publish directory")?;
    unpack_tarball_to_dir(&request.artifact_path, temp.path())
        .with_context(|| format!("failed to unpack {}", request.artifact_path.display()))?;
    let manifest = load_manifest(temp.path())?;
    enforce_publish_quality_gates(temp.path(), &manifest)?;

    let index_path = locator_to_path(&request.index_locator);
    let index_dir = index_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    if !request.dry_run {
        fs::create_dir_all(&index_dir).with_context(|| {
            format!(
                "failed to ensure index directory exists {}",
                index_dir.display()
            )
        })?;
    }

    let artifact_file_name = request
        .artifact_path
        .file_name()
        .map(|value| value.to_string_lossy().to_string())
        .ok_or_else(|| {
            anyhow!(
                "artifact path has no file name: {}",
                request.artifact_path.display()
            )
        })?;
    let published_artifact_path = index_dir.join(&artifact_file_name);
    let signature_file_name = format!("{artifact_file_name}.sig");
    let published_signature_path = index_dir.join(&signature_file_name);
    if !request.dry_run {
        stage_published_artifact(
            &request.artifact_path,
            &published_artifact_path,
            &verify_outcome.artifact_sha256,
            request.replace_existing,
        )?;
        atomic_write_bytes(&published_signature_path, signature.as_bytes())?;
    }

    let mut catalog = load_index_catalog(&index_path)?;

    let summary = summarize_permissions(&manifest.permissions);
    let version_entry = IndexVersion {
        version: manifest.version.clone(),
        artifact: artifact_file_name,
        artifact_sha256: Some(verify_outcome.artifact_sha256.clone()),
        signature_uri: Some(signature_file_name),
        publisher_id: Some(publisher_id.clone()),
        signature_fingerprint: Some(signature_fingerprint),
        size_bytes: Some(verify_outcome.artifact_size_bytes),
        entry_type: Some(manifest.entry_type),
        host_api: Some(manifest.host_api.clone()),
        permissions: manifest
            .permissions
            .iter()
            .map(permission_to_json)
            .collect::<Vec<_>>(),
    };

    let mut created_game = false;
    let mut created_version = false;
    let mut replaced_existing_version = false;

    if let Some(existing) = catalog.games.iter_mut().find(|item| item.id == manifest.id) {
        if let Some(existing_version) = existing
            .versions
            .iter_mut()
            .find(|item| item.version == manifest.version)
        {
            if existing_version.artifact_sha256.as_deref() == Some(&verify_outcome.artifact_sha256)
            {
                // idempotent publish for identical artifact/version
            } else if request.replace_existing {
                *existing_version = version_entry.clone();
                replaced_existing_version = true;
            } else {
                return Err(anyhow!(
                    "version already exists with different checksum: {}@{}",
                    manifest.id,
                    manifest.version
                ));
            }
        } else {
            existing.versions.push(version_entry);
            created_version = true;
        }
        existing.name = manifest.name.clone();
        existing.author = manifest.author.clone();
        existing.permissions_summary = summary;
        existing.host_api_range = manifest.host_api.clone();
        existing.entry_type = Some(manifest.entry_type);
        existing.publisher_id = Some(publisher_id.clone());
        existing.verified = true;
    } else {
        catalog.games.push(IndexGame {
            id: manifest.id.clone(),
            name: manifest.name.clone(),
            description: String::new(),
            tags: Vec::new(),
            author: manifest.author.clone(),
            permissions_summary: summary,
            host_api_range: manifest.host_api.clone(),
            entry_type: Some(manifest.entry_type),
            verified: true,
            publisher_id: Some(publisher_id),
            collections: Vec::new(),
            compatibility: None,
            versions: vec![version_entry],
        });
        created_game = true;
        created_version = true;
    }

    sort_catalog(&mut catalog);
    if !request.dry_run {
        atomic_write_json(&index_path, &catalog)?;
    }

    Ok(PublishOutcome {
        index_path,
        game_id: verify_outcome.game_id,
        version: verify_outcome.version,
        artifact_path: published_artifact_path,
        created_game,
        created_version,
        replaced_existing_version,
        dry_run: request.dry_run,
    })
}

pub fn run_dev_cycle(request: &DevRequest) -> Result<DevCycleOutcome> {
    let pack = pack_game(&PackRequest {
        game_dir: request.game_dir.clone(),
        out: request.out.clone(),
        metadata_out: request.metadata_out.clone(),
    })?;
    let verify = verify_artifact(&VerifyRequest {
        artifact_path: pack.artifact_path.clone(),
        metadata_path: Some(pack.metadata_path.clone()),
    })?;
    let keygen = keygen_publisher(&KeygenRequest {
        publisher_id: "local-dev".to_string(),
        out_dir: request.game_dir.join(".dark-forest-dev"),
    })?;
    let _sign = sign_artifact(&SignRequest {
        artifact_path: pack.artifact_path.clone(),
        metadata_path: Some(pack.metadata_path.clone()),
        publisher_id: "local-dev".to_string(),
        private_key_path: keygen.private_key_path,
        signature_out: None,
    })?;
    let publish = publish_to_index(&PublishRequest {
        artifact_path: pack.artifact_path.clone(),
        metadata_path: Some(pack.metadata_path.clone()),
        index_locator: request.index_locator.clone(),
        dry_run: request.dry_run_publish,
        replace_existing: request.replace_existing,
    })?;
    Ok(DevCycleOutcome {
        pack,
        verify,
        publish,
    })
}

pub fn init_wasm_template(request: &TemplateInitRequest) -> Result<TemplateInitOutcome> {
    ensure_directory_ready(&request.game_dir)?;

    let game_id = request
        .game_id
        .clone()
        .unwrap_or_else(|| derive_default_game_id(&request.game_dir));
    validate_game_id(&game_id)?;

    let name = request
        .name
        .clone()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| derive_default_name(&request.game_dir));
    let author = request
        .author
        .clone()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "Unknown Author".to_string());
    let requested_version = request
        .version
        .clone()
        .unwrap_or_else(|| "0.1.0".to_string());
    let version = Version::parse(&requested_version)
        .with_context(|| format!("template version must be valid semver: {requested_version}"))?
        .to_string();

    let manifest_path = request.game_dir.join("game.json");
    let entry_path = request.game_dir.join("main.wasm");
    let readme_path = request.game_dir.join("README.md");
    let manifest = serde_json::json!({
        "id": game_id,
        "name": name,
        "version": version,
        "author": author,
        "entry_type": "wasm",
        "entry": "main.wasm",
        "host_api": "^0.1",
        "permissions": []
    });

    atomic_write_json(&manifest_path, &manifest)?;
    atomic_write_bytes(&entry_path, WASM_BASIC_TEMPLATE_WASM)?;
    atomic_write_bytes(&readme_path, WASM_BASIC_TEMPLATE_README.as_bytes())?;

    Ok(TemplateInitOutcome {
        game_dir: request.game_dir.clone(),
        manifest_path,
        entry_path,
        readme_path,
        game_id: manifest["id"]
            .as_str()
            .map(ToString::to_string)
            .ok_or_else(|| anyhow!("generated manifest id is invalid"))?,
        version: manifest["version"]
            .as_str()
            .map(ToString::to_string)
            .ok_or_else(|| anyhow!("generated manifest version is invalid"))?,
    })
}

pub fn keygen_publisher(request: &KeygenRequest) -> Result<KeygenOutcome> {
    validate_game_id(&request.publisher_id)?;
    fs::create_dir_all(&request.out_dir).with_context(|| {
        format!(
            "failed to create publisher key output directory {}",
            request.out_dir.display()
        )
    })?;

    let rng = SystemRandom::new();
    let pkcs8 = Ed25519KeyPair::generate_pkcs8(&rng)
        .map_err(|_| anyhow!("failed to generate ed25519 keypair"))?;
    let key_pair =
        Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).map_err(|_| anyhow!("invalid ed25519 pkcs8"))?;

    let private_key_path = request
        .out_dir
        .join(format!("{}.ed25519.pk8", request.publisher_id));
    atomic_write_bytes(&private_key_path, pkcs8.as_ref())?;

    let public_key_base64 =
        base64::engine::general_purpose::STANDARD.encode(key_pair.public_key().as_ref());
    let public_key_fingerprint = fingerprint_public_key_bytes(key_pair.public_key().as_ref());

    Ok(KeygenOutcome {
        publisher_id: request.publisher_id.clone(),
        private_key_path,
        public_key_base64,
        public_key_fingerprint,
    })
}

pub fn sign_artifact(request: &SignRequest) -> Result<SignOutcome> {
    let verify = verify_artifact(&VerifyRequest {
        artifact_path: request.artifact_path.clone(),
        metadata_path: request.metadata_path.clone(),
    })?;

    let metadata_raw = fs::read(&verify.metadata_path)
        .with_context(|| format!("failed to read metadata {}", verify.metadata_path.display()))?;
    let mut metadata: PackageMetadata =
        serde_json::from_slice(&metadata_raw).with_context(|| {
            format!(
                "failed to parse metadata {}",
                verify.metadata_path.display()
            )
        })?;

    let private_key = fs::read(&request.private_key_path).with_context(|| {
        format!(
            "failed to read private key {}",
            request.private_key_path.display()
        )
    })?;
    let key_pair =
        Ed25519KeyPair::from_pkcs8(&private_key).map_err(|_| anyhow!("invalid ed25519 pkcs8"))?;
    let public_key_fingerprint = fingerprint_public_key_bytes(key_pair.public_key().as_ref());

    let signature = key_pair.sign(verify.artifact_sha256.as_bytes());
    let signature_base64 = base64::engine::general_purpose::STANDARD.encode(signature.as_ref());

    let signature_path = request
        .signature_out
        .clone()
        .unwrap_or_else(|| default_signature_path(&request.artifact_path));
    atomic_write_bytes(&signature_path, signature_base64.as_bytes())?;

    metadata.schema_version = CREATOR_METADATA_SCHEMA_VERSION;
    metadata.signature_alg = Some("ed25519".to_string());
    metadata.signature = Some(signature_base64.clone());
    metadata.publisher_id = Some(request.publisher_id.clone());
    metadata.public_key_fingerprint = Some(public_key_fingerprint.clone());
    atomic_write_json(&verify.metadata_path, &metadata)?;

    Ok(SignOutcome {
        artifact_path: request.artifact_path.clone(),
        metadata_path: verify.metadata_path,
        signature_path,
        publisher_id: request.publisher_id.clone(),
        signature_base64,
        public_key_fingerprint,
    })
}

pub fn verify_signature(metadata: &PackageMetadata, public_key_base64: &str) -> Result<()> {
    let signature_alg = metadata
        .signature_alg
        .as_deref()
        .ok_or_else(|| anyhow!("metadata signature_alg is missing"))?;
    if signature_alg != "ed25519" {
        return Err(anyhow!("unsupported signature algorithm: {signature_alg}"));
    }

    let signature_b64 = metadata
        .signature
        .as_deref()
        .ok_or_else(|| anyhow!("metadata signature is missing"))?;
    let signature = base64::engine::general_purpose::STANDARD
        .decode(signature_b64)
        .with_context(|| "metadata signature is not valid base64")?;
    let public_key = base64::engine::general_purpose::STANDARD
        .decode(public_key_base64)
        .with_context(|| "publisher public key is not valid base64")?;

    if let Some(expected_fingerprint) = &metadata.public_key_fingerprint {
        let actual_fingerprint = fingerprint_public_key_bytes(&public_key);
        if &actual_fingerprint != expected_fingerprint {
            return Err(anyhow!(
                "public key fingerprint mismatch (expected {}, actual {})",
                expected_fingerprint,
                actual_fingerprint
            ));
        }
    }

    let verifier = signature::UnparsedPublicKey::new(&signature::ED25519, &public_key);
    verifier
        .verify(metadata.artifact_sha256.as_bytes(), &signature)
        .map_err(|_| anyhow!("signature verification failed"))
}

pub fn public_key_fingerprint_from_base64(public_key_base64: &str) -> Result<String> {
    let public_key = base64::engine::general_purpose::STANDARD
        .decode(public_key_base64)
        .with_context(|| "publisher public key is not valid base64")?;
    Ok(fingerprint_public_key_bytes(&public_key))
}

pub fn game_dir_signature(game_dir: &Path) -> Result<String> {
    if !game_dir.exists() || !game_dir.is_dir() {
        return Err(anyhow!("game directory is invalid: {}", game_dir.display()));
    }

    let mut files = Vec::new();
    collect_regular_files(game_dir, game_dir, &mut files)?;
    files.sort_by_key(|path| normalize_relative_path(path));

    let mut hasher = Sha256::new();
    for relative in files {
        let relative_normalized = normalize_relative_path(&relative);
        hasher.update(relative_normalized.as_bytes());
        hasher.update([0_u8]);
        let bytes = fs::read(game_dir.join(&relative))
            .with_context(|| format!("failed to read {}", game_dir.join(&relative).display()))?;
        hasher.update(bytes);
    }

    Ok(to_hex_lower(&hasher.finalize()))
}

pub fn default_artifact_path(game_id: &str, version: &str) -> PathBuf {
    PathBuf::from("dist").join(format!("{game_id}-{version}.tar.gz"))
}

pub fn default_signature_path(artifact_path: &Path) -> PathBuf {
    let file_name = artifact_path
        .file_name()
        .map(|value| value.to_string_lossy().to_string())
        .unwrap_or_else(|| "artifact.tar.gz".to_string());
    artifact_path.with_file_name(format!("{file_name}.sig"))
}

pub fn default_metadata_path(artifact_path: &Path) -> Result<PathBuf> {
    let file_name = artifact_path
        .file_name()
        .ok_or_else(|| {
            anyhow!(
                "artifact path has no file name: {}",
                artifact_path.display()
            )
        })?
        .to_string_lossy()
        .to_string();
    let stem = if let Some(stripped) = file_name.strip_suffix(".tar.gz") {
        stripped.to_string()
    } else if let Some((name, _)) = file_name.rsplit_once('.') {
        name.to_string()
    } else {
        file_name
    };
    let metadata_name = format!("{stem}.metadata.json");
    Ok(artifact_path.with_file_name(metadata_name))
}

fn ensure_directory_ready(path: &Path) -> Result<()> {
    if path.exists() {
        let metadata = fs::metadata(path)
            .with_context(|| format!("failed to inspect template directory {}", path.display()))?;
        if !metadata.is_dir() {
            return Err(anyhow!(
                "template target path is not a directory: {}",
                path.display()
            ));
        }
        let mut entries = fs::read_dir(path)
            .with_context(|| format!("failed to read template directory {}", path.display()))?;
        if entries.next().transpose()?.is_some() {
            return Err(anyhow!(
                "template target directory must be empty: {}",
                path.display()
            ));
        }
        return Ok(());
    }

    fs::create_dir_all(path)
        .with_context(|| format!("failed to create template directory {}", path.display()))
}

fn derive_default_game_id(game_dir: &Path) -> String {
    let raw = game_dir
        .file_name()
        .map(|value| value.to_string_lossy().to_string())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "example-wasm-game".to_string());
    let mut normalized = String::with_capacity(raw.len());
    let mut previous_dash = false;
    for ch in raw.chars() {
        if ch.is_ascii_alphanumeric() {
            normalized.push(ch.to_ascii_lowercase());
            previous_dash = false;
            continue;
        }
        if matches!(ch, '.' | '_' | '-') {
            normalized.push(ch);
            previous_dash = ch == '-';
            continue;
        }
        if !previous_dash {
            normalized.push('-');
            previous_dash = true;
        }
    }

    let trimmed = normalized
        .trim_matches('-')
        .trim_matches('.')
        .trim_matches('_')
        .to_string();
    if trimmed.is_empty() {
        "example-wasm-game".to_string()
    } else {
        trimmed
    }
}

fn derive_default_name(game_dir: &Path) -> String {
    game_dir
        .file_name()
        .map(|value| value.to_string_lossy().to_string())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "Example WASM Game".to_string())
}

fn validate_game_id(game_id: &str) -> Result<()> {
    if game_id.trim().is_empty() {
        return Err(anyhow!("game id must not be empty"));
    }
    if !game_id
        .chars()
        .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || matches!(ch, '.' | '_' | '-'))
    {
        return Err(anyhow!(
            "game id must match [a-z0-9._-]+ (received {game_id})"
        ));
    }
    Ok(())
}

fn load_manifest(game_dir: &Path) -> Result<registry::Manifest> {
    let manifest_path = game_dir.join("game.json");
    let manifest_raw = fs::read_to_string(&manifest_path)
        .with_context(|| format!("failed to read manifest {}", manifest_path.display()))?;
    parse_manifest(&manifest_raw)
        .with_context(|| format!("failed to parse manifest {}", manifest_path.display()))
}

fn load_manifest_json(game_dir: &Path) -> Result<serde_json::Value> {
    let manifest_path = game_dir.join("game.json");
    let manifest_raw = fs::read_to_string(&manifest_path)
        .with_context(|| format!("failed to read manifest {}", manifest_path.display()))?;
    serde_json::from_str(&manifest_raw)
        .with_context(|| format!("failed to parse manifest json {}", manifest_path.display()))
}

fn enforce_publish_quality_gates(game_dir: &Path, manifest: &registry::Manifest) -> Result<()> {
    let manifest_json = load_manifest_json(game_dir)?;
    let object = manifest_json
        .as_object()
        .ok_or_else(|| anyhow!("manifest root must be a JSON object"))?;
    let permissions = object.get("permissions").ok_or_else(|| {
        anyhow!("manifest must declare a permissions field for marketplace publish")
    })?;
    if !permissions.is_array() {
        return Err(anyhow!(
            "manifest permissions field must be an array for marketplace publish"
        ));
    }

    let host_api_range = VersionReq::parse(&manifest.host_api).with_context(|| {
        format!(
            "manifest host_api '{}' is not a valid semver range",
            manifest.host_api
        )
    })?;
    let host_version = Version::parse(CURRENT_HOST_API_VERSION).with_context(|| {
        format!(
            "current host api version '{}' is not valid semver",
            CURRENT_HOST_API_VERSION
        )
    })?;
    if !host_api_range.matches(&host_version) {
        return Err(anyhow!(
            "manifest host_api '{}' is incompatible with host api {}",
            manifest.host_api,
            CURRENT_HOST_API_VERSION
        ));
    }

    Ok(())
}

fn build_deterministic_tarball(game_dir: &Path) -> Result<Vec<u8>> {
    let mut files = Vec::new();
    collect_regular_files(game_dir, game_dir, &mut files)?;
    files.sort_by_key(|path| normalize_relative_path(path));

    let mut encoder = GzBuilder::new()
        .mtime(0)
        .write(Vec::new(), Compression::default());
    {
        let mut tar = Builder::new(&mut encoder);
        tar.mode(tar::HeaderMode::Deterministic);

        for relative in files {
            let absolute = game_dir.join(&relative);
            let bytes = fs::read(&absolute)
                .with_context(|| format!("failed to read {}", absolute.display()))?;
            let archive_path = normalize_relative_path(&relative);
            let mut header = Header::new_gnu();
            header.set_entry_type(TarEntryType::Regular);
            header.set_size(
                u64::try_from(bytes.len())
                    .map_err(|_| anyhow!("file too large to archive: {}", absolute.display()))?,
            );
            header.set_mode(0o644);
            header.set_uid(0);
            header.set_gid(0);
            header.set_mtime(0);
            let _ = header.set_username("");
            let _ = header.set_groupname("");
            header.set_cksum();
            tar.append_data(&mut header, archive_path, bytes.as_slice())
                .with_context(|| {
                    format!("failed to append archive entry {}", absolute.display())
                })?;
        }

        tar.finish().context("failed to finalize tar archive")?;
    }

    encoder
        .finish()
        .context("failed to finalize deterministic gzip stream")
}

fn collect_regular_files(root: &Path, current: &Path, output: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(current)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_regular_files(root, &path, output)?;
            continue;
        }
        if path.is_file() {
            let relative = path
                .strip_prefix(root)
                .map(Path::to_path_buf)
                .map_err(|err| anyhow!("failed to normalize path {}: {err}", path.display()))?;
            output.push(relative);
        }
    }
    Ok(())
}

fn normalize_relative_path(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy().to_string())
        .collect::<Vec<_>>()
        .join("/")
}

fn fingerprint_public_key_bytes(bytes: &[u8]) -> String {
    to_hex_lower(&Sha256::digest(bytes))
}

fn entry_type_label(value: impl Serialize) -> Result<String> {
    let serialized = serde_json::to_value(value).context("failed to serialize entry_type")?;
    serialized
        .as_str()
        .map(ToString::to_string)
        .ok_or_else(|| anyhow!("entry_type did not serialize as string"))
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

fn scope_to_json(scope: &Scope) -> serde_json::Value {
    match scope {
        Scope::None => serde_json::Value::Null,
        Scope::Prompt => serde_json::json!({"prompt": true}),
        Scope::Path(path) => serde_json::json!({"path": path}),
        Scope::Paths(paths) => serde_json::json!({"paths": paths}),
        Scope::Allowlist(values) => serde_json::json!({"allowlist": values}),
    }
}

fn permission_to_json(grant: &plugin_host::CapabilityGrant) -> serde_json::Value {
    serde_json::json!({
        "capability": capability_label(grant.capability),
        "scope": scope_to_json(&grant.scope),
        "decision": match grant.decision {
            Decision::Allow => "allow",
            Decision::Deny => "deny",
        }
    })
}

fn summarize_permissions(permissions: &[plugin_host::CapabilityGrant]) -> Vec<String> {
    let mut summary = Vec::new();
    for grant in permissions {
        let label = capability_label(grant.capability).to_string();
        if !summary.iter().any(|item| item == &label) {
            summary.push(label);
        }
    }
    summary
}

fn locator_to_path(locator: &str) -> PathBuf {
    if let Some(rest) = locator.strip_prefix("file://") {
        return PathBuf::from(rest);
    }
    PathBuf::from(locator)
}

fn stage_published_artifact(
    source: &Path,
    destination: &Path,
    expected_sha256: &str,
    replace_existing: bool,
) -> Result<()> {
    if source == destination {
        return Ok(());
    }

    if destination.exists() {
        let existing = fs::read(destination).with_context(|| {
            format!(
                "failed to read existing published artifact {}",
                destination.display()
            )
        })?;
        let existing_sha = to_hex_lower(&Sha256::digest(&existing));
        if existing_sha.eq_ignore_ascii_case(expected_sha256) {
            return Ok(());
        }
        if !replace_existing {
            return Err(anyhow!(
                "published artifact conflict at {}",
                destination.display()
            ));
        }
    }

    let bytes = fs::read(source)
        .with_context(|| format!("failed to read source artifact {}", source.display()))?;
    let actual_sha = to_hex_lower(&Sha256::digest(&bytes));
    if !actual_sha.eq_ignore_ascii_case(expected_sha256) {
        return Err(anyhow!(
            "source artifact checksum mismatch while publishing (expected {expected_sha256}, actual {actual_sha})"
        ));
    }
    atomic_write_bytes(destination, &bytes)
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
    compatibility: Option<IndexCompatibility>,
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
struct IndexCompatibility {
    host_api: String,
    permissions: String,
}

fn load_index_catalog(index_path: &Path) -> Result<IndexCatalog> {
    if !index_path.exists() {
        return Ok(IndexCatalog {
            schema_version: 2,
            games: Vec::new(),
        });
    }

    let raw = fs::read_to_string(index_path)
        .with_context(|| format!("failed to read index {}", index_path.display()))?;
    let mut parsed: IndexCatalog = serde_json::from_str(&raw)
        .with_context(|| format!("failed to parse index {}", index_path.display()))?;
    if parsed.schema_version == 0 {
        parsed.schema_version = 2;
    }
    Ok(parsed)
}

fn sort_catalog(catalog: &mut IndexCatalog) {
    catalog.games.sort_by(|a, b| a.id.cmp(&b.id));
    for game in &mut catalog.games {
        game.versions.sort_by(|a, b| b.version.cmp(&a.version));
    }
}

fn atomic_write_bytes(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let tmp = path.with_extension("tmp");
    let mut file = File::create(&tmp)
        .with_context(|| format!("failed to create temporary file {}", tmp.display()))?;
    file.write_all(bytes)
        .with_context(|| format!("failed to write temporary file {}", tmp.display()))?;
    file.sync_all()
        .with_context(|| format!("failed to sync temporary file {}", tmp.display()))?;
    drop(file);

    fs::rename(&tmp, path).with_context(|| {
        format!(
            "failed to atomically move {} to {}",
            tmp.display(),
            path.display()
        )
    })?;

    if let Some(parent) = path.parent()
        && let Ok(dir) = File::open(parent)
    {
        let _ = dir.sync_all();
    }

    Ok(())
}

fn atomic_write_json<T>(path: &Path, value: &T) -> Result<()>
where
    T: Serialize,
{
    let bytes = serde_json::to_vec_pretty(value)?;
    atomic_write_bytes(path, &bytes)
}

fn to_hex_lower(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push_str(&format!("{byte:02x}"));
    }
    output
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::{LazyLock, Mutex};

    use anyhow::{Context, Result};
    use serde_json::Value;

    use super::{
        DevRequest, KeygenRequest, PackRequest, PublishRequest, SignRequest, TemplateInitRequest,
        VerifyRequest, default_artifact_path, default_metadata_path, game_dir_signature,
        init_wasm_template, keygen_publisher, pack_game, publish_to_index, run_dev_cycle,
        sign_artifact, verify_artifact, verify_signature,
    };

    static TEST_CWD_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

    struct CurrentDirGuard {
        previous: PathBuf,
    }

    impl CurrentDirGuard {
        fn enter(target: &Path) -> Result<Self> {
            let previous = std::env::current_dir().context("failed to read current dir")?;
            std::env::set_current_dir(target)
                .with_context(|| format!("failed to set cwd to {}", target.display()))?;
            Ok(Self { previous })
        }
    }

    impl Drop for CurrentDirGuard {
        fn drop(&mut self) {
            let _ = std::env::set_current_dir(&self.previous);
        }
    }

    fn create_sample_game(root: &Path, game_id: &str, version: &str) -> Result<PathBuf> {
        let game_dir = root.join("sample-game");
        fs::create_dir_all(&game_dir)?;

        let manifest = serde_json::json!({
            "id": game_id,
            "name": "Sample Game",
            "version": version,
            "author": "Dark Forest",
            "entry_type": "wasm",
            "entry": "main.wasm",
            "host_api": "^0.1",
            "permissions": ["terminal.raw_input"]
        });
        fs::write(
            game_dir.join("game.json"),
            serde_json::to_vec_pretty(&manifest)?,
        )?;
        fs::write(game_dir.join("main.wasm"), b"wasm bytes")?;
        fs::write(game_dir.join("assets.txt"), b"asset payload")?;
        Ok(game_dir)
    }

    fn write_manifest_version(game_dir: &Path, game_id: &str, version: &str) -> Result<()> {
        let manifest = serde_json::json!({
            "id": game_id,
            "name": "Sample Game",
            "version": version,
            "author": "Dark Forest",
            "entry_type": "wasm",
            "entry": "main.wasm",
            "host_api": "^0.1",
            "permissions": ["terminal.raw_input"]
        });
        fs::write(
            game_dir.join("game.json"),
            serde_json::to_vec_pretty(&manifest)?,
        )?;
        Ok(())
    }

    fn write_manifest_without_permissions(
        game_dir: &Path,
        game_id: &str,
        version: &str,
    ) -> Result<()> {
        let manifest = serde_json::json!({
            "id": game_id,
            "name": "Sample Game",
            "version": version,
            "author": "Dark Forest",
            "entry_type": "wasm",
            "entry": "main.wasm",
            "host_api": "^0.1"
        });
        fs::write(
            game_dir.join("game.json"),
            serde_json::to_vec_pretty(&manifest)?,
        )?;
        Ok(())
    }

    fn write_manifest_host_api(
        game_dir: &Path,
        game_id: &str,
        version: &str,
        host_api: &str,
    ) -> Result<()> {
        let manifest = serde_json::json!({
            "id": game_id,
            "name": "Sample Game",
            "version": version,
            "author": "Dark Forest",
            "entry_type": "wasm",
            "entry": "main.wasm",
            "host_api": host_api,
            "permissions": ["terminal.raw_input"]
        });
        fs::write(
            game_dir.join("game.json"),
            serde_json::to_vec_pretty(&manifest)?,
        )?;
        Ok(())
    }

    fn write_wasm_payload(game_dir: &Path, payload: &[u8]) -> Result<()> {
        fs::write(game_dir.join("main.wasm"), payload)?;
        Ok(())
    }

    fn sign_for_publish(
        root: &Path,
        artifact_path: &Path,
        metadata_path: &Path,
    ) -> Result<super::KeygenOutcome> {
        let keygen = keygen_publisher(&KeygenRequest {
            publisher_id: "dark-forest".to_string(),
            out_dir: root.join("keys"),
        })?;
        sign_artifact(&SignRequest {
            artifact_path: artifact_path.to_path_buf(),
            metadata_path: Some(metadata_path.to_path_buf()),
            publisher_id: "dark-forest".to_string(),
            private_key_path: keygen.private_key_path.clone(),
            signature_out: None,
        })?;
        Ok(keygen)
    }

    #[test]
    fn init_wasm_template_creates_manifest_and_entry() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let game_dir = temp.path().join("space-blaster");

        let outcome = init_wasm_template(&TemplateInitRequest {
            game_dir: game_dir.clone(),
            game_id: Some("space-blaster".to_string()),
            name: Some("Space Blaster".to_string()),
            author: Some("Dark Forest".to_string()),
            version: Some("0.4.2".to_string()),
        })?;

        assert_eq!(outcome.game_dir, game_dir);
        assert!(outcome.manifest_path.exists());
        assert!(outcome.entry_path.exists());
        assert!(outcome.readme_path.exists());
        assert_eq!(outcome.game_id, "space-blaster");
        assert_eq!(outcome.version, "0.4.2");

        let manifest: Value = serde_json::from_slice(&fs::read(&outcome.manifest_path)?)?;
        assert_eq!(manifest["id"], "space-blaster");
        assert_eq!(manifest["name"], "Space Blaster");
        assert_eq!(manifest["version"], "0.4.2");
        assert_eq!(manifest["author"], "Dark Forest");
        assert_eq!(manifest["entry_type"], "wasm");
        assert_eq!(manifest["entry"], "main.wasm");
        assert_eq!(manifest["host_api"], "^0.1");
        assert_eq!(manifest["permissions"], serde_json::json!([]));
        Ok(())
    }

    #[test]
    fn init_wasm_template_rejects_nonempty_directory() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let game_dir = temp.path().join("existing");
        fs::create_dir_all(&game_dir)?;
        fs::write(game_dir.join("already.txt"), b"occupied")?;

        let result = init_wasm_template(&TemplateInitRequest {
            game_dir,
            game_id: Some("existing".to_string()),
            name: Some("Existing".to_string()),
            author: Some("Dark Forest".to_string()),
            version: Some("0.1.0".to_string()),
        });
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn init_wasm_template_derives_defaults_from_directory_name() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let game_dir = temp.path().join("My Cool Game");

        let outcome = init_wasm_template(&TemplateInitRequest {
            game_dir: game_dir.clone(),
            game_id: None,
            name: None,
            author: Some("Dark Forest".to_string()),
            version: None,
        })?;

        assert_eq!(outcome.game_id, "my-cool-game");
        assert_eq!(outcome.version, "0.1.0");
        let manifest: Value = serde_json::from_slice(&fs::read(game_dir.join("game.json"))?)?;
        assert_eq!(manifest["id"], "my-cool-game");
        assert_eq!(manifest["name"], "My Cool Game");
        assert_eq!(manifest["version"], "0.1.0");
        Ok(())
    }

    #[test]
    fn init_wasm_template_rejects_invalid_game_id() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let game_dir = temp.path().join("my-game");
        let result = init_wasm_template(&TemplateInitRequest {
            game_dir,
            game_id: Some("Invalid Game Id".to_string()),
            name: Some("My Game".to_string()),
            author: Some("Dark Forest".to_string()),
            version: Some("0.1.0".to_string()),
        });
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn init_wasm_template_rejects_invalid_version() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let game_dir = temp.path().join("my-game");
        let result = init_wasm_template(&TemplateInitRequest {
            game_dir,
            game_id: Some("my-game".to_string()),
            name: Some("My Game".to_string()),
            author: Some("Dark Forest".to_string()),
            version: Some("invalid".to_string()),
        });
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn pack_creates_tarball_and_metadata_with_expected_fields() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let game_dir = create_sample_game(temp.path(), "sample-game", "1.2.3")?;
        let artifact_path = temp.path().join("out").join("sample-game-1.2.3.tar.gz");
        let metadata_path = temp
            .path()
            .join("out")
            .join("sample-game-1.2.3.metadata.json");

        let outcome = pack_game(&PackRequest {
            game_dir,
            out: Some(artifact_path.clone()),
            metadata_out: Some(metadata_path.clone()),
        })?;

        assert!(artifact_path.exists());
        assert!(metadata_path.exists());
        assert_eq!(outcome.artifact_path, artifact_path);
        assert_eq!(outcome.metadata_path, metadata_path);

        let metadata_json: Value = serde_json::from_slice(&fs::read(&outcome.metadata_path)?)?;
        for field in [
            "schema_version",
            "game_id",
            "version",
            "entry_type",
            "host_api",
            "artifact_file",
            "artifact_sha256",
            "artifact_size_bytes",
            "generated_at",
        ] {
            assert!(
                metadata_json.get(field).is_some(),
                "expected metadata field {field}"
            );
        }
        Ok(())
    }

    #[test]
    fn pack_is_byte_deterministic_for_same_input() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let game_dir = create_sample_game(temp.path(), "sample-game", "1.2.3")?;
        let artifact_a = temp.path().join("out").join("first.tar.gz");
        let metadata_a = temp.path().join("out").join("first.metadata.json");
        let artifact_b = temp.path().join("out").join("second.tar.gz");
        let metadata_b = temp.path().join("out").join("second.metadata.json");

        pack_game(&PackRequest {
            game_dir: game_dir.clone(),
            out: Some(artifact_a.clone()),
            metadata_out: Some(metadata_a),
        })?;
        pack_game(&PackRequest {
            game_dir,
            out: Some(artifact_b.clone()),
            metadata_out: Some(metadata_b),
        })?;

        assert_eq!(fs::read(&artifact_a)?, fs::read(&artifact_b)?);
        Ok(())
    }

    #[test]
    fn verify_artifact_succeeds_for_matching_artifact_and_metadata() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let game_dir = create_sample_game(temp.path(), "sample-game", "1.2.3")?;
        let artifact_path = temp.path().join("out").join("sample-game-1.2.3.tar.gz");
        let metadata_path = temp
            .path()
            .join("out")
            .join("sample-game-1.2.3.metadata.json");

        pack_game(&PackRequest {
            game_dir,
            out: Some(artifact_path.clone()),
            metadata_out: Some(metadata_path.clone()),
        })?;

        let outcome = verify_artifact(&VerifyRequest {
            artifact_path,
            metadata_path: Some(metadata_path),
        })?;

        assert_eq!(outcome.game_id, "sample-game");
        assert_eq!(outcome.version, "1.2.3");
        Ok(())
    }

    #[test]
    fn verify_artifact_fails_on_checksum_mismatch() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let game_dir = create_sample_game(temp.path(), "sample-game", "1.2.3")?;
        let artifact_path = temp.path().join("out").join("sample-game-1.2.3.tar.gz");
        let metadata_path = temp
            .path()
            .join("out")
            .join("sample-game-1.2.3.metadata.json");

        pack_game(&PackRequest {
            game_dir,
            out: Some(artifact_path.clone()),
            metadata_out: Some(metadata_path.clone()),
        })?;

        fs::write(&artifact_path, b"tampered artifact payload")?;

        let result = verify_artifact(&VerifyRequest {
            artifact_path,
            metadata_path: Some(metadata_path),
        });
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn verify_artifact_fails_on_manifest_id_or_version_mismatch() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let game_dir = create_sample_game(temp.path(), "sample-game", "1.2.3")?;
        let artifact_path = temp.path().join("out").join("sample-game-1.2.3.tar.gz");
        let metadata_path = temp
            .path()
            .join("out")
            .join("sample-game-1.2.3.metadata.json");

        pack_game(&PackRequest {
            game_dir,
            out: Some(artifact_path.clone()),
            metadata_out: Some(metadata_path.clone()),
        })?;

        let mut metadata_json: Value = serde_json::from_slice(&fs::read(&metadata_path)?)?;
        metadata_json["version"] = Value::String("9.9.9".to_string());
        fs::write(&metadata_path, serde_json::to_vec_pretty(&metadata_json)?)?;

        let result = verify_artifact(&VerifyRequest {
            artifact_path,
            metadata_path: Some(metadata_path),
        });
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn keygen_and_sign_produce_verifiable_signature() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let game_dir = create_sample_game(temp.path(), "sample-game", "1.2.3")?;
        let artifact_path = temp.path().join("out").join("sample-game-1.2.3.tar.gz");
        let metadata_path = temp
            .path()
            .join("out")
            .join("sample-game-1.2.3.metadata.json");

        pack_game(&PackRequest {
            game_dir,
            out: Some(artifact_path.clone()),
            metadata_out: Some(metadata_path.clone()),
        })?;
        let keygen = sign_for_publish(temp.path(), &artifact_path, &metadata_path)?;

        let metadata_raw = fs::read(&metadata_path)?;
        let metadata: super::PackageMetadata = serde_json::from_slice(&metadata_raw)?;
        verify_signature(&metadata, &keygen.public_key_base64)?;
        assert_eq!(metadata.publisher_id.as_deref(), Some("dark-forest"));
        assert_eq!(
            metadata.public_key_fingerprint.as_deref(),
            Some(keygen.public_key_fingerprint.as_str())
        );
        Ok(())
    }

    #[test]
    fn pack_defaults_to_dist_when_out_not_provided() -> Result<()> {
        let _cwd_guard = TEST_CWD_LOCK.lock().expect("cwd lock must succeed");
        let temp = tempfile::tempdir()?;
        let game_dir = create_sample_game(temp.path(), "sample-game", "1.2.3")?;
        let _dir_guard = CurrentDirGuard::enter(temp.path())?;

        let expected_artifact = default_artifact_path("sample-game", "1.2.3");
        let expected_metadata = default_metadata_path(&expected_artifact)?;

        let outcome = pack_game(&PackRequest {
            game_dir,
            out: None,
            metadata_out: None,
        })?;

        assert_eq!(outcome.artifact_path, expected_artifact);
        assert_eq!(outcome.metadata_path, expected_metadata);
        assert!(outcome.artifact_path.exists());
        assert!(outcome.metadata_path.exists());
        Ok(())
    }

    #[test]
    fn publish_to_new_index_creates_catalog_entry() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let game_dir = create_sample_game(temp.path(), "sample-game", "1.2.3")?;
        let artifact_path = temp.path().join("dist").join("sample-game-1.2.3.tar.gz");
        let metadata_path = temp
            .path()
            .join("dist")
            .join("sample-game-1.2.3.metadata.json");

        pack_game(&PackRequest {
            game_dir,
            out: Some(artifact_path.clone()),
            metadata_out: Some(metadata_path.clone()),
        })?;
        sign_for_publish(temp.path(), &artifact_path, &metadata_path)?;

        let index_path = temp.path().join("registry").join("index.json");
        let outcome = publish_to_index(&PublishRequest {
            artifact_path: artifact_path.clone(),
            metadata_path: Some(metadata_path),
            index_locator: index_path.to_string_lossy().to_string(),
            dry_run: false,
            replace_existing: false,
        })?;

        assert_eq!(outcome.game_id, "sample-game");
        assert_eq!(outcome.version, "1.2.3");
        assert!(outcome.created_game);
        assert!(outcome.created_version);
        assert!(!outcome.replaced_existing_version);
        assert!(!outcome.dry_run);
        assert!(index_path.exists());
        assert!(outcome.artifact_path.exists());

        let index_raw = fs::read_to_string(&index_path)?;
        let index_json: Value = serde_json::from_str(&index_raw)?;
        assert_eq!(index_json["schema_version"], 2);
        assert_eq!(index_json["games"][0]["id"], "sample-game");
        assert_eq!(index_json["games"][0]["versions"][0]["version"], "1.2.3");
        Ok(())
    }

    #[test]
    fn publish_to_existing_index_appends_new_version() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let game_dir = create_sample_game(temp.path(), "sample-game", "1.2.3")?;
        let index_path = temp.path().join("registry").join("index.json");

        let artifact_v1 = temp.path().join("dist").join("sample-game-1.2.3.tar.gz");
        let metadata_v1 = temp
            .path()
            .join("dist")
            .join("sample-game-1.2.3.metadata.json");
        pack_game(&PackRequest {
            game_dir: game_dir.clone(),
            out: Some(artifact_v1.clone()),
            metadata_out: Some(metadata_v1.clone()),
        })?;
        sign_for_publish(temp.path(), &artifact_v1, &metadata_v1)?;
        publish_to_index(&PublishRequest {
            artifact_path: artifact_v1,
            metadata_path: Some(metadata_v1),
            index_locator: index_path.to_string_lossy().to_string(),
            dry_run: false,
            replace_existing: false,
        })?;

        write_manifest_version(&game_dir, "sample-game", "1.2.4")?;
        let artifact_v2 = temp.path().join("dist").join("sample-game-1.2.4.tar.gz");
        let metadata_v2 = temp
            .path()
            .join("dist")
            .join("sample-game-1.2.4.metadata.json");
        let pack_v2 = pack_game(&PackRequest {
            game_dir,
            out: Some(artifact_v2.clone()),
            metadata_out: Some(metadata_v2.clone()),
        })?;
        sign_for_publish(temp.path(), &artifact_v2, &metadata_v2)?;
        let publish_v2 = publish_to_index(&PublishRequest {
            artifact_path: artifact_v2,
            metadata_path: Some(metadata_v2),
            index_locator: index_path.to_string_lossy().to_string(),
            dry_run: false,
            replace_existing: false,
        })?;

        assert!(!publish_v2.created_game);
        assert!(publish_v2.created_version);
        assert!(!publish_v2.replaced_existing_version);
        assert!(!publish_v2.dry_run);
        assert_eq!(publish_v2.version, "1.2.4");
        assert_eq!(pack_v2.metadata.version, "1.2.4");

        let index_raw = fs::read_to_string(&index_path)?;
        let index_json: Value = serde_json::from_str(&index_raw)?;
        let versions = index_json["games"][0]["versions"]
            .as_array()
            .expect("versions must be array");
        assert_eq!(versions.len(), 2);
        assert!(
            versions
                .iter()
                .any(|item| item["version"] == Value::String("1.2.3".to_string()))
        );
        assert!(
            versions
                .iter()
                .any(|item| item["version"] == Value::String("1.2.4".to_string()))
        );
        Ok(())
    }

    #[test]
    fn publish_rejects_remote_index_locator() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let game_dir = create_sample_game(temp.path(), "sample-game", "1.2.3")?;
        let artifact_path = temp.path().join("dist").join("sample-game-1.2.3.tar.gz");
        let metadata_path = temp
            .path()
            .join("dist")
            .join("sample-game-1.2.3.metadata.json");
        pack_game(&PackRequest {
            game_dir,
            out: Some(artifact_path.clone()),
            metadata_out: Some(metadata_path.clone()),
        })?;
        sign_for_publish(temp.path(), &artifact_path, &metadata_path)?;

        let result = publish_to_index(&PublishRequest {
            artifact_path,
            metadata_path: Some(metadata_path),
            index_locator: "https://example.com/index.json".to_string(),
            dry_run: false,
            replace_existing: false,
        });
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn publish_dry_run_does_not_write_index_or_artifact() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let game_dir = create_sample_game(temp.path(), "sample-game", "1.2.3")?;
        let artifact_path = temp.path().join("dist").join("sample-game-1.2.3.tar.gz");
        let metadata_path = temp
            .path()
            .join("dist")
            .join("sample-game-1.2.3.metadata.json");

        pack_game(&PackRequest {
            game_dir,
            out: Some(artifact_path.clone()),
            metadata_out: Some(metadata_path.clone()),
        })?;
        sign_for_publish(temp.path(), &artifact_path, &metadata_path)?;

        let index_path = temp.path().join("registry").join("index.json");
        let outcome = publish_to_index(&PublishRequest {
            artifact_path,
            metadata_path: Some(metadata_path),
            index_locator: index_path.to_string_lossy().to_string(),
            dry_run: true,
            replace_existing: false,
        })?;

        assert!(outcome.dry_run);
        assert!(!index_path.exists());
        assert!(!outcome.artifact_path.exists());
        Ok(())
    }

    #[test]
    fn publish_fails_when_manifest_permissions_not_declared() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let game_dir = create_sample_game(temp.path(), "sample-game", "1.2.3")?;
        write_manifest_without_permissions(&game_dir, "sample-game", "1.2.3")?;

        let artifact_path = temp.path().join("dist").join("sample-game-1.2.3.tar.gz");
        let metadata_path = temp
            .path()
            .join("dist")
            .join("sample-game-1.2.3.metadata.json");
        pack_game(&PackRequest {
            game_dir,
            out: Some(artifact_path.clone()),
            metadata_out: Some(metadata_path.clone()),
        })?;
        sign_for_publish(temp.path(), &artifact_path, &metadata_path)?;

        let index_path = temp.path().join("registry").join("index.json");
        let result = publish_to_index(&PublishRequest {
            artifact_path: artifact_path.clone(),
            metadata_path: Some(metadata_path),
            index_locator: index_path.to_string_lossy().to_string(),
            dry_run: false,
            replace_existing: false,
        });
        assert!(result.is_err());
        assert!(!index_path.exists());
        assert!(
            !temp
                .path()
                .join("registry")
                .join("sample-game-1.2.3.tar.gz")
                .exists()
        );
        Ok(())
    }

    #[test]
    fn publish_fails_when_manifest_host_api_is_incompatible() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let game_dir = create_sample_game(temp.path(), "sample-game", "1.2.3")?;
        write_manifest_host_api(&game_dir, "sample-game", "1.2.3", ">=2.0.0")?;

        let artifact_path = temp.path().join("dist").join("sample-game-1.2.3.tar.gz");
        let metadata_path = temp
            .path()
            .join("dist")
            .join("sample-game-1.2.3.metadata.json");
        pack_game(&PackRequest {
            game_dir,
            out: Some(artifact_path.clone()),
            metadata_out: Some(metadata_path.clone()),
        })?;
        sign_for_publish(temp.path(), &artifact_path, &metadata_path)?;

        let index_path = temp.path().join("registry").join("index.json");
        let result = publish_to_index(&PublishRequest {
            artifact_path: artifact_path.clone(),
            metadata_path: Some(metadata_path),
            index_locator: index_path.to_string_lossy().to_string(),
            dry_run: false,
            replace_existing: false,
        });
        assert!(result.is_err());
        assert!(!index_path.exists());
        assert!(
            !temp
                .path()
                .join("registry")
                .join("sample-game-1.2.3.tar.gz")
                .exists()
        );
        Ok(())
    }

    #[test]
    fn publish_fails_when_manifest_host_api_is_invalid() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let game_dir = create_sample_game(temp.path(), "sample-game", "1.2.3")?;
        write_manifest_host_api(&game_dir, "sample-game", "1.2.3", "not-a-range")?;

        let artifact_path = temp.path().join("dist").join("sample-game-1.2.3.tar.gz");
        let metadata_path = temp
            .path()
            .join("dist")
            .join("sample-game-1.2.3.metadata.json");
        pack_game(&PackRequest {
            game_dir,
            out: Some(artifact_path.clone()),
            metadata_out: Some(metadata_path.clone()),
        })?;
        sign_for_publish(temp.path(), &artifact_path, &metadata_path)?;

        let index_path = temp.path().join("registry").join("index.json");
        let result = publish_to_index(&PublishRequest {
            artifact_path: artifact_path.clone(),
            metadata_path: Some(metadata_path),
            index_locator: index_path.to_string_lossy().to_string(),
            dry_run: false,
            replace_existing: false,
        });
        assert!(result.is_err());
        assert!(!index_path.exists());
        assert!(
            !temp
                .path()
                .join("registry")
                .join("sample-game-1.2.3.tar.gz")
                .exists()
        );
        Ok(())
    }

    #[test]
    fn publish_replace_overwrites_existing_version_checksum() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let game_dir = create_sample_game(temp.path(), "sample-game", "1.2.3")?;
        let index_path = temp.path().join("registry").join("index.json");

        let artifact_v1 = temp.path().join("dist").join("sample-game-1.2.3.tar.gz");
        let metadata_v1 = temp
            .path()
            .join("dist")
            .join("sample-game-1.2.3.metadata.json");
        let pack_v1 = pack_game(&PackRequest {
            game_dir: game_dir.clone(),
            out: Some(artifact_v1.clone()),
            metadata_out: Some(metadata_v1.clone()),
        })?;
        sign_for_publish(temp.path(), &artifact_v1, &metadata_v1)?;
        publish_to_index(&PublishRequest {
            artifact_path: artifact_v1,
            metadata_path: Some(metadata_v1),
            index_locator: index_path.to_string_lossy().to_string(),
            dry_run: false,
            replace_existing: false,
        })?;

        write_wasm_payload(&game_dir, b"updated wasm payload")?;
        let artifact_v2 = temp
            .path()
            .join("dist")
            .join("sample-game-1.2.3-replace.tar.gz");
        let metadata_v2 = temp
            .path()
            .join("dist")
            .join("sample-game-1.2.3-replace.metadata.json");
        let pack_v2 = pack_game(&PackRequest {
            game_dir,
            out: Some(artifact_v2.clone()),
            metadata_out: Some(metadata_v2.clone()),
        })?;
        sign_for_publish(temp.path(), &artifact_v2, &metadata_v2)?;

        let conflict = publish_to_index(&PublishRequest {
            artifact_path: artifact_v2.clone(),
            metadata_path: Some(metadata_v2.clone()),
            index_locator: index_path.to_string_lossy().to_string(),
            dry_run: false,
            replace_existing: false,
        });
        assert!(conflict.is_err());

        let replaced = publish_to_index(&PublishRequest {
            artifact_path: artifact_v2,
            metadata_path: Some(metadata_v2),
            index_locator: index_path.to_string_lossy().to_string(),
            dry_run: false,
            replace_existing: true,
        })?;
        assert!(!replaced.created_game);
        assert!(!replaced.created_version);
        assert!(replaced.replaced_existing_version);
        assert!(!replaced.dry_run);

        let index_raw = fs::read_to_string(&index_path)?;
        let index_json: Value = serde_json::from_str(&index_raw)?;
        let checksum = index_json["games"][0]["versions"][0]["artifact_sha256"]
            .as_str()
            .expect("checksum must be present");
        assert_ne!(checksum, pack_v1.metadata.artifact_sha256);
        assert_eq!(checksum, pack_v2.metadata.artifact_sha256);
        Ok(())
    }

    #[test]
    fn run_dev_cycle_packs_verifies_and_publishes() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let game_dir = create_sample_game(temp.path(), "sample-game", "1.2.3")?;
        let index_path = temp.path().join("registry").join("index.json");

        let outcome = run_dev_cycle(&DevRequest {
            game_dir,
            out: Some(temp.path().join("dist").join("sample-game-1.2.3.tar.gz")),
            metadata_out: Some(
                temp.path()
                    .join("dist")
                    .join("sample-game-1.2.3.metadata.json"),
            ),
            index_locator: index_path.to_string_lossy().to_string(),
            dry_run_publish: false,
            replace_existing: false,
        })?;

        assert_eq!(outcome.pack.metadata.game_id, "sample-game");
        assert_eq!(outcome.verify.game_id, "sample-game");
        assert_eq!(outcome.publish.game_id, "sample-game");
        assert!(index_path.exists());
        Ok(())
    }

    #[test]
    fn game_dir_signature_changes_when_content_changes() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let game_dir = create_sample_game(temp.path(), "sample-game", "1.2.3")?;
        let before = game_dir_signature(&game_dir)?;
        write_wasm_payload(&game_dir, b"changed payload bytes")?;
        let after = game_dir_signature(&game_dir)?;
        assert_ne!(before, after);
        Ok(())
    }
}
