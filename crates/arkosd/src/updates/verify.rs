//! The two checks before anything from a download is trusted: the manifest's ed25519 signature
//! against the compiled-in keys, and the zip's sha256 against the manifest.
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use ed25519_compact::{PublicKey, Signature};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::Path;

/// Accepts the manifest when any listed key signed exactly these bytes. A key that does not
/// decode is skipped.
pub fn verify_manifest(bytes: &[u8], sig_b64: &str, keys: &[&str]) -> Result<(), String> {
    let sig_bytes = B64.decode(sig_b64.trim()).map_err(|e| format!("signature: not base64: {e}"))?;
    let sig = Signature::from_slice(&sig_bytes).map_err(|_| "signature: not 64 bytes".to_string())?;
    for key in keys {
        let Ok(raw) = B64.decode(key.trim()) else { continue };
        let Ok(pk) = PublicKey::from_slice(&raw) else { continue };
        if pk.verify(bytes, &sig).is_ok() {
            return Ok(());
        }
    }
    Err("signature: no release key signed this manifest".into())
}

/// Streams the file through SHA-256.
pub fn sha256_hex(path: &Path) -> std::io::Result<String> {
    let mut f = std::fs::File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn a_manifest_is_accepted_only_with_a_known_keys_signature() {
        let (sk, pk) = test_keygen();
        let msg = b"{\"format\":1}";
        let sig = test_sign(&sk, msg);
        assert_eq!(verify_manifest(msg, &sig, &[&pk]), Ok(()));
        assert_eq!(verify_manifest(msg, &sig, &["AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=", &pk]), Ok(()), "any listed key");
        assert!(verify_manifest(b"{\"format\":1} ", &sig, &[&pk]).is_err(), "one byte more");
        assert!(verify_manifest(msg, &sig, &[]).is_err(), "no keys");
        assert!(verify_manifest(msg, "AAAA", &[&pk]).is_err());
        assert!(verify_manifest(msg, &sig, &["garbage"]).is_err(), "a malformed key is skipped, not a panic");
    }

    #[test]
    fn sha256_of_a_file_matches_shasum() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("f");
        std::fs::write(&p, b"abc").unwrap();
        assert_eq!(sha256_hex(&p).unwrap(), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        std::fs::write(&p, vec![7u8; 3 * 1024 * 1024]).unwrap();
        assert_eq!(sha256_hex(&p).unwrap().len(), 64, "streams a file bigger than one read");
    }

    pub fn test_keygen() -> (String, String) {
        let kp = ed25519_compact::KeyPair::from_seed(ed25519_compact::Seed::generate());
        (B64.encode(kp.sk.as_ref()), B64.encode(kp.pk.as_ref()))
    }

    pub fn test_sign(sk_b64: &str, msg: &[u8]) -> String {
        let sk = ed25519_compact::SecretKey::from_slice(&B64.decode(sk_b64).unwrap()).unwrap();
        B64.encode(sk.sign(msg, None).as_ref())
    }
}
