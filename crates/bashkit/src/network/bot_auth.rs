// Decision: Implement draft-meunier-web-bot-auth-architecture using Ed25519 signatures
// over RFC 9421 HTTP Message Signatures. Bind signatures to request method + target URI.
// Feature-gated behind `bot-auth` to avoid pulling crypto deps by default.
// Non-blocking: signing failures log a warning and send the request unsigned.

//! Web Bot Authentication support (draft-meunier-web-bot-auth-architecture).
//!
//! Signs outgoing HTTP requests with Ed25519 signatures per RFC 9421,
//! enabling origins to verify bot identity cryptographically.
//!
//! # Quick Start
//!
//! ```rust,ignore
//! use bashkit::network::BotAuthConfig;
//!
//! let config = BotAuthConfig::from_seed([42u8; 32])
//!     .with_agent_fqdn("bot.example.com")
//!     .with_validity_secs(300);
//! ```

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use ed25519_dalek::{Signer, SigningKey, VerifyingKey};
use rand::Rng;
use sha2::{Digest, Sha256};
use std::time::{SystemTime, UNIX_EPOCH};
use zeroize::Zeroize;

/// Configuration for Web Bot Authentication.
///
/// Holds an Ed25519 signing key and optional metadata for the
/// `Signature-Agent` discovery header.
pub struct BotAuthConfig {
    // THREAT[TM-CRY-001]: Store raw seed and explicitly zeroize in Drop.
    seed: [u8; 32],
    agent_fqdn: Option<String>,
    validity_secs: u64,
}

impl std::fmt::Debug for BotAuthConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BotAuthConfig")
            .field("seed", &"[REDACTED]")
            .field("agent_fqdn", &self.agent_fqdn)
            .field("validity_secs", &self.validity_secs)
            .finish()
    }
}

impl Clone for BotAuthConfig {
    fn clone(&self) -> Self {
        Self {
            seed: self.seed,
            agent_fqdn: self.agent_fqdn.clone(),
            validity_secs: self.validity_secs,
        }
    }
}

impl Drop for BotAuthConfig {
    fn drop(&mut self) {
        self.seed.zeroize();
    }
}

impl BotAuthConfig {
    /// Create from a 32-byte Ed25519 secret key seed.
    pub fn from_seed(seed: [u8; 32]) -> Self {
        Self {
            seed,
            agent_fqdn: None,
            validity_secs: 300,
        }
    }

    /// Create from a base64url-encoded Ed25519 secret key seed.
    pub fn from_base64_seed(encoded: &str) -> Result<Self, BotAuthError> {
        let bytes = URL_SAFE_NO_PAD
            .decode(encoded)
            .map_err(|_| BotAuthError::InvalidKey("invalid base64url encoding"))?;
        let seed: [u8; 32] = bytes
            .try_into()
            .map_err(|_| BotAuthError::InvalidKey("seed must be exactly 32 bytes"))?;
        Ok(Self::from_seed(seed))
    }

    /// Set the agent FQDN for key discovery (`Signature-Agent` header).
    pub fn with_agent_fqdn(mut self, fqdn: impl Into<String>) -> Self {
        self.agent_fqdn = Some(fqdn.into());
        self
    }

    /// Set signature validity duration in seconds (default: 300).
    pub fn with_validity_secs(mut self, secs: u64) -> Self {
        self.validity_secs = secs;
        self
    }

    /// Compute the JWK Thumbprint (RFC 7638) keyid for the public key.
    pub fn keyid(&self) -> String {
        let signing_key = SigningKey::from_bytes(&self.seed);
        jwk_thumbprint_ed25519(&signing_key.verifying_key())
    }

    /// Sign a request and return headers to attach.
    ///
    /// Returns `Err` on clock errors; callers should log and send unsigned.
    pub(crate) fn sign_request(
        &self,
        method: &str,
        target_uri: &str,
    ) -> Result<BotAuthHeaders, BotAuthError> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| BotAuthError::Clock)?
            .as_secs();
        let expires = now + self.validity_secs;
        let keyid = self.keyid();
        let nonce = generate_nonce();

        // Build covered components list
        let mut covered = String::from("\"@method\" \"@target-uri\"");
        if self.agent_fqdn.is_some() {
            covered.push_str(" \"signature-agent\"");
        }

        // Signature parameters (without label, for @signature-params line)
        let sig_params = format!(
            "({covered});created={now};expires={expires};\
             keyid=\"{keyid}\";alg=\"ed25519\";nonce=\"{nonce}\";\
             tag=\"web-bot-auth\""
        );

        // Build signature base per RFC 9421 Section 2.5
        let mut sig_base = format!("\"@method\": {method}\n\"@target-uri\": {target_uri}\n");
        if let Some(ref fqdn) = self.agent_fqdn {
            sig_base.push_str(&format!("\"signature-agent\": {fqdn}\n"));
        }
        sig_base.push_str(&format!("\"@signature-params\": {sig_params}"));

        // Sign
        let signing_key = SigningKey::from_bytes(&self.seed);
        let signature = signing_key.sign(sig_base.as_bytes());
        let sig_b64 = URL_SAFE_NO_PAD.encode(signature.to_bytes());

        Ok(BotAuthHeaders {
            signature: format!("sig=:{sig_b64}:"),
            signature_input: format!("sig={sig_params}"),
            signature_agent: self.agent_fqdn.clone(),
        })
    }
}

