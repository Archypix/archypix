use crate::error::Result;
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::Path;

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn hash_file_known_bytes() {
        // SHA-256("hello\n") == 5891b5b522d5df086d0ff0b110fbd9d21bb4fc7163af34d08286a2e846f6be03
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(b"hello\n").unwrap();
        tmp.flush().unwrap();

        let digest = hash_file(tmp.path()).unwrap();
        assert_eq!(
            digest, "5891b5b522d5df086d0ff0b110fbd9d21bb4fc7163af34d08286a2e846f6be03",
            "SHA-256(b\"hello\\n\") must match"
        );
    }

    #[test]
    fn hash_file_empty() {
        // SHA-256("") == e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let digest = hash_file(tmp.path()).unwrap();
        assert_eq!(
            digest,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }
}

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
