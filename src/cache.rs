//! Result caching for Raff analysis operations.
//!
//! This module provides caching functionality to avoid re-running expensive
//! analysis operations (Git history traversal, AST parsing) on unchanged files.
//!
//! # Cache Key Computation
//!
//! Cache keys are computed from:
//! - File content hash (SHA-256)
//! - Git HEAD commit hash (for repository-level analysis)
//! - Analysis parameters (e.g., threshold, alpha, etc.)
//!
//! # Cache Storage
//!
//! Cache entries are stored in `~/.cache/raff/` or a local `.raff-cache/` directory.
//! Each entry is serialized using `bincode` for efficient storage and retrieval.
//!
//! # Cache Invalidation
//!
//! Cache entries are invalidated when:
//! - The source file content changes
//! - Git HEAD changes (for Git-based analysis)
//! - Analysis parameters change
//! - The `--no-cache` flag is used
//! - The `--clear-cache` flag is used
//!
//! # Usage
//!
//! ```no_run
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! use raff_core::cache::{CacheKey, CacheEntry, CacheManager};
//! use std::path::PathBuf;
//!
//! let mut cache_manager = CacheManager::new()?;
//!
//! // Create a cache key from file hash and parameters
//! let key = CacheKey::new(
//!     "abc123".to_string(),
//!     Some("def456".to_string()),
//!     vec![("threshold".to_string(), "10".to_string())],
//! );
//!
//! // Try to get cached result
//! if let Some(entry) = cache_manager.get(&key)? {
//!     println!("Cache hit: {:?}", entry);
//! } else {
//!     // Perform analysis and cache result
//!     let data = b"result data".to_vec();
//!     let entry = CacheEntry::new(data);
//!     cache_manager.put(&key, entry)?;
//! }
//! # Ok(())
//! # }
//! ```

use crate::error::{RaffError, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Default cache directory name when using local cache.
const LOCAL_CACHE_DIR: &str = ".raff-cache";

/// Default cache directory name in user's home directory.
const GLOBAL_CACHE_DIR: &str = "raff";

/// Maximum number of cache entries to keep (LRU eviction).
const MAX_CACHE_ENTRIES: usize = 1000;

/// Maximum age of cache entries in seconds (7 days).
const MAX_CACHE_AGE_SECONDS: u64 = 7 * 24 * 60 * 60;

/// A cache key that uniquely identifies an analysis operation.
///
/// The key incorporates file hashes, git state, and analysis parameters
/// to ensure cache entries are only reused when the analysis would produce
/// identical results.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CacheKey {
    /// Hash of the primary file(s) being analyzed (e.g., SHA-256).
    pub content_hash: String,

    /// Git HEAD commit hash, if applicable (for Git-based analysis).
    pub git_head: Option<String>,

    /// Analysis-specific parameters that affect results.
    /// Each tuple is (parameter_name, parameter_value).
    #[serde(default)]
    pub parameters: Vec<(String, String)>,
}

impl CacheKey {
    /// Creates a new cache key.
    ///
    /// # Arguments
    ///
    /// * `content_hash` - Hash of the content being analyzed.
    /// * `git_head` - Optional Git HEAD commit hash.
    /// * `parameters` - Analysis parameters that affect results.
    #[must_use]
    pub const fn new(
        content_hash: String,
        git_head: Option<String>,
        parameters: Vec<(String, String)>,
    ) -> Self {
        Self {
            content_hash,
            git_head,
            parameters,
        }
    }

    /// Creates a cache key from a file path and additional parameters.
    ///
    /// # Arguments
    ///
    /// * `file_path` - Path to the file to hash.
    /// * `git_head` - Optional Git HEAD commit hash.
    /// * `parameters` - Analysis parameters.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read.
    pub fn from_file(
        file_path: &Path,
        git_head: Option<String>,
        parameters: Vec<(String, String)>,
    ) -> Result<Self> {
        let content = fs::read(file_path).map_err(|e| {
            RaffError::io_error_with_source("read file for cache key", file_path.to_path_buf(), e)
        })?;
        let content_hash = hash_bytes(&content);
        Ok(Self::new(content_hash, git_head, parameters))
    }

