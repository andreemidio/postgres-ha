//! Patroni runner - Wrapper to run Patroni with proper setup
//!
//! Generates Patroni configuration and starts Patroni.
//! Runs as PID 1 in container with built-in health monitoring.

use anyhow::{Context, Result};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use common::init_logging;
use nix::sys::stat::{umask, Mode};
use postgres_patroni::health_server::{self, HealthServerConfig};
use postgres_patroni::patroni::{
    generate_patroni_config, run_monitoring_loop, update_pg_hba_for_replication, Config,
};
use postgres_patroni::{volume_root, Telemetry};
use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::net::ToSocketAddrs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::process::Command;
use tracing::{error, info, warn};

/// Request body for etcd v3 range API
#[derive(Serialize)]
struct EtcdRangeRequest {
    key: String,
}

/// Response from etcd v3 range API
#[derive(Deserialize)]
struct EtcdRangeResponse {
    #[serde(default)]
    kvs: Option<Vec<EtcdKeyValue>>,
}

/// Key-value pair from etcd
#[derive(Deserialize)]
struct EtcdKeyValue {
    /// Base64 encoded key
    #[allow(dead_code)]
    key: String,
    /// Base64 encoded value
    value: String,
}

/// Member data stored in etcd by Patroni
#[derive(Deserialize)]
struct PatroniMemberData {
    /// Connection URL for PostgreSQL
    conn_url: Option<String>,
}

/// Fetch a key from etcd, trying each host until one succeeds.
/// Returns the decoded value if found, None if not found.
async fn fetch_etcd_key(
    client: &reqwest::Client,
    etcd_hosts: &[&str],
    key: &str,
) -> Option<String> {
    let key_base64 = BASE64.encode(key.as_bytes());

    for host in etcd_hosts {
        let url = format!("http://{}/v3/kv/range", host.trim());
        let request = EtcdRangeRequest {
            key: key_base64.clone(),
        };

        match client.post(&url).json(&request).send().await {
            Ok(response) if response.status().is_success() => {
                if let Ok(range_response) = response.json::<EtcdRangeResponse>().await {
                    if let Some(kvs) = range_response.kvs {
                        if let Some(kv) = kvs.first() {
                            // Decode the base64 value
                            if let Ok(decoded) = BASE64.decode(&kv.value) {
                                if let Ok(value) = String::from_utf8(decoded) {
                                    return Some(value);
                                }
                            }
                        }
                    }
                }
                return None; // Key doesn't exist
            }
            Ok(response) => {
                warn!(
                    host = %host,
                    status = %response.status(),
                    "etcd returned non-success status"
                );
            }
            Err(e) => {
                warn!(host = %host, error = %e, "Failed to connect to etcd");
            }
        }
    }
    None
}

/// Wait for the Patroni cluster to exist in etcd before starting.
/// This prevents replicas from racing with the primary during initial setup.
/// Only the primary (with existing data) should be allowed to initialize the cluster.
///
/// We wait for TWO conditions:
/// 1. The leader key exists (/service/{scope}/leader) - contains leader name
/// 2. The leader's member key has valid conn_url (/service/{scope}/members/{leader})
///
/// This prevents a race condition where the replica starts before the leader
/// has fully registered its connection info, causing pg_basebackup to fail.
async fn wait_for_cluster_in_etcd(config: &Config) -> Result<()> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .context("Failed to create HTTP client")?;

    // Parse etcd hosts - format is "host1:port1,host2:port2,..."
    let etcd_hosts: Vec<&str> = config.etcd_hosts.split(',').collect();

    let max_wait = Duration::from_secs(300); // 5 minute max wait
    let poll_interval = Duration::from_secs(2);
    let start = std::time::Instant::now();

    info!(
        scope = %config.scope,
        "Waiting for cluster to be initialized by primary before starting..."
    );

    loop {
        if start.elapsed() > max_wait {
            anyhow::bail!(
                "Timeout waiting for cluster '{}' to be initialized in etcd after {:?}",
                config.scope,
                max_wait
            );
        }

        // Step 1: Check if leader key exists and get leader name
        let leader_key = format!("/service/{}/leader", config.scope);
        if let Some(leader_name) = fetch_etcd_key(&client, &etcd_hosts, &leader_key).await {
            info!(leader = %leader_name, "Found leader in etcd");

            // Step 2: Check if leader's member data has conn_url
            let member_key = format!("/service/{}/members/{}", config.scope, leader_name);
            if let Some(member_data_json) = fetch_etcd_key(&client, &etcd_hosts, &member_key).await
            {
                // Parse member data to check for conn_url
                if let Ok(member_data) =
                    serde_json::from_str::<PatroniMemberData>(&member_data_json)
                {
                    if let Some(conn_url) = &member_data.conn_url {
                        if !conn_url.is_empty() {
                            info!(
                                scope = %config.scope,
                                leader = %leader_name,
                                conn_url = %conn_url,
                                elapsed = ?start.elapsed(),
                                "Leader has valid conn_url, proceeding to start Patroni"
                            );
                            return Ok(());
                        }
                        info!(
                            leader = %leader_name,
                            "Leader member data exists but conn_url is empty, waiting..."
                        );
                    } else {
                        info!(
                            leader = %leader_name,
                            "Leader member data exists but missing conn_url, waiting..."
                        );
                    }
                } else {
                    warn!(
                        leader = %leader_name,
                        data = %member_data_json,
                        "Failed to parse leader member data, waiting..."
                    );
                }
            } else {
                info!(
                    leader = %leader_name,
                    "Leader exists but member data not yet available, waiting..."
                );
            }
        } else {
            info!(
                elapsed = ?start.elapsed(),
                "Cluster not yet initialized (no leader), waiting..."
            );
        }

        tokio::time::sleep(poll_interval).await;
    }
}

