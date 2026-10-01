use sha2::{Digest, Sha256};

use crate::{Error, Result};

/// The digest in a `.sha256` file: hex, optionally followed by whitespace and a file name.
///
/// # Errors
/// When the first token is not 64 hex digits.
pub fn parse_sha256_line(text: &str) -> Result<String> {
    let token = text.split_whitespace().next().unwrap_or("");
    if token.len() == 64 && token.bytes().all(|b| b.is_ascii_hexdigit()) {
        Ok(token.to_ascii_lowercase())
    } else {
        Err(Error::new(
            "the .sha256 file does not start with a sha256 hex digest",
        ))
    }
}

/// Checks `bytes` against the contents of a `.sha256` file.
///
/// # Errors
/// On an unreadable digest line or a mismatch.
pub fn verify_sha256(bytes: &[u8], sha256_file: &str) -> Result<()> {
    let expected = parse_sha256_line(sha256_file)?;
    let actual = format!("{:x}", Sha256::digest(bytes));
    if actual == expected {
        Ok(())
    } else {
        Err(Error::new(format!(
            "sha256 mismatch: expected {expected}, downloaded file is {actual}"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ABC: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

    #[test]
    fn parses_bare_digest_and_sha256sum_format() {
        assert_eq!(parse_sha256_line(&format!("{ABC}\n")).unwrap(), ABC);
        assert_eq!(
            parse_sha256_line(&format!("{}  warpify-zellij.wasm\n", ABC.to_uppercase())).unwrap(),
            ABC
        );
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse_sha256_line("").is_err());
        assert!(parse_sha256_line("deadbeef  x").is_err());
        assert!(parse_sha256_line(&"z".repeat(64)).is_err());
    }

    #[test]
    fn verifies_and_reports_mismatch() {
        assert!(verify_sha256(b"abc", &format!("{ABC}  warpify-zellij.wasm")).is_ok());
        let err = verify_sha256(b"abd", ABC).unwrap_err().to_string();
        assert!(err.contains("mismatch"), "{err}");
    }
}