    /// Creates a cache key from multiple file paths.
    ///
    /// The combined hash is computed by hashing all file contents together.
    ///
    /// # Arguments
    ///
    /// * `file_paths` - Paths to the files to hash.
    /// * `git_head` - Optional Git HEAD commit hash.
    /// * `parameters` - Analysis parameters.
    ///
    /// # Errors
    ///
    /// Returns an error if any file cannot be read.
    pub fn from_files(
        file_paths: &[PathBuf],
        git_head: Option<String>,
        parameters: Vec<(String, String)>,
    ) -> Result<Self> {
        let mut hasher = Sha256::new();
        for path in file_paths {
            let content = fs::read(path).map_err(|e| {
                RaffError::io_error_with_source("read file for cache key", path.clone(), e)
            })?;
            hasher.update(&content);
        }
        let content_hash = format!("{:x}", hasher.finalize());
        Ok(Self::new(content_hash, git_head, parameters))
    }

    /// Returns a string representation of this cache key for use as a filename.
    #[must_use]
    pub fn as_filename(&self) -> String {
        let mut hasher = Sha256::new();
        hasher.update(self.content_hash.as_bytes());
        if let Some(ref git) = self.git_head {
            hasher.update(git.as_bytes());
        }
        for (key, value) in &self.parameters {
            hasher.update(key.as_bytes());
            hasher.update(value.as_bytes());
        }
        format!("{:x}", hasher.finalize())
    }
}

/// A cached analysis result.
///
/// Contains the serialized result data along with metadata
/// for cache management (timestamp, size).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheEntry {
    /// The cached result data as raw bytes.
    pub data: Vec<u8>,

    /// Unix timestamp when this entry was created.
    pub timestamp: u64,

    /// Size of the cached data in bytes.
    size_bytes: usize,
}

impl CacheEntry {
    /// Creates a new cache entry with the current timestamp.
    ///
    /// # Arguments
    ///
    /// * `data` - The result data to cache (as bytes).
    #[must_use]
    pub fn new(data: Vec<u8>) -> Self {
        let size_bytes = data.len();
        Self {
            data,
            timestamp: current_timestamp(),
            size_bytes,
        }
    }

    /// Creates a cache entry from JSON-serializable data.
    ///
    /// # Arguments
    ///
    /// * `value` - The JSON value to cache.
    ///
    /// # Errors
    ///
    /// Returns an error if the value cannot be serialized to JSON.
    pub fn from_json(value: &serde_json::Value) -> Result<Self> {
        let json_bytes = serde_json::to_vec(value).map_err(|e| {
            RaffError::parse_error(format!("Failed to serialize JSON for cache: {}", e))
        })?;
        Ok(Self::new(json_bytes))
    }

    /// Returns the age of this cache entry in seconds.
    #[must_use]
    pub fn age_seconds(&self) -> u64 {
        current_timestamp().saturating_sub(self.timestamp)
    }

    /// Returns whether this cache entry has expired.
    #[must_use]
    pub fn is_expired(&self, max_age_seconds: u64) -> bool {
        self.age_seconds() > max_age_seconds
    }

    /// Returns the cached data as a JSON value.
    ///
    /// # Errors
    ///
    /// Returns an error if the data cannot be deserialized from JSON.
    pub fn as_json(&self) -> Result<serde_json::Value> {
        serde_json::from_slice(&self.data).map_err(|e| {
            RaffError::parse_error(format!("Failed to deserialize cached JSON: {}", e))
        })
    }

    /// Size of the cached data in bytes.
    #[must_use]
    pub const fn get_size_bytes(&self) -> usize {
        self.size_bytes
    }
}

/// Manages the cache storage and retrieval.
///
/// Supports both local (`.raff-cache/`) and global (`~/.cache/raff/`) cache directories.
pub struct CacheManager {
    /// The cache directory path.
    cache_dir: PathBuf,

    /// Whether caching is enabled.
    enabled: bool,
}

impl CacheManager {
    /// Creates a new cache manager with the default global cache directory.
    ///
    /// # Errors
    ///
    /// Returns an error if the cache directory cannot be created.
    pub fn new() -> Result<Self> {
        Self::with_dir(None)
    }

