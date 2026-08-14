use std::{collections::BTreeMap, fmt};

use aes_gcm::{
    Aes256Gcm, Nonce,
    aead::{Aead, KeyInit, Payload},
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use rand::RngExt as _;
use secrecy::{ExposeSecret as _, SecretString};
use thiserror::Error;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CredentialContext {
    pub organization_id: String,
    pub connection_id: String,
    pub provider: String,
}

impl CredentialContext {
    fn aad(&self, key_version: u32) -> Vec<u8> {
        format!(
            "featherlane/source-credential/v1\0{}\0{}\0{}\0{key_version}",
            self.organization_id, self.connection_id, self.provider
        )
        .into_bytes()
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct EncryptedCredential {
    pub ciphertext: Vec<u8>,
    pub nonce: [u8; 12],
    pub key_version: u32,
}

impl fmt::Debug for EncryptedCredential {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("EncryptedCredential")
            .field("ciphertext", &"<redacted>")
            .field("nonce", &"<redacted>")
            .field("key_version", &self.key_version)
            .finish()
    }
}

#[derive(Clone)]
pub struct DecryptedCredential {
    pub plaintext: SecretString,
    pub needs_reencrypt: bool,
}

impl fmt::Debug for DecryptedCredential {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DecryptedCredential")
            .field("plaintext", &"<redacted>")
            .field("needs_reencrypt", &self.needs_reencrypt)
            .finish()
    }
}

#[derive(Debug, Error)]
pub enum CredentialCipherError {
    #[error("credential cipher is not configured")]
    NotConfigured,
    #[error("credential encryption failed")]
    Encryption,
    #[error("credential decryption failed")]
    Decryption,
}

#[derive(Clone)]
pub struct CredentialCipher {
    keys: BTreeMap<u32, [u8; 32]>,
    active_version: u32,
}

impl fmt::Debug for CredentialCipher {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CredentialCipher")
            .field("keys", &format_args!("<{} configured>", self.keys.len()))
            .field("active_version", &self.active_version)
            .finish()
    }
}

impl CredentialCipher {
    /// Builds a versioned cipher from base64-encoded 256-bit keys.
    ///
    /// # Errors
    ///
    /// Returns [`CredentialCipherError::NotConfigured`] when a key is not valid base64,
    /// is not 256 bits, or the active version is absent.
    pub fn from_base64_keys(
        encoded: &BTreeMap<u32, SecretString>,
        active_version: u32,
    ) -> Result<Self, CredentialCipherError> {
        let mut keys = BTreeMap::new();
        for (version, value) in encoded {
            let bytes = STANDARD
                .decode(value.expose_secret().as_bytes())
                .map_err(|_| CredentialCipherError::NotConfigured)?;
            let key: [u8; 32] = bytes
                .try_into()
                .map_err(|_| CredentialCipherError::NotConfigured)?;
            keys.insert(*version, key);
        }
        if !keys.contains_key(&active_version) {
            return Err(CredentialCipherError::NotConfigured);
        }
        Ok(Self {
            keys,
            active_version,
        })
    }

    /// Encrypts one credential with a fresh nonce and context-bound associated data.
    ///
    /// # Errors
    ///
    /// Returns an error when the active key is unavailable or encryption fails.
    pub fn encrypt(
        &self,
        context: &CredentialContext,
        plaintext: &SecretString,
    ) -> Result<EncryptedCredential, CredentialCipherError> {
        let key = self
            .keys
            .get(&self.active_version)
            .ok_or(CredentialCipherError::NotConfigured)?;
        let cipher =
            Aes256Gcm::new_from_slice(key).map_err(|_| CredentialCipherError::NotConfigured)?;
        let mut nonce = [0_u8; 12];
        rand::rng().fill(&mut nonce);
        let nonce_value =
            Nonce::try_from(nonce.as_slice()).map_err(|_| CredentialCipherError::Encryption)?;
        let ciphertext = cipher
            .encrypt(
                &nonce_value,
                Payload {
                    msg: plaintext.expose_secret().as_bytes(),
                    aad: &context.aad(self.active_version),
                },
            )
            .map_err(|_| CredentialCipherError::Encryption)?;
        Ok(EncryptedCredential {
            ciphertext,
            nonce,
            key_version: self.active_version,
        })
    }

