use crate::error::Result;
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::Path;

/// Compute the SHA-256 hex digest of the file at `path`.
///
/// Reads in 64 KiB chunks to avoid buffering large files in memory.
/// Must be called inside `tokio::task::spawn_blocking`.
pub fn hash_file(path: &Path) -> Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 65536];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}