    /// Creates a new cache manager with a specific cache directory.
    ///
    /// If `cache_dir` is `None`, uses `~/.cache/raff/`.
    /// If `cache_dir` is `Some`, uses that directory.
    ///
    /// # Arguments
    ///
    /// * `cache_dir` - Optional custom cache directory path.
    ///
    /// # Errors
    ///
    /// Returns an error if the cache directory cannot be created.
    pub fn with_dir(cache_dir: Option<PathBuf>) -> Result<Self> {
        let dir = cache_dir.unwrap_or_else(|| {
            dirs::home_dir()
                .map(|h| h.join(".cache").join(GLOBAL_CACHE_DIR))
                .unwrap_or_else(|| PathBuf::from(LOCAL_CACHE_DIR))
        });

        // Create cache directory if it doesn't exist
        fs::create_dir_all(&dir).map_err(|e| {
            RaffError::io_error_with_source("create cache directory", dir.clone(), e)
        })?;

        Ok(Self {
            cache_dir: dir,
            enabled: true,
        })
    }

    /// Creates a new cache manager with local cache directory.
    ///
    /// Uses `.raff-cache/` in the current directory.
    ///
    /// # Errors
    ///
    /// Returns an error if the cache directory cannot be created.
    pub fn local() -> Result<Self> {
        Self::with_dir(Some(PathBuf::from(LOCAL_CACHE_DIR)))
    }

    /// Sets whether caching is enabled.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    /// Returns whether caching is enabled.
    #[must_use]
    pub const fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Returns the cache directory path.
    #[must_use]
    pub const fn cache_dir(&self) -> &PathBuf {
        &self.cache_dir
    }

    /// Retrieves a cache entry for the given key.
    ///
    /// Returns `None` if:
    /// - Caching is disabled
    /// - No entry exists for the key
    /// - The entry has expired
    ///
    /// # Arguments
    ///
    /// * `key` - The cache key to look up.
    ///
    /// # Errors
    ///
    /// Returns an error if the cache file cannot be read or deserialized.
    pub fn get(&self, key: &CacheKey) -> Result<Option<CacheEntry>> {
        if !self.enabled {
            return Ok(None);
        }

        let cache_path = self.cache_path(key);
        if !cache_path.exists() {
            return Ok(None);
        }

        let bytes = fs::read(&cache_path).map_err(|e| {
            RaffError::io_error_with_source("read cache entry", cache_path.clone(), e)
        })?;

        let entry: CacheEntry = bincode::deserialize(&bytes).map_err(|e| {
            RaffError::parse_error_with_file(
                cache_path.clone(),
                format!("Failed to deserialize cache entry: {}", e),
            )
        })?;

        // Check if entry has expired
        if entry.is_expired(MAX_CACHE_AGE_SECONDS) {
            // Remove expired entry
            let _ = fs::remove_file(&cache_path);
            return Ok(None);
        }

        Ok(Some(entry))
    }

    /// Stores a cache entry for the given key.
    ///
    /// # Arguments
    ///
    /// * `key` - The cache key to store under.
    /// * `entry` - The cache entry to store.
    ///
    /// # Errors
    ///
    /// Returns an error if the entry cannot be serialized or written.
    pub fn put(&self, key: &CacheKey, entry: CacheEntry) -> Result<()> {
        if !self.enabled {
            return Ok(());
        }

        // Check cache size and evict if necessary
        self.maybe_evict_entries()?;

        let cache_path = self.cache_path(key);
        let bytes = bincode::serialize(&entry).map_err(|e| {
            RaffError::parse_error(format!("Failed to serialize cache entry: {}", e))
        })?;

        fs::write(&cache_path, bytes)
            .map_err(|e| RaffError::io_error_with_source("write cache entry", cache_path, e))?;

        Ok(())
    }

    /// Clears all cache entries.
    ///
    /// # Errors
    ///
    /// Returns an error if the cache directory cannot be read or cleared.
    pub fn clear(&self) -> Result<()> {
        if !self.cache_dir.exists() {
            return Ok(());
        }

        for entry in fs::read_dir(&self.cache_dir).map_err(|e| {
            RaffError::io_error_with_source("read cache directory", self.cache_dir.clone(), e)
        })? {
            let entry = entry.map_err(|e| {
                RaffError::io_error_with_source("read cache dir entry", self.cache_dir.clone(), e)
            })?;
            let path = entry.path();
            if path.is_file() {
                fs::remove_file(&path).map_err(|e| {
                    RaffError::io_error_with_source("remove cache file", path.clone(), e)
                })?;
            }
        }

        Ok(())
    }

