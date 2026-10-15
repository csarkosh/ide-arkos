//! ed25519 over raw bytes, base64 on the outside. The secret key is 64 bytes (seed plus public
//! key, ed25519-compact's own layout), the public key 32, the signature 64.
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use ed25519_compact::{KeyPair, PublicKey, SecretKey, Seed, Signature};

pub fn keygen() -> (String, String) {
    let kp = KeyPair::from_seed(Seed::generate());
    (B64.encode(kp.sk.as_ref()), B64.encode(kp.pk.as_ref()))
}

fn decode(label: &str, b64: &str, len: usize) -> Result<Vec<u8>, String> {
    let bytes = B64.decode(b64.trim()).map_err(|e| format!("{label}: not base64: {e}"))?;
    if bytes.len() != len {
        return Err(format!("{label}: {} bytes, expected {len}", bytes.len()));
    }
    Ok(bytes)
}

pub fn sign(secret_b64: &str, msg: &[u8]) -> Result<String, String> {
    let sk = SecretKey::from_slice(&decode("secret key", secret_b64, 64)?).map_err(|e| format!("secret key: {e}"))?;
    Ok(B64.encode(sk.sign(msg, None).as_ref()))
}

pub fn verify(public_b64: &str, msg: &[u8], sig_b64: &str) -> Result<(), String> {
    let pk = PublicKey::from_slice(&decode("public key", public_b64, 32)?).map_err(|e| format!("public key: {e}"))?;
    let sig = Signature::from_slice(&decode("signature", sig_b64, 64)?).map_err(|e| format!("signature: {e}"))?;
    pk.verify(msg, &sig).map_err(|_| "signature does not match".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_signature_round_trips_and_any_change_fails_it() {
        let (sk, pk) = keygen();
        let msg = b"{\"version\":\"0.2.1\"}";
        let sig = sign(&sk, msg).unwrap();
        assert_eq!(verify(&pk, msg, &sig), Ok(()));
        assert!(verify(&pk, b"{\"version\":\"0.2.2\"}", &sig).is_err(), "another message");
        let mut flipped = base64::engine::general_purpose::STANDARD.decode(&sig).unwrap();
        flipped[10] ^= 1;
        assert!(verify(&pk, msg, &base64::engine::general_purpose::STANDARD.encode(flipped)).is_err(), "a flipped byte");
        let (_, other_pk) = keygen();
        assert!(verify(&other_pk, msg, &sig).is_err(), "another key");
        assert!(verify("not base64!", msg, &sig).unwrap_err().contains("not base64"));
        assert!(verify(&pk, msg, "AAAA").unwrap_err().contains("expected 64"));
    }
}
