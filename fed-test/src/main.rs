use std::sync::Arc;

use anyhow::Context;
use axum::{Json, Router, extract::State, routing::post};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, Notify};
use uuid::Uuid;

#[derive(Serialize)]
struct AuthRequest {
    requester_instance: String,
    use_https: bool,
    nonce: String,
}

#[derive(Debug, Deserialize)]
struct AuthGrant {
    issuer_instance: String,
    token: String,
    expires_at: i64,
    #[allow(dead_code)]
    nonce: String,
}

#[derive(Clone)]
struct AppState {
    token: Arc<Mutex<Option<String>>>,
    notify: Arc<Notify>,
}

async fn auth_grant_handler(
    State(state): State<AppState>,
    Json(grant): Json<AuthGrant>,
) -> Json<serde_json::Value> {
    tracing::info!(
        issuer = %grant.issuer_instance,
        expires_at = grant.expires_at,
        "Federation grant received"
    );
    *state.token.lock().await = Some(grant.token);
    state.notify.notify_one();
    Json(serde_json::json!({ "stored": true }))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,fed_test=debug".into()),
        )
        .init();

    let base_url = std::env::var("BASE_URL").context("BASE_URL is required")?;
    let host = std::env::var("HOST").context("HOST is required")?;

    let token_store: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let notify = Arc::new(Notify::new());

    let state = AppState {
        token: token_store.clone(),
        notify: notify.clone(),
    };

    let app = Router::new()
        .route("/api/federation/auth/grant", post(auth_grant_handler))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:80")
        .await
        .context("Failed to bind port 80 — try running as root or with CAP_NET_BIND_SERVICE")?;
    tracing::info!("Listening on 0.0.0.0:80");

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    // Send the auth request
    let nonce = Uuid::new_v4().to_string();
    let request_url = format!(
        "{}/api/federation/auth/request",
        base_url.trim_end_matches('/')
    );

    tracing::info!(url = %request_url, "Sending federation auth request");

    let resp = Client::new()
        .post(&request_url)
        .json(&AuthRequest {
            requester_instance: host.clone(),
            use_https: false,
            nonce,
        })
        .send()
        .await
        .context("Failed to send auth request")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("Auth request rejected: {} — {}", status, body);
    }

    tracing::info!(status = %resp.status(), "Auth request accepted — waiting for grant callback");

    notify.notified().await;

    let token = token_store.lock().await.clone().unwrap();

    println!("\n=== Federation Token ===\n{token}\n========================\n");

    // Keep the server running so the backend can re-issue tokens if needed.
    tokio::signal::ctrl_c().await?;

    Ok(())
}