    /// Removes the cache entry for the given key, if it exists.
    ///
    /// # Arguments
    ///
    /// * `key` - The cache key to remove.
    ///
    /// # Errors
    ///
    /// Returns an error if the cache file cannot be removed.
    pub fn remove(&self, key: &CacheKey) -> Result<()> {
        let cache_path = self.cache_path(key);
        if cache_path.exists() {
            fs::remove_file(&cache_path).map_err(|e| {
                RaffError::io_error_with_source("remove cache entry", cache_path, e)
            })?;
        }
        Ok(())
    }

    /// Returns the number of cache entries.
    ///
    /// # Errors
    ///
    /// Returns an error if the cache directory cannot be read.
    pub fn entry_count(&self) -> Result<usize> {
        if !self.cache_dir.exists() {
            return Ok(0);
        }

        let mut count = 0;
        for entry in fs::read_dir(&self.cache_dir).map_err(|e| {
            RaffError::io_error_with_source("read cache directory", self.cache_dir.clone(), e)
        })? {
            let entry = entry.map_err(|e| {
                RaffError::io_error_with_source("read cache dir entry", self.cache_dir.clone(), e)
            })?;
            if entry.path().is_file() {
                count += 1;
            }
        }

        Ok(count)
    }

    /// Returns the total size of all cache entries in bytes.
    ///
    /// # Errors
    ///
    /// Returns an error if the cache directory cannot be read.
    pub fn total_size(&self) -> Result<u64> {
        if !self.cache_dir.exists() {
            return Ok(0);
        }

        let mut total_size = 0u64;
        for entry in fs::read_dir(&self.cache_dir).map_err(|e| {
            RaffError::io_error_with_source("read cache directory", self.cache_dir.clone(), e)
        })? {
            let entry = entry.map_err(|e| {
                RaffError::io_error_with_source("read cache dir entry", self.cache_dir.clone(), e)
            })?;
            let path = entry.path();
            if path.is_file() {
                let metadata = fs::metadata(&path).map_err(|e| {
                    RaffError::io_error_with_source("read cache file metadata", path, e)
                })?;
                total_size += metadata.len();
            }
        }

        Ok(total_size)
    }

    /// Evicts old cache entries if the cache is too large.
    ///
    /// Uses LRU eviction based on file modification time.
    ///
    /// # Errors
    ///
    /// Returns an error if the cache cannot be read or entries cannot be removed.
    fn maybe_evict_entries(&self) -> Result<()> {
        let count = self.entry_count()?;
        if count <= MAX_CACHE_ENTRIES {
            return Ok(());
        }

        // Collect entries with their modification times
        let mut entries: Vec<(PathBuf, SystemTime)> = Vec::new();
        for entry in fs::read_dir(&self.cache_dir).map_err(|e| {
            RaffError::io_error_with_source("read cache directory", self.cache_dir.clone(), e)
        })? {
            let entry = entry.map_err(|e| {
                RaffError::io_error_with_source("read cache dir entry", self.cache_dir.clone(), e)
            })?;
            let path = entry.path();
            if path.is_file() {
                let metadata = fs::metadata(&path).map_err(|e| {
                    RaffError::io_error_with_source("read cache file metadata", path.clone(), e)
                })?;
                let modified = metadata.modified().map_err(|e| {
                    RaffError::io_error_with_source(
                        "read cache file modified time",
                        path.clone(),
                        e,
                    )
                })?;
                entries.push((path, modified));
            }
        }

        // Sort by modification time (oldest first)
        entries.sort_by_key(|(_, time)| *time);

        // Remove oldest entries until we're under the limit
        let to_remove = count - MAX_CACHE_ENTRIES;
        for (path, _) in entries.iter().take(to_remove) {
            fs::remove_file(path).map_err(|e| {
                RaffError::io_error_with_source("remove old cache entry", path.clone(), e)
            })?;
        }

        Ok(())
    }

    /// Returns the cache file path for a given key.
    #[must_use]
    fn cache_path(&self, key: &CacheKey) -> PathBuf {
        self.cache_dir.join(key.as_filename())
    }
}

