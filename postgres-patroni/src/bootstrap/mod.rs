//! Post-bootstrap components
//!
//! This module provides functionality for the post-bootstrap script:
//! - Reading credentials from Patroni config
//! - SQL execution helpers

mod config;
mod extensions;
mod sql;

pub use config::{Credentials, read_credentials, PATRONI_CONFIG};
pub use extensions::{run_extensions, run_hypertables, run_schemas};
pub use sql::{run_psql, run_psql_in_db, run_psql_script};
