//! Google OAuth 2.0 for installed apps: PKCE + loopback redirect.
//! https://developers.google.com/identity/protocols/oauth2/native-app

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Deserialize;
use tauri_plugin_opener::OpenerExt;
use tokio::net::TcpListener;

use crate::db::Db;
use crate::error::{AppError, Result};
use crate::loopback::{pkce_challenge, random_urlsafe, wait_for_code};
use crate::{secrets, settings};

const AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const REVOKE_URL: &str = "https://oauth2.googleapis.com/revoke";
// `gmail.modify` covers read, send, label changes and trash; it excludes
// permanent delete, which the app never does. `gmail.settings.basic` is
// needed to create and delete filters.
const SCOPES: &str = "https://www.googleapis.com/auth/gmail.modify https://www.googleapis.com/auth/gmail.settings.basic";

fn secret_key(email: &str) -> String {
    format!("gmail:refresh:{email}")
}

#[derive(Clone)]
struct CachedToken {
    token: String,
    expires_at: Instant,
}

pub struct GmailAuth {
    db: Arc<Db>,
    http: reqwest::Client,
    cache: Mutex<HashMap<String, CachedToken>>,
}

pub struct FreshTokens {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_in: u64,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    expires_in: u64,
    refresh_token: Option<String>,
}

#[derive(Deserialize)]
struct ErrorResponse {
    error: String,
    error_description: Option<String>,
}

impl GmailAuth {
    pub fn new(db: Arc<Db>, http: reqwest::Client) -> Self {
        Self {
            db,
            http,
            cache: Mutex::new(HashMap::new()),
        }
    }

    fn client_credentials(&self) -> Result<(String, String)> {
        let id = settings::get(&self.db, settings::GOOGLE_CLIENT_ID)?.unwrap_or_default();
        let secret = secrets::get(secrets::GOOGLE_CLIENT_SECRET)?.unwrap_or_default();
        if id.trim().is_empty() {
            return Err(AppError::NotConfigured(
                "Google OAuth client ID is not set. Add it under Settings.".into(),
            ));
        }
        Ok((id.trim().to_string(), secret.trim().to_string()))
    }

    /// Runs the full interactive consent flow. The caller uses the access
    /// token to look up which address was authorised, then persists the
    /// tokens with [`store_tokens`](Self::store_tokens).
    pub async fn login(&self, app: &tauri::AppHandle) -> Result<FreshTokens> {
        let (client_id, client_secret) = self.client_credentials()?;

        let verifier = random_urlsafe(64);
        let challenge = pkce_challenge(&verifier);
        let state = random_urlsafe(24);

        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let redirect_uri = format!("http://127.0.0.1:{}", listener.local_addr()?.port());

        let mut auth = url::Url::parse(AUTH_URL).expect("static url");
        auth.query_pairs_mut()
            .append_pair("client_id", &client_id)
            .append_pair("redirect_uri", &redirect_uri)
            .append_pair("response_type", "code")
            .append_pair("scope", SCOPES)
            .append_pair("code_challenge", &challenge)
            .append_pair("code_challenge_method", "S256")
            .append_pair("state", &state)
            .append_pair("access_type", "offline")
            .append_pair("prompt", "consent");

        app.opener()
            .open_url(auth.as_str(), None::<&str>)
            .map_err(|e| AppError::Other(format!("could not open browser: {e}")))?;

        let code = tokio::time::timeout(Duration::from_secs(300), wait_for_code(listener, &state, "Google"))
            .await
            .map_err(|_| AppError::Auth("timed out waiting for browser sign-in".into()))??;

        let form = [
            ("code", code.as_str()),
            ("client_id", client_id.as_str()),
            ("client_secret", client_secret.as_str()),
            ("redirect_uri", redirect_uri.as_str()),
            ("grant_type", "authorization_code"),
            ("code_verifier", verifier.as_str()),
        ];
        let tokens = self.token_request(&form).await?;
        let refresh = tokens
            .refresh_token
            .ok_or_else(|| AppError::Auth("Google did not return a refresh token".into()))?;
        Ok(FreshTokens {
            access_token: tokens.access_token,
            refresh_token: refresh,
            expires_in: tokens.expires_in,
        })
    }

    pub fn store_tokens(&self, email: &str, tokens: FreshTokens) -> Result<()> {
        secrets::set(&secret_key(email), &tokens.refresh_token)?;
        self.cache.lock().unwrap().insert(
            email.to_string(),
            CachedToken {
                token: tokens.access_token,
                expires_at: Instant::now()
                    + Duration::from_secs(tokens.expires_in.saturating_sub(60)),
            },
        );
        Ok(())
    }

    pub async fn logout(&self, email: &str) -> Result<()> {
        if let Some(refresh) = secrets::get(&secret_key(email))? {
            let _ = self
                .http
                .post(REVOKE_URL)
                .form(&[("token", refresh.as_str())])
                .send()
                .await;
        }
        secrets::delete(&secret_key(email))?;
        self.cache.lock().unwrap().remove(email);
        Ok(())
    }

    pub async fn access_token(&self, email: &str) -> Result<String> {
        if let Some(c) = self.cache.lock().unwrap().get(email) {
            if c.expires_at > Instant::now() {
                return Ok(c.token.clone());
            }
        }
        let refresh = secrets::get(&secret_key(email))?.ok_or_else(|| {
            AppError::Auth(format!("no stored credentials for {email}; sign in again"))
        })?;
        let (client_id, client_secret) = self.client_credentials()?;
        let form = [
            ("refresh_token", refresh.as_str()),
            ("client_id", client_id.as_str()),
            ("client_secret", client_secret.as_str()),
            ("grant_type", "refresh_token"),
        ];
        let tokens = self.token_request(&form).await?;
        self.cache.lock().unwrap().insert(
            email.to_string(),
            CachedToken {
                token: tokens.access_token.clone(),
                expires_at: Instant::now()
                    + Duration::from_secs(tokens.expires_in.saturating_sub(60)),
            },
        );
        Ok(tokens.access_token)
    }

    async fn token_request(&self, form: &[(&str, &str)]) -> Result<TokenResponse> {
        let res = self.http.post(TOKEN_URL).form(form).send().await?;
        if res.status().is_success() {
            Ok(res.json().await?)
        } else {
            let status = res.status();
            let body = res.text().await.unwrap_or_default();
            let msg = serde_json::from_str::<ErrorResponse>(&body)
                .map(|e| {
                    format!(
                        "{}{}",
                        e.error,
                        e.error_description
                            .map(|d| format!(": {d}"))
                            .unwrap_or_default()
                    )
                })
                .unwrap_or_else(|_| format!("HTTP {status}: {body}"));
            Err(AppError::Auth(msg))
        }
    }
}