    /// Decrypts one credential and reports whether it should be rotated to the active key.
    ///
    /// # Errors
    ///
    /// Returns an error when the stored key version is unavailable, authentication fails,
    /// or the plaintext is not valid UTF-8.
    pub fn decrypt(
        &self,
        context: &CredentialContext,
        encrypted: &EncryptedCredential,
    ) -> Result<DecryptedCredential, CredentialCipherError> {
        let key = self
            .keys
            .get(&encrypted.key_version)
            .ok_or(CredentialCipherError::Decryption)?;
        let cipher =
            Aes256Gcm::new_from_slice(key).map_err(|_| CredentialCipherError::Decryption)?;
        let nonce = Nonce::try_from(encrypted.nonce.as_slice())
            .map_err(|_| CredentialCipherError::Decryption)?;
        let plaintext = cipher
            .decrypt(
                &nonce,
                Payload {
                    msg: &encrypted.ciphertext,
                    aad: &context.aad(encrypted.key_version),
                },
            )
            .map_err(|_| CredentialCipherError::Decryption)?;
        let plaintext =
            String::from_utf8(plaintext).map_err(|_| CredentialCipherError::Decryption)?;
        Ok(DecryptedCredential {
            plaintext: SecretString::from(plaintext),
            needs_reencrypt: encrypted.key_version != self.active_version,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cipher(version: u32) -> CredentialCipher {
        let mut keys = BTreeMap::new();
        keys.insert(
            version,
            SecretString::from(STANDARD.encode([u8::try_from(version).unwrap_or(7); 32])),
        );
        CredentialCipher::from_base64_keys(&keys, version).expect("test key should be valid")
    }

    #[test]
    fn credentials_round_trip_only_in_the_bound_context() {
        let context = CredentialContext {
            organization_id: "organization-1".to_owned(),
            connection_id: "connection-1".to_owned(),
            provider: "google_drive".to_owned(),
        };
        let encrypted = cipher(1)
            .encrypt(&context, &SecretString::from("refresh-token".to_owned()))
            .expect("encryption should succeed");
        let decrypted = cipher(1)
            .decrypt(&context, &encrypted)
            .expect("decryption should succeed");
        assert_eq!(decrypted.plaintext.expose_secret(), "refresh-token");

        let wrong_context = CredentialContext {
            connection_id: "connection-2".to_owned(),
            ..context
        };
        assert!(cipher(1).decrypt(&wrong_context, &encrypted).is_err());
        assert!(!format!("{encrypted:?}").contains("refresh-token"));
    }

    #[test]
    fn older_keys_decrypt_and_request_rotation() {
        let mut encoded = BTreeMap::new();
        encoded.insert(1, SecretString::from(STANDARD.encode([1_u8; 32])));
        encoded.insert(2, SecretString::from(STANDARD.encode([2_u8; 32])));
        let old = CredentialCipher::from_base64_keys(&encoded, 1).expect("old keyring");
        let active = CredentialCipher::from_base64_keys(&encoded, 2).expect("active keyring");
        let context = CredentialContext {
            organization_id: "organization-1".to_owned(),
            connection_id: "connection-1".to_owned(),
            provider: "notion".to_owned(),
        };
        let encrypted = old
            .encrypt(&context, &SecretString::from("token".to_owned()))
            .expect("encryption should succeed");
        assert!(
            active
                .decrypt(&context, &encrypted)
                .expect("old key should decrypt")
                .needs_reencrypt
        );
    }
}