/// Headers produced by bot-auth signing. Applied to outbound HTTP requests.
#[derive(Debug)]
pub(crate) struct BotAuthHeaders {
    pub signature: String,
    pub signature_input: String,
    pub signature_agent: Option<String>,
}

/// Errors from bot-auth operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BotAuthError {
    /// The provided key material is invalid.
    InvalidKey(&'static str),
    /// System clock returned a time before the Unix epoch.
    Clock,
}

impl std::fmt::Display for BotAuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BotAuthError::InvalidKey(msg) => write!(f, "invalid bot-auth key: {msg}"),
            BotAuthError::Clock => write!(f, "system clock error"),
        }
    }
}

impl std::error::Error for BotAuthError {}

/// Derived Ed25519 public key and JWK Thumbprint for key directory serving.
pub struct BotAuthPublicKey {
    /// JWK Thumbprint (RFC 7638) — used as `keyid` in signatures.
    pub key_id: String,
    /// Full JWK object (OKP/Ed25519) for inclusion in JWKS responses.
    pub jwk: serde_json::Value,
}

/// Derive the Ed25519 public key and JWK Thumbprint from a base64url seed.
///
/// The consumer uses the returned key to serve the well-known key directory
/// endpoint so target servers can verify signatures.
pub fn derive_bot_auth_public_key(seed: &str) -> Result<BotAuthPublicKey, BotAuthError> {
    let config = BotAuthConfig::from_base64_seed(seed)?;
    let signing_key = SigningKey::from_bytes(&config.seed);
    let verifying_key = signing_key.verifying_key();
    let x = URL_SAFE_NO_PAD.encode(verifying_key.as_bytes());
    let key_id = jwk_thumbprint_ed25519(&verifying_key);
    let jwk = serde_json::json!({
        "kty": "OKP",
        "crv": "Ed25519",
        "x": x,
    });
    Ok(BotAuthPublicKey { key_id, jwk })
}

