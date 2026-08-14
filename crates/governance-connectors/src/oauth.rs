use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use governance_domain::SourceProvider;
use rand::RngExt as _;
use secrecy::{ExposeSecret as _, SecretString};
use serde::Deserialize;
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OAuthProof {
    pub state: String,
    pub state_hash: String,
    pub pkce_verifier: String,
    pub pkce_challenge: String,
}

#[must_use]
pub fn new_oauth_proof() -> OAuthProof {
    let mut state_bytes = [0_u8; 32];
    let mut verifier_bytes = [0_u8; 64];
    rand::rng().fill(&mut state_bytes);
    rand::rng().fill(&mut verifier_bytes);
    let state = URL_SAFE_NO_PAD.encode(state_bytes);
    let pkce_verifier = URL_SAFE_NO_PAD.encode(verifier_bytes);
    OAuthProof {
        state_hash: hex_digest(state.as_bytes()),
        pkce_challenge: URL_SAFE_NO_PAD.encode(Sha256::digest(pkce_verifier.as_bytes())),
        state,
        pkce_verifier,
    }
}

#[must_use]
pub fn hex_digest(value: &[u8]) -> String {
    format!("{:x}", Sha256::digest(value))
}

#[derive(Clone, Debug)]
pub struct OAuthClientCredentials {
    pub client_id: String,
    pub client_secret: SecretString,
}

#[derive(Clone, Debug, Deserialize)]
pub struct RefreshedOAuthToken {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub token_type: Option<String>,
    pub scope: Option<String>,
    pub expires_in: Option<i64>,
}

/// Exchanges a stored provider refresh token for a fresh access token.
///
/// # Errors
///
/// Returns a classified connector error when the provider rejects the token, is unavailable,
/// or returns an invalid response.
pub async fn refresh_provider_token(
    provider: SourceProvider,
    credentials: &OAuthClientCredentials,
    refresh_token: &str,
) -> Result<RefreshedOAuthToken, crate::ConnectorError> {
    let endpoint = match provider {
        SourceProvider::GoogleDrive => "https://oauth2.googleapis.com/token",
        SourceProvider::MicrosoftGraph => {
            "https://login.microsoftonline.com/common/oauth2/v2.0/token"
        }
        SourceProvider::Notion => "https://api.notion.com/v1/oauth/token",
    };
    let request = reqwest::Client::new().post(endpoint);
    let response = if provider == SourceProvider::Notion {
        request
            .basic_auth(
                &credentials.client_id,
                Some(credentials.client_secret.expose_secret()),
            )
            .json(&serde_json::json!({
                "grant_type": "refresh_token",
                "refresh_token": refresh_token,
            }))
            .send()
            .await
    } else {
        request
            .form(&[
                ("grant_type", "refresh_token"),
                ("refresh_token", refresh_token),
                ("client_id", credentials.client_id.as_str()),
                ("client_secret", credentials.client_secret.expose_secret()),
            ])
            .send()
            .await
    }
    .map_err(|_| crate::ConnectorError::retryable("provider_unavailable"))?;
    if !response.status().is_success() {
        return Err(match response.status().as_u16() {
            400 | 401 => crate::ConnectorError::reauthorize(),
            429 => crate::ConnectorError::retryable("provider_rate_limited"),
            _ if response.status().is_server_error() => {
                crate::ConnectorError::retryable("provider_unavailable")
            }
            _ => crate::ConnectorError::terminal("provider_refresh_failed"),
        });
    }
    response
        .json()
        .await
        .map_err(|_| crate::ConnectorError::terminal("provider_response_invalid"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oauth_proof_binds_hash_and_s256_challenge() {
        let proof = new_oauth_proof();
        assert_eq!(proof.state_hash, hex_digest(proof.state.as_bytes()));
        assert_eq!(
            proof.pkce_challenge,
            URL_SAFE_NO_PAD.encode(Sha256::digest(proof.pkce_verifier.as_bytes()))
        );
        assert_ne!(proof, new_oauth_proof());
    }
}
