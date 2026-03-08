//! Direct PostgreSQL health check server
//!
//! This server bypasses Patroni's REST API and checks PostgreSQL directly.
//! It solves the problem where Patroni's API becomes unresponsive when etcd
//! is slow (since Patroni uses a single thread for both etcd and API handling).
//!
//! Endpoints:
//! - GET /primary - Returns 200 if PostgreSQL is primary (not in recovery)
//! - GET /replica - Returns 200 if PostgreSQL is replica (in recovery)
//! - GET /health  - Returns 200 if PostgreSQL is accepting connections

use std::net::SocketAddr;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::process::Command;
use tokio::sync::RwLock;
use tracing::{debug, info};

/// Health server port - separate from Patroni's 8008
pub const HEALTH_SERVER_PORT: u16 = 8009;

/// Cached health state to avoid hammering PostgreSQL
#[derive(Debug, Clone)]
struct HealthState {
    /// PostgreSQL is accepting connections
    is_healthy: bool,
    /// PostgreSQL is in recovery mode (replica)
    is_in_recovery: bool,
    /// Last check timestamp
    last_check: std::time::Instant,
}

impl Default for HealthState {
    fn default() -> Self {
        Self {
            is_healthy: false,
            is_in_recovery: true,
            last_check: std::time::Instant::now(),
        }
    }
}

/// Check if PostgreSQL is accepting connections using pg_isready
async fn check_pg_ready() -> bool {
    let result = Command::new("pg_isready")
        .args(["-h", "localhost", "-p", "5432", "-q"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await;

    match result {
        Ok(status) => status.success(),
        Err(e) => {
            debug!(error = %e, "pg_isready failed");
            false
        }
    }
}

/// Check if PostgreSQL is in recovery mode (replica) using psql
async fn check_is_in_recovery(superuser: &str) -> Option<bool> {
    let result = Command::new("psql")
        .args([
            "-h", "localhost",
            "-p", "5432",
            "-U", superuser,
            "-d", "postgres",
            "-tAc", "SELECT pg_is_in_recovery();"
        ])
        .env("PGCONNECT_TIMEOUT", "2")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .await;

    match result {
        Ok(output) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let value = stdout.trim();
            match value {
                "t" => Some(true),
                "f" => Some(false),
                _ => {
                    debug!(value = %value, "Unexpected pg_is_in_recovery response");
                    None
                }
            }
        }
        Ok(output) => {
            debug!(status = %output.status, "psql pg_is_in_recovery failed");
            None
        }
        Err(e) => {
            debug!(error = %e, "psql command failed");
            None
        }
    }
}

/// Background task that periodically checks PostgreSQL health
async fn health_check_loop(
    state: Arc<RwLock<HealthState>>,
    superuser: String,
    shutdown: Arc<AtomicBool>,
) {
    let check_interval = Duration::from_secs(1);

    while !shutdown.load(Ordering::Relaxed) {
        // Check if PostgreSQL is ready
        let is_healthy = check_pg_ready().await;

        // Only check recovery status if PostgreSQL is healthy
        let is_in_recovery = if is_healthy {
            check_is_in_recovery(&superuser).await.unwrap_or(true)
        } else {
            true // Assume replica if we can't check
        };

        // Update cached state
        {
            let mut state = state.write().await;
            state.is_healthy = is_healthy;
            state.is_in_recovery = is_in_recovery;
            state.last_check = std::time::Instant::now();
        }

        debug!(
            is_healthy = is_healthy,
            is_in_recovery = is_in_recovery,
            "Health check completed"
        );

        tokio::time::sleep(check_interval).await;
    }
}

/// Simple HTTP response
fn http_response(status: u16, body: &str) -> String {
    let status_text = match status {
        200 => "OK",
        503 => "Service Unavailable",
        404 => "Not Found",
        _ => "Unknown",
    };

    format!(
        "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        status,
        status_text,
        body.len(),
        body
    )
}

/// Handle a single HTTP request
async fn handle_request(
    request: &str,
    state: &Arc<RwLock<HealthState>>,
) -> String {
    // Parse the request line
    let first_line = request.lines().next().unwrap_or("");
    let parts: Vec<&str> = first_line.split_whitespace().collect();

    if parts.len() < 2 {
        return http_response(400, r#"{"error": "Bad request"}"#);
    }

    let method = parts[0];
    let path = parts[1];

    // Only support GET, HEAD, and OPTIONS
    if !matches!(method, "GET" | "HEAD" | "OPTIONS") {
        return http_response(405, r#"{"error": "Method not allowed"}"#);
    }

    let state = state.read().await;
    let stale = state.last_check.elapsed() > Duration::from_secs(5);

    match path {
        "/health" => {
            if state.is_healthy && !stale {
                http_response(200, r#"{"status": "healthy"}"#)
            } else {
                http_response(503, r#"{"status": "unhealthy"}"#)
            }
        }
        "/primary" => {
            if state.is_healthy && !state.is_in_recovery && !stale {
                http_response(200, r#"{"role": "primary", "state": "running"}"#)
            } else {
                http_response(503, r#"{"role": "replica", "state": "running"}"#)
            }
        }
        "/replica" => {
            if state.is_healthy && state.is_in_recovery && !stale {
                http_response(200, r#"{"role": "replica", "state": "running"}"#)
            } else {
                http_response(503, r#"{"role": "primary", "state": "running"}"#)
            }
        }
        "/" => {
            let role = if state.is_in_recovery { "replica" } else { "primary" };
            let status = if state.is_healthy && !stale { "running" } else { "unhealthy" };
            http_response(
                200,
                &format!(r#"{{"role": "{}", "state": "{}"}}"#, role, status),
            )
        }
        _ => http_response(404, r#"{"error": "Not found"}"#),
    }
}

/// Run the health check HTTP server
pub async fn run_health_server(superuser: String) -> anyhow::Result<()> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    let addr = SocketAddr::from(([0, 0, 0, 0], HEALTH_SERVER_PORT));
    let listener = TcpListener::bind(addr).await?;

    info!(port = HEALTH_SERVER_PORT, "Direct health server started");

    let state = Arc::new(RwLock::new(HealthState::default()));
    let shutdown = Arc::new(AtomicBool::new(false));

    // Spawn background health check loop
    let check_state = state.clone();
    let check_shutdown = shutdown.clone();
    tokio::spawn(async move {
        health_check_loop(check_state, superuser, check_shutdown).await;
    });

    loop {
        let (mut socket, _) = listener.accept().await?;
        let state = state.clone();

        tokio::spawn(async move {
            let mut buf = [0u8; 1024];
            match socket.read(&mut buf).await {
                Ok(n) if n > 0 => {
                    let request = String::from_utf8_lossy(&buf[..n]);
                    let response = handle_request(&request, &state).await;
                    let _ = socket.write_all(response.as_bytes()).await;
                }
                _ => {}
            }
        });
    }
}