async fn start_patroni() -> Result<tokio::process::Child> {
    let child = Command::new("patroni")
        .arg("/etc/patroni/patroni.yml")
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .context("Failed to start patroni")?;

    Ok(child)
}

#[tokio::main]
async fn main() -> Result<()> {
    let _guard = init_logging("patroni-runner");

    // Capture health server config BEFORE clearing PG* env vars
    let health_config = HealthServerConfig::from_env();

    let telemetry = Telemetry::from_env("postgres-ha");
    let config = Config::from_env()?;

    info!(
        node = %config.name,
        address = %config.connect_address,
        "=== Patroni Runner ==="
    );

    let volume_root = volume_root();
    let bootstrap_marker = format!("{}/.patroni_bootstrap_complete", volume_root);

    // Handle data adoption from vanilla PostgreSQL
    if config.adopt_existing_data {
        update_pg_hba_for_replication(&config)?;
    }

    let pg_control_path = format!("{}/global/pg_control", config.data_dir);
    let has_pg_control = Path::new(&pg_control_path).exists();
    let has_marker = Path::new(&bootstrap_marker).exists();

    if config.adopt_existing_data && has_pg_control && !has_marker {
        info!("PATRONI_ADOPT_EXISTING_DATA=true - migrating from vanilla PostgreSQL");
        fs::write(&bootstrap_marker, "").context("Failed to create bootstrap marker")?;
    } else if has_pg_control && has_marker {
        info!("Found valid data with bootstrap marker");
    } else if has_pg_control {
        info!("Found pg_control but NO bootstrap marker - stale data");
    } else {
        info!("No PostgreSQL data found");
    }

    // Prevent race condition during HA conversion:
    // When PATRONI_WAIT_FOR_LEADER=true, this replica waits for the primary to
    // establish leadership before starting. This prevents empty replicas from
    // winning the election and causing data loss during conversion.
    // Only used during conversion when postgres-1 has existing data to preserve.
    if config.wait_for_leader && !has_pg_control {
        wait_for_cluster_in_etcd(&config).await?;
    }

    // Generate and write Patroni config
    let patroni_config = generate_patroni_config(&config);
    fs::create_dir_all("/etc/patroni").context("Failed to create /etc/patroni directory")?;
    fs::write("/etc/patroni/patroni.yml", &patroni_config).context("Failed to write patroni.yml")?;

    info!(
        scope = %config.scope,
        etcd = %config.etcd_hosts,
        "Starting Patroni"
    );

    // Prepare data directory
    fs::create_dir_all(&config.data_dir).context("Failed to create data directory")?;
    fs::set_permissions(&config.data_dir, std::fs::Permissions::from_mode(0o700))
        .context("Failed to set data directory permissions")?;

    // Clear PostgreSQL environment variables to avoid conflicts
    env::remove_var("PGPASSWORD");
    env::remove_var("PGUSER");
    env::remove_var("PGHOST");
    env::remove_var("PGPORT");
    env::remove_var("PGDATABASE");

    // Set umask so pg_basebackup creates files with correct permissions (0600/0700)
    // Without this, container environments may create files too permissive for PostgreSQL
    umask(Mode::from_bits_truncate(0o077));

    // Start health server for HAProxy health checks
    // This runs independently and queries PostgreSQL directly for primary/replica status
    let _health_handle = health_server::start(health_config).await?;

    // Start Patroni and run monitoring loop
    let child = start_patroni().await?;
    run_monitoring_loop(&config, child, &telemetry).await
}