/// Compute JWK Thumbprint (RFC 7638) for an Ed25519 key (RFC 8037).
///
/// Members in lexicographic order: `crv`, `kty`, `x`.
fn jwk_thumbprint_ed25519(key: &VerifyingKey) -> String {
    let x = URL_SAFE_NO_PAD.encode(key.as_bytes());
    let jwk_json = format!(r#"{{"crv":"Ed25519","kty":"OKP","x":"{x}"}}"#);
    let hash = Sha256::digest(jwk_json.as_bytes());
    URL_SAFE_NO_PAD.encode(hash)
}

/// Generate a cryptographically random nonce (32 bytes, base64url-encoded).
fn generate_nonce() -> String {
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::Verifier;

    #[test]
    fn from_seed_roundtrip() {
        let seed = [1u8; 32];
        let config = BotAuthConfig::from_seed(seed);
        let keyid = config.keyid();
        assert!(!keyid.is_empty());
    }

    #[test]
    fn from_base64_seed() {
        let seed = [2u8; 32];
        let encoded = URL_SAFE_NO_PAD.encode(seed);
        let config = BotAuthConfig::from_base64_seed(&encoded).unwrap();
        assert_eq!(config.keyid(), BotAuthConfig::from_seed(seed).keyid());
    }

    #[test]
    fn from_base64_seed_invalid() {
        assert!(BotAuthConfig::from_base64_seed("!!!invalid!!!").is_err());
        let short = URL_SAFE_NO_PAD.encode([0u8; 16]);
        assert!(BotAuthConfig::from_base64_seed(&short).is_err());
    }

    #[test]
    fn sign_request_produces_valid_headers() {
        let config = BotAuthConfig::from_seed([3u8; 32]);
        let headers = config.sign_request("GET", "https://example.com").unwrap();

        assert!(headers.signature.starts_with("sig=:"));
        assert!(headers.signature.ends_with(':'));
        assert!(headers.signature_input.starts_with("sig=("));
        assert!(headers.signature_input.contains("tag=\"web-bot-auth\""));
        assert!(headers.signature_input.contains("alg=\"ed25519\""));
        assert!(headers.signature_input.contains("keyid="));
        assert!(headers.signature_input.contains("nonce="));
        assert!(headers.signature_agent.is_none());
    }

    #[test]
    fn sign_request_with_agent_fqdn() {
        let config = BotAuthConfig::from_seed([4u8; 32]).with_agent_fqdn("bot.example.com");
        let headers = config.sign_request("GET", "https://example.com").unwrap();

        assert_eq!(headers.signature_agent.as_deref(), Some("bot.example.com"));
        assert!(headers.signature_input.contains("\"signature-agent\""));
    }

    #[test]
    fn signature_is_verifiable() {
        let seed = [5u8; 32];
        let config = BotAuthConfig::from_seed(seed);
        let signing_key = SigningKey::from_bytes(&seed);
        let verifying_key = signing_key.verifying_key();

        let headers = config
            .sign_request("POST", "https://verify.example.com/v1/items?q=1")
            .unwrap();

        // Reconstruct signature base
        let sig_params = headers.signature_input.strip_prefix("sig=").unwrap();
        let sig_base = format!(
            "\"@method\": POST\n\"@target-uri\": https://verify.example.com/v1/items?q=1\n\"@signature-params\": {sig_params}"
        );

        // Extract raw signature bytes
        let sig_b64 = headers
            .signature
            .strip_prefix("sig=:")
            .unwrap()
            .strip_suffix(':')
            .unwrap();
        let sig_bytes = URL_SAFE_NO_PAD.decode(sig_b64).unwrap();
        let signature = ed25519_dalek::Signature::from_slice(&sig_bytes).unwrap();

        assert!(
            verifying_key
                .verify(sig_base.as_bytes(), &signature)
                .is_ok()
        );
    }

    #[test]
    fn signature_rejects_method_or_target_tampering() {
        let seed = [15u8; 32];
        let config = BotAuthConfig::from_seed(seed);
        let signing_key = SigningKey::from_bytes(&seed);
        let verifying_key = signing_key.verifying_key();

        let headers = config
            .sign_request("POST", "https://verify.example.com/v1/items?q=1")
            .unwrap();
        let sig_params = headers.signature_input.strip_prefix("sig=").unwrap();
        let sig_b64 = headers
            .signature
            .strip_prefix("sig=:")
            .unwrap()
            .strip_suffix(':')
            .unwrap();
        let sig_bytes = URL_SAFE_NO_PAD.decode(sig_b64).unwrap();
        let signature = ed25519_dalek::Signature::from_slice(&sig_bytes).unwrap();

        let tampered_method = format!(
            "\"@method\": GET\n\"@target-uri\": https://verify.example.com/v1/items?q=1\n\"@signature-params\": {sig_params}"
        );
        let tampered_target = format!(
            "\"@method\": POST\n\"@target-uri\": https://verify.example.com/admin/delete\n\"@signature-params\": {sig_params}"
        );

        assert!(
            verifying_key
                .verify(tampered_method.as_bytes(), &signature)
                .is_err()
        );
        assert!(
            verifying_key
                .verify(tampered_target.as_bytes(), &signature)
                .is_err()
        );
    }

    #[test]
    fn jwk_thumbprint_deterministic() {
        let key = SigningKey::from_bytes(&[6u8; 32]).verifying_key();
        let t1 = jwk_thumbprint_ed25519(&key);
        let t2 = jwk_thumbprint_ed25519(&key);
        assert_eq!(t1, t2);
        assert!(!t1.is_empty());
    }

    #[test]
    fn validity_secs_respected() {
        let config = BotAuthConfig::from_seed([7u8; 32]).with_validity_secs(600);
        let headers = config.sign_request("GET", "https://example.com").unwrap();
        let input = &headers.signature_input;
        let created: u64 = input
            .split("created=")
            .nth(1)
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .parse()
            .unwrap();
        let expires: u64 = input
            .split("expires=")
            .nth(1)
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .parse()
            .unwrap();
        assert_eq!(expires - created, 600);
    }

    #[test]
    fn derive_public_key() {
        let seed = [8u8; 32];
        let encoded = URL_SAFE_NO_PAD.encode(seed);
        let pubkey = derive_bot_auth_public_key(&encoded).unwrap();
        assert!(!pubkey.key_id.is_empty());
        assert_eq!(pubkey.jwk["kty"], "OKP");
        assert_eq!(pubkey.jwk["crv"], "Ed25519");
        assert!(pubkey.jwk["x"].is_string());
    }

    #[test]
    fn derive_public_key_matches_config_keyid() {
        let seed = [9u8; 32];
        let encoded = URL_SAFE_NO_PAD.encode(seed);
        let pubkey = derive_bot_auth_public_key(&encoded).unwrap();
        let config = BotAuthConfig::from_seed(seed);
        assert_eq!(pubkey.key_id, config.keyid());
    }

    #[test]
    fn seed_zeroized_on_drop() {
        let mut slot = std::mem::MaybeUninit::new(BotAuthConfig::from_seed([0xAB; 32]));
        let cfg_ptr = slot.as_mut_ptr();
        let seed_ptr = unsafe { std::ptr::addr_of_mut!((*cfg_ptr).seed) };

        unsafe { std::ptr::drop_in_place(cfg_ptr) };
        let seed_after_drop = unsafe { std::ptr::read(seed_ptr) };
        assert_eq!(seed_after_drop, [0u8; 32]);
    }

    #[test]
    fn debug_redacts_key_material() {
        let seed = [0xABu8; 32];
        let config = BotAuthConfig::from_seed(seed);
        let debug = format!("{config:?}");
        assert!(debug.contains("[REDACTED]"));
        assert!(!debug.contains("171"));
    }
}