impl Default for CacheManager {
    fn default() -> Self {
        Self::new().expect("Failed to create default cache manager")
    }
}

/// Computes the SHA-256 hash of the given bytes.
///
/// # Arguments
///
/// * `bytes` - The bytes to hash.
///
/// # Returns
///
/// A hexadecimal string representation of the hash.
#[must_use]
pub fn hash_bytes(bytes: &[u8]) -> String {
    let hasher = Sha256::new();
    let hash = hasher.chain_update(bytes).finalize();
    format!("{:x}", hash)
}

/// Computes the SHA-256 hash of a file's contents.
///
/// # Arguments
///
/// * `path` - Path to the file to hash.
///
/// # Errors
///
/// Returns an error if the file cannot be read.
pub fn hash_file(path: &Path) -> Result<String> {
    let content = fs::read(path).map_err(|e| {
        RaffError::io_error_with_source("read file for hashing", path.to_path_buf(), e)
    })?;
    Ok(hash_bytes(&content))
}

/// Returns the current Unix timestamp in seconds.
#[must_use]
pub fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Module providing cross-platform home directory support.
///
/// Re-exported for convenience.
mod dirs {
    /// Returns the user's home directory.
    #[must_use]
    pub fn home_dir() -> Option<std::path::PathBuf> {
        // Try standard environment variables
        if let Some(home) = std::env::var_os("HOME") {
            return Some(std::path::PathBuf::from(home));
        }

        #[cfg(target_os = "windows")]
        {
            if let Some(home) = std::env::var_os("USERPROFILE") {
                return Some(std::path::PathBuf::from(home));
            }
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;
    use tempfile::TempDir;

    /// Helper to create a temporary file with content
    fn create_temp_file_with_content(content: &str) -> (TempDir, PathBuf) {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let file_path = temp_dir.path().join("test_file.txt");
        fs::write(&file_path, content).expect("Failed to write temp file");
        (temp_dir, file_path)
    }

    // CacheKey tests

    #[test]
    fn test_cache_key_new_creates_key() {
        let key = CacheKey::new(
            "abc123".to_string(),
            Some("git_head".to_string()),
            vec![("param1".to_string(), "value1".to_string())],
        );

        assert_eq!(key.content_hash, "abc123");
        assert_eq!(key.git_head, Some("git_head".to_string()));
        assert_eq!(key.parameters.len(), 1);
        assert_eq!(
            key.parameters[0],
            ("param1".to_string(), "value1".to_string())
        );
    }

    #[test]
    fn test_cache_key_from_file_creates_hash() {
        let (_temp_dir, file_path) = create_temp_file_with_content("test content");

        let key = CacheKey::from_file(&file_path, None, Vec::new())
            .expect("CacheKey::from_file should succeed");

        assert!(!key.content_hash.is_empty());
        assert!(key.git_head.is_none());
        assert!(key.parameters.is_empty());
    }

    #[test]
    fn test_cache_key_from_file_with_same_content_creates_same_hash() {
        let (_temp_dir1, file_path1) = create_temp_file_with_content("same content");
        let (_temp_dir2, file_path2) = create_temp_file_with_content("same content");

        let key1 = CacheKey::from_file(&file_path1, None, Vec::new())
            .expect("CacheKey::from_file should succeed");
        let key2 = CacheKey::from_file(&file_path2, None, Vec::new())
            .expect("CacheKey::from_file should succeed");

        assert_eq!(key1.content_hash, key2.content_hash);
    }

    #[test]
    fn test_cache_key_from_file_with_different_content_creates_different_hash() {
        let (_temp_dir1, file_path1) = create_temp_file_with_content("content A");
        let (_temp_dir2, file_path2) = create_temp_file_with_content("content B");

        let key1 = CacheKey::from_file(&file_path1, None, Vec::new())
            .expect("CacheKey::from_file should succeed");
        let key2 = CacheKey::from_file(&file_path2, None, Vec::new())
            .expect("CacheKey::from_file should succeed");

        assert_ne!(key1.content_hash, key2.content_hash);
    }

    #[test]
    fn test_cache_key_from_files_combines_hashes() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let file1 = temp_dir.path().join("file1.txt");
        let file2 = temp_dir.path().join("file2.txt");

        fs::write(&file1, "content 1").expect("Failed to write file1");
        fs::write(&file2, "content 2").expect("Failed to write file2");

        let key = CacheKey::from_files(&[file1.clone(), file2], None, Vec::new())
            .expect("CacheKey::from_files should succeed");

        assert!(!key.content_hash.is_empty());
    }

    #[test]
    fn test_cache_key_as_filename_produces_valid_filename() {
        let key = CacheKey::new(
            "abc123".to_string(),
            Some("def456".to_string()),
            vec![("param".to_string(), "value".to_string())],
        );

        let filename = key.as_filename();

        // SHA-256 produces 64 hex characters
        assert_eq!(filename.len(), 64);
        // All characters should be valid hex digits
        assert!(filename.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_cache_key_equality_same_parameters() {
        let key1 = CacheKey::new(
            "hash".to_string(),
            Some("git".to_string()),
            vec![("p".to_string(), "v".to_string())],
        );
        let key2 = CacheKey::new(
            "hash".to_string(),
            Some("git".to_string()),
            vec![("p".to_string(), "v".to_string())],
        );

        assert_eq!(key1, key2);
    }

    // CacheEntry tests

    #[test]
    fn test_cache_entry_new_creates_entry() {
        let data = b"{\"result\":\"value\"}".to_vec();
        let entry = CacheEntry::new(data.clone());

        assert_eq!(entry.data, data);
        assert!(entry.timestamp > 0);
        assert!(entry.get_size_bytes() > 0);
    }

    #[test]
    fn test_cache_entry_age_seconds_returns_age() {
        let entry = CacheEntry::new(b"{}".to_vec());

        // Entry should be very recent
        assert!(entry.age_seconds() < 10);
    }

    #[test]
    fn test_cache_entry_is_expired_with_old_entry() {
        let mut entry = CacheEntry::new(b"{}".to_vec());
        // Set timestamp to be older than max age
        entry.timestamp = current_timestamp().saturating_sub(MAX_CACHE_AGE_SECONDS + 100);

        assert!(entry.is_expired(MAX_CACHE_AGE_SECONDS));
    }

    #[test]
    fn test_cache_entry_is_expired_with_new_entry() {
        let entry = CacheEntry::new(b"{}".to_vec());

        assert!(!entry.is_expired(MAX_CACHE_AGE_SECONDS));
    }

    #[test]
    fn test_cache_entry_from_json_and_as_json_roundtrip() {
        let json_value = json!({"result": "value"});
        let entry = CacheEntry::from_json(&json_value).expect("from_json should succeed");

        let retrieved = entry.as_json().expect("as_json should succeed");
        assert_eq!(json_value, retrieved);
    }

    // CacheManager tests

    #[test]
    fn test_cache_manager_new_creates_manager() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let cache_dir = temp_dir.path().join("cache");
        let manager = CacheManager::with_dir(Some(cache_dir.clone()));

        assert!(manager.is_ok());
        let manager = manager.unwrap();
        assert!(manager.is_enabled());
        assert_eq!(manager.cache_dir(), &cache_dir);
        assert!(cache_dir.exists());
    }

    #[test]
    fn test_cache_manager_local_creates_local_cache() {
        let manager = CacheManager::local();

        assert!(manager.is_ok());
        let manager = manager.unwrap();
        assert_eq!(manager.cache_dir(), &PathBuf::from(LOCAL_CACHE_DIR));
    }

    #[test]
    fn test_cache_manager_set_enabled_disables_cache() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let mut manager = CacheManager::with_dir(Some(temp_dir.path().join("cache")))
            .expect("Failed to create manager");

        manager.set_enabled(false);
        assert!(!manager.is_enabled());
    }

    #[test]
    fn test_cache_manager_get_returns_none_when_disabled() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let mut manager = CacheManager::with_dir(Some(temp_dir.path().join("cache")))
            .expect("Failed to create manager");

        manager.set_enabled(false);

        let key = CacheKey::new("test".to_string(), None, Vec::new());
        let result = manager.get(&key);

        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn test_cache_manager_put_and_get_roundtrip() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let manager = CacheManager::with_dir(Some(temp_dir.path().join("cache")))
            .expect("Failed to create manager");

        let key = CacheKey::new("test_key".to_string(), None, Vec::new());
        let json_value = json!({"cached": "data"});
        let entry = CacheEntry::from_json(&json_value).expect("from_json should succeed");

        let put_result = manager.put(&key, entry);
        assert!(put_result.is_ok(), "put should succeed");

        let get_result = manager.get(&key);
        assert!(get_result.is_ok(), "get should succeed");

        let retrieved = get_result.unwrap();
        assert!(retrieved.is_some(), "should retrieve cached entry");
        let retrieved_entry = retrieved.unwrap();
        let retrieved_json = retrieved_entry.as_json().expect("as_json should succeed");
        assert_eq!(retrieved_json, json_value);
    }

