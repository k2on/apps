//! SHA-256 (`Ark.Sha256`), through the `sha2` crate. The spec carries its
//! own implementation so that it imports nothing; a runtime may use a
//! library, and this is the one this crate uses.

use sha2::{Digest, Sha256};

/// The 32-byte SHA-256 digest of some bytes.
pub fn sha256(bytes: &[u8]) -> Vec<u8> {
    Sha256::digest(bytes).to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::hex;

    #[test]
    fn the_empty_string() {
        assert_eq!(hex(&sha256(b"")), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
    }
}
