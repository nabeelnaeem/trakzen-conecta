//! Browser sign-in helpers shared by the OAuth-style flows (Gmail, Trakzen
//! Files): a random PKCE verifier/state, and a one-shot loopback listener
//! that receives the redirect carrying the authorisation code.

use std::collections::HashMap;

use base64::Engine;
use rand::RngCore;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use crate::error::{AppError, Result};

pub fn random_urlsafe(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    rand::thread_rng().fill_bytes(&mut buf);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(buf)
}

/// The S256 PKCE challenge for `verifier`.
pub fn pkce_challenge(verifier: &str) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

/// Accepts the single redirect from the browser, validates `state`, and
/// returns the authorisation code. `service` names the other side in error
/// messages ("Google", "Trakzen Files").
pub async fn wait_for_code(listener: TcpListener, expected_state: &str, service: &str) -> Result<String> {
    loop {
        let (mut stream, _) = listener.accept().await?;
        let mut buf = vec![0u8; 8192];
        let n = stream.read(&mut buf).await?;
        let req = String::from_utf8_lossy(&buf[..n]);
        let Some(path) = req.lines().next().and_then(|l| l.split_whitespace().nth(1)) else {
            continue;
        };
        // Browsers also ask for /favicon.ico; ignore anything without a query.
        let Some(query) = path.split_once('?').map(|(_, q)| q) else {
            let _ = stream
                .write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n")
                .await;
            continue;
        };
        let params: HashMap<String, String> = url::form_urlencoded::parse(query.as_bytes()).into_owned().collect();

        let outcome = match (params.get("code"), params.get("state"), params.get("error")) {
            (_, _, Some(err)) if err == "access_denied" => {
                Err(AppError::Auth(format!("access was not allowed in {service}")))
            }
            (_, _, Some(err)) => Err(AppError::Auth(format!("{service} returned '{err}'"))),
            (Some(code), Some(state), _) if state == expected_state => Ok(code.clone()),
            (Some(_), _, _) => Err(AppError::Auth("state mismatch in OAuth redirect".into())),
            _ => Err(AppError::Auth("redirect did not include a code".into())),
        };

        let (title, detail) = match &outcome {
            Ok(_) => ("Signed in", "You can close this tab and return to Trakzen Conecta."),
            Err(_) => ("Sign-in failed", "Return to Trakzen Conecta and try again."),
        };
        let body = format!(
            "<!doctype html><html><head><meta charset=\"utf-8\"><title>{title}</title></head>\
             <body style=\"font-family:sans-serif;padding:3rem;text-align:center\">\
             <h2>{title}</h2><p>{detail}</p></body></html>"
        );
        let resp = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        let _ = stream.write_all(resp.as_bytes()).await;
        let _ = stream.shutdown().await;
        return outcome;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn challenge_matches_rfc7636_example() {
        // Appendix B of RFC 7636.
        assert_eq!(
            pkce_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }
}