    #[test]
    fn test_cache_manager_get_returns_none_for_nonexistent_key() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let manager = CacheManager::with_dir(Some(temp_dir.path().join("cache")))
            .expect("Failed to create manager");

        let key = CacheKey::new("nonexistent".to_string(), None, Vec::new());
        let result = manager.get(&key);

        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn test_cache_manager_clear_removes_all_entries() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let manager = CacheManager::with_dir(Some(temp_dir.path().join("cache")))
            .expect("Failed to create manager");

        // Add some entries
        let key1 = CacheKey::new("key1".to_string(), None, Vec::new());
        let key2 = CacheKey::new("key2".to_string(), None, Vec::new());
        manager
            .put(
                &key1,
                CacheEntry::from_json(&json!({})).expect("from_json should succeed"),
            )
            .expect("put key1 should succeed");
        manager
            .put(
                &key2,
                CacheEntry::from_json(&json!({})).expect("from_json should succeed"),
            )
            .expect("put key2 should succeed");

        assert_eq!(
            manager.entry_count().expect("entry_count should succeed"),
            2
        );

        // Clear cache
        manager.clear().expect("clear should succeed");

        assert_eq!(
            manager.entry_count().expect("entry_count should succeed"),
            0
        );
    }

    #[test]
    fn test_cache_manager_remove_removes_specific_entry() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let manager = CacheManager::with_dir(Some(temp_dir.path().join("cache")))
            .expect("Failed to create manager");

        let key1 = CacheKey::new("key1".to_string(), None, Vec::new());
        let key2 = CacheKey::new("key2".to_string(), None, Vec::new());
        manager
            .put(
                &key1,
                CacheEntry::from_json(&json!({})).expect("from_json should succeed"),
            )
            .expect("put key1 should succeed");
        manager
            .put(
                &key2,
                CacheEntry::from_json(&json!({})).expect("from_json should succeed"),
            )
            .expect("put key2 should succeed");

        manager.remove(&key1).expect("remove should succeed");

        assert!(manager
            .get(&key1)
            .expect("get key1 should succeed")
            .is_none());
        assert!(manager
            .get(&key2)
            .expect("get key2 should succeed")
            .is_some());
    }

    #[test]
    fn test_cache_manager_entry_count_returns_correct_count() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let manager = CacheManager::with_dir(Some(temp_dir.path().join("cache")))
            .expect("Failed to create manager");

        assert_eq!(
            manager.entry_count().expect("entry_count should succeed"),
            0
        );

        manager
            .put(
                &CacheKey::new("k1".to_string(), None, Vec::new()),
                CacheEntry::from_json(&json!({})).expect("from_json should succeed"),
            )
            .expect("put k1 should succeed");
        manager
            .put(
                &CacheKey::new("k2".to_string(), None, Vec::new()),
                CacheEntry::from_json(&json!({})).expect("from_json should succeed"),
            )
            .expect("put k2 should succeed");

        assert_eq!(
            manager.entry_count().expect("entry_count should succeed"),
            2
        );
    }

    #[test]
    fn test_cache_manager_total_size_returns_size() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let manager = CacheManager::with_dir(Some(temp_dir.path().join("cache")))
            .expect("Failed to create manager");

        let key = CacheKey::new("test".to_string(), None, Vec::new());
        let entry = CacheEntry::from_json(&json!({"large_data": "x".repeat(1000)}))
            .expect("from_json should succeed");
        manager.put(&key, entry).expect("put should succeed");

        let total_size = manager.total_size().expect("total_size should succeed");
        assert!(total_size > 0);
    }

    #[test]
    fn test_cache_manager_default_creates_manager() {
        let manager = CacheManager::default();
        assert!(manager.is_enabled());
        assert!(
            manager.cache_dir().ends_with(GLOBAL_CACHE_DIR)
                || manager.cache_dir().ends_with(LOCAL_CACHE_DIR)
        );
    }

    // Utility function tests

    #[test]
    fn test_hash_bytes_produces_consistent_hash() {
        let bytes = b"test content";
        let hash1 = hash_bytes(bytes);
        let hash2 = hash_bytes(bytes);

        assert_eq!(hash1, hash2);
        assert_eq!(hash1.len(), 64);
    }

    #[test]
    fn test_hash_bytes_produces_different_hash_for_different_content() {
        let hash1 = hash_bytes(b"content A");
        let hash2 = hash_bytes(b"content B");

        assert_ne!(hash1, hash2);
    }

    #[test]
    fn test_hash_file_hashes_file_content() {
        let (_temp_dir, file_path) = create_temp_file_with_content("file content");

        let hash = hash_file(&file_path).expect("hash_file should succeed");

        assert!(!hash.is_empty());
        assert_eq!(hash.len(), 64);
    }

    #[test]
    fn test_hash_file_with_same_content_produces_same_hash() {
        let (_temp_dir1, file_path1) = create_temp_file_with_content("same");
        let (_temp_dir2, file_path2) = create_temp_file_with_content("same");

        let hash1 = hash_file(&file_path1).expect("hash_file 1 should succeed");
        let hash2 = hash_file(&file_path2).expect("hash_file 2 should succeed");

        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_current_timestamp_returns_reasonable_value() {
        let ts = current_timestamp();

        // Should be a reasonable timestamp (after year 2020)
        assert!(ts > 1_577_836_800);
    }

    // Integration tests

    #[test]
    fn test_cache_manager_get_expired_entry_returns_none() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let manager = CacheManager::with_dir(Some(temp_dir.path().join("cache")))
            .expect("Failed to create manager");

        let key = CacheKey::new("test".to_string(), None, Vec::new());
        let mut entry = CacheEntry::new(b"{}".to_vec());
        // Make entry expired
        entry.timestamp = current_timestamp().saturating_sub(MAX_CACHE_AGE_SECONDS + 100);

        // Manually write an expired entry
        let cache_path = manager.cache_dir().join(key.as_filename());
        let bytes = bincode::serialize(&entry).expect("serialization should succeed");
        fs::write(&cache_path, bytes).expect("write should succeed");

        // Getting expired entry should return None and remove the file
        let result = manager.get(&key).expect("get should succeed");
        assert!(result.is_none());
        assert!(!cache_path.exists());
    }

    #[test]
    fn test_cache_key_with_different_git_head_creates_different_key() {
        let key1 = CacheKey::new("hash".to_string(), Some("git1".to_string()), Vec::new());
        let key2 = CacheKey::new("hash".to_string(), Some("git2".to_string()), Vec::new());

        assert_ne!(key1, key2);
        assert_ne!(key1.as_filename(), key2.as_filename());
    }

    #[test]
    fn test_cache_key_with_different_parameters_creates_different_key() {
        let key1 = CacheKey::new(
            "hash".to_string(),
            None,
            vec![("param".to_string(), "value1".to_string())],
        );
        let key2 = CacheKey::new(
            "hash".to_string(),
            None,
            vec![("param".to_string(), "value2".to_string())],
        );

        assert_ne!(key1, key2);
        assert_ne!(key1.as_filename(), key2.as_filename());
    }

    #[test]
    fn test_cache_entry_serialize_deserialize_roundtrip() {
        let original = CacheEntry::new(b"{\"complex\":{\"nested\":\"data\"}}".to_vec());

        let bytes = bincode::serialize(&original).expect("serialize should succeed");
        let deserialized: CacheEntry =
            bincode::deserialize(&bytes).expect("deserialize should succeed");

        assert_eq!(original.data, deserialized.data);
        assert_eq!(original.timestamp, deserialized.timestamp);
        assert_eq!(original.get_size_bytes(), deserialized.get_size_bytes());
    }

    #[test]
    fn test_dirs_home_dir_returns_home() {
        if let Some(home_path) = dirs::home_dir() {
            assert!(home_path.is_absolute());
        }
        // If None, that's also valid (e.g., in restricted environments)
    }
}
