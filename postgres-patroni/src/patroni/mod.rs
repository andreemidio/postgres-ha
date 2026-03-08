//! Patroni runner components
//!
//! This module provides the core functionality for running Patroni:
//! - Configuration parsing from environment
//! - YAML config generation
//! - Health checking
//! - Process monitoring
//! - Direct PostgreSQL health server (bypasses Patroni API)

mod config;
mod health;
mod health_server;
mod monitoring;
mod yaml;

pub use config::Config;
pub use health::check_health;
pub use health_server::{run_health_server, HEALTH_SERVER_PORT};
pub use monitoring::run_monitoring_loop;
pub use yaml::{generate_patroni_config, update_pg_hba_for_replication};
