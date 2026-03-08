//! HAProxy configuration generator and launcher
//!
//! Generates HAProxy configuration dynamically from PostgreSQL node information
//! in environment variables. Uses direct PostgreSQL health checks via
//! pg_is_in_recovery() to bypass Patroni REST API blocking issues.

mod config;
mod monitoring;
mod nodes;
mod template;

use anyhow::{Context, Result};
use common::{init_logging, Telemetry, TelemetryEvent};
use std::env;
use std::fs;
use std::process::Command;
use tracing::info;

use config::Config;
use monitoring::run_monitoring_loop;
use nodes::parse_nodes;
use template::generate_config;

const CONFIG_FILE: &str = "/usr/local/etc/haproxy/haproxy.cfg";
const PG_ENV_FILE: &str = "/tmp/pg_env.sh";

fn main() -> Result<()> {
    let _guard = init_logging("haproxy");

    let telemetry = Telemetry::from_env("haproxy");
    let config = Config::from_env()?;
    let nodes = parse_nodes(&config.postgres_nodes)?;
    let single_node_mode = nodes.len() == 1;

    info!(
        nodes = %config.postgres_nodes,
        count = nodes.len(),
        "Generating HAProxy config"
    );

    if single_node_mode {
        info!("Single node mode: routing directly without role checks");
    } else if config.use_pgsql_check {
        info!("Multi-node mode: using pg_is_in_recovery() health checks (bypasses Patroni API)");
    } else {
        info!("Multi-node mode: using Patroni REST API health checks");
    }

    telemetry.send(TelemetryEvent::HaproxyConfigGenerating {
        nodes: nodes.iter().map(|n| n.name.clone()).collect(),
    });

    let haproxy_config = generate_config(&config, &nodes);

    fs::write(CONFIG_FILE, &haproxy_config).context("Failed to write HAProxy config")?;
    info!(path = CONFIG_FILE, "Config written");

    // Log config for debugging
    for line in haproxy_config.lines() {
        info!("  {}", line);
    }

    // Write PostgreSQL credentials to a file for external-check scripts
    // HAProxy external-check runs in a restricted environment without container env vars
    if config.use_pgsql_check {
        let pguser = env::var("PGUSER").unwrap_or_else(|_| "postgres".to_string());
        let pgpassword = env::var("PGPASSWORD").unwrap_or_default();
        let pgport = env::var("PGPORT").unwrap_or_else(|_| "5432".to_string());

        let pg_env = format!(
            "export PGUSER='{}'\nexport PGPASSWORD='{}'\nexport PGPORT='{}'\n",
            pguser.replace('\'', "'\"'\"'"),
            pgpassword.replace('\'', "'\"'\"'"),
            pgport
        );

        fs::write(PG_ENV_FILE, &pg_env).context("Failed to write pg_env.sh")?;
        info!(path = PG_ENV_FILE, "PostgreSQL credentials written for health checks");
    }

    telemetry.send(TelemetryEvent::HaproxyStarted {
        node_count: nodes.len(),
        single_node_mode,
    });

    info!("Starting HAProxy...");

    let child = Command::new("haproxy")
        .arg("-f")
        .arg(CONFIG_FILE)
        .spawn()
        .context("Failed to spawn haproxy")?;

    run_monitoring_loop(child, &telemetry, single_node_mode)
}
