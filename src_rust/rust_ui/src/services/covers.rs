//! Cover-art fetcher. Downloads resources.tidal.com images on demand
//! and caches them under `~/.cache/hiresti/covers/<size>/<id>.jpg`.
//!
//! TIDAL serves square JPEGs at canonical sizes. We standardize on a
//! single size per use-site (160 for grid cards, 80 for the mini
//! player, 320 for detail headers) so the cache hit rate stays high.
//!
//! Downloads run on the caller's thread — wrap in `spawn_blocking` for
//! UI use. The batch helper parallelizes across N worker threads to
//! cut the total wall-clock for a freshly-loaded library page.

use std::collections::HashMap;
use std::io::Read;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;

use crate::error::{AppError, AppResult};
use crate::paths;

const COVERS_DIR: &str = "covers";
const PARALLEL_WORKERS: usize = 4;
/// Per-request timeout. Tidal's image CDN is fast; long stalls usually
/// mean the URL is dead and we should fail fast rather than hold up
/// the whole batch.
const REQUEST_TIMEOUT_SECS: u64 = 5;

fn cache_root(size: u32) -> AppResult<PathBuf> {
    let mut p = paths::cache_dir()?;
    p.push(COVERS_DIR);
    p.push(size.to_string());
    if !p.exists() {
        std::fs::create_dir_all(&p)
            .map_err(|e| AppError::ConfigDir(format!("create cover cache dir: {e}")))?;
    }
    Ok(p)
}

fn cache_path_for(cover_id: &str, size: u32) -> AppResult<PathBuf> {
    let mut p = cache_root(size)?;
    // Cover IDs are UUID-shaped (with dashes); they're filename-safe.
    // Sanitize defensively in case TIDAL ever changes the format.
    let safe: String = cover_id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    if safe.is_empty() {
        return Err(AppError::ConfigDir("empty cover_id".into()));
    }
    p.push(format!("{safe}.jpg"));
    Ok(p)
}

fn cover_url(cover_id: &str, size: u32) -> String {
    // resources.tidal.com expects the cover-id with `-` replaced by `/`.
    let token = cover_id.replace('-', "/");
    format!("https://resources.tidal.com/images/{token}/{size}x{size}.jpg")
}

/// Fetch a single cover. Returns the cached path on success. Cache hit
/// short-circuits the network. Failures (404, timeout, write error)
/// surface as an error — caller decides whether to fall back to the
/// placeholder icon.
pub fn fetch_cover_blocking(cover_id: &str, size: u32) -> AppResult<PathBuf> {
    let path = cache_path_for(cover_id, size)?;
    if path.exists() {
        return Ok(path);
    }
    let url = cover_url(cover_id, size);
    let agent = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(REQUEST_TIMEOUT_SECS))
        .build();
    let resp = agent
        .get(&url)
        .call()
        .map_err(|e| AppError::ConfigDir(format!("cover GET {url}: {e}")))?;
    if resp.status() != 200 {
        return Err(AppError::ConfigDir(format!(
            "cover GET {url} returned {}",
            resp.status()
        )));
    }
    let mut bytes: Vec<u8> = Vec::with_capacity(64 * 1024);
    resp.into_reader()
        .read_to_end(&mut bytes)
        .map_err(|e| AppError::ConfigDir(format!("cover read {url}: {e}")))?;
    // Write atomically: tmp file → rename. Two requesters racing is
    // fine since the destination is the same content.
    let tmp = path.with_extension("jpg.tmp");
    std::fs::write(&tmp, &bytes)
        .map_err(|e| AppError::ConfigDir(format!("cover write {tmp:?}: {e}")))?;
    std::fs::rename(&tmp, &path)
        .map_err(|e| AppError::ConfigDir(format!("cover rename {path:?}: {e}")))?;
    Ok(path)
}

/// Batch-fetch a set of covers in parallel. Failed individual fetches
/// drop out of the result map — callers see them as missing and stay
/// on the placeholder icon.
pub fn fetch_covers_batch_blocking(
    cover_ids: Vec<String>,
    size: u32,
) -> HashMap<String, PathBuf> {
    let result: Arc<Mutex<HashMap<String, PathBuf>>> = Arc::new(Mutex::new(HashMap::new()));
    let queue: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(cover_ids));
    let mut handles = Vec::with_capacity(PARALLEL_WORKERS);
    for _ in 0..PARALLEL_WORKERS {
        let q = Arc::clone(&queue);
        let r = Arc::clone(&result);
        handles.push(thread::spawn(move || loop {
            let id = match q.lock().ok().and_then(|mut g| g.pop()) {
                Some(id) => id,
                None => break,
            };
            match fetch_cover_blocking(&id, size) {
                Ok(p) => {
                    if let Ok(mut g) = r.lock() {
                        g.insert(id, p);
                    }
                }
                Err(e) => {
                    tracing::debug!(cover = %id, error = %e, "cover fetch failed");
                }
            }
        }));
    }
    for h in handles {
        let _ = h.join();
    }
    Arc::try_unwrap(result)
        .map(|m| m.into_inner().unwrap_or_default())
        .unwrap_or_default()
}
