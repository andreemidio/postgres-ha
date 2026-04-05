//! Extension, schema, and hypertable setup for post-bootstrap

use anyhow::Result;

use super::config::Credentials;
use super::sql::run_psql_in_db;

/// Create all required PostgreSQL extensions in the app database.
/// timescaledb MUST be created first.
pub fn run_extensions(creds: &Credentials) -> Result<()> {
    let extensions = [
        "CREATE EXTENSION IF NOT EXISTS timescaledb CASCADE",
        "CREATE EXTENSION IF NOT EXISTS postgis CASCADE",
        "CREATE EXTENSION IF NOT EXISTS postgis_raster",
        "CREATE EXTENSION IF NOT EXISTS timescaledb_toolkit",
        "CREATE EXTENSION IF NOT EXISTS pgvector",
        "CREATE EXTENSION IF NOT EXISTS pg_stat_statements",
        "CREATE EXTENSION IF NOT EXISTS pgcrypto",
        r#"CREATE EXTENSION IF NOT EXISTS "uuid-ossp""#,
        "CREATE EXTENSION IF NOT EXISTS pg_trgm",
        "CREATE EXTENSION IF NOT EXISTS btree_gin",
        "CREATE EXTENSION IF NOT EXISTS btree_gist",
    ];

    for sql in &extensions {
        run_psql_in_db(&creds.superuser, &creds.app_db, sql)?;
    }

    Ok(())
}

/// Create application schemas in the app database.
pub fn run_schemas(creds: &Credentials) -> Result<()> {
    let schemas = ["core", "geo", "ts", "analytics"];

    for schema in &schemas {
        let sql = format!("CREATE SCHEMA IF NOT EXISTS {schema}");
        run_psql_in_db(&creds.superuser, &creds.app_db, &sql)?;
    }

    Ok(())
}

/// Create tables and hypertables in the app database.
pub fn run_hypertables(creds: &Credentials) -> Result<()> {
    let db = &creds.app_db;
    let su = &creds.superuser;

    // core.field
    run_psql_in_db(su, db, "CREATE TABLE IF NOT EXISTS core.field (
    id UUID PRIMARY KEY,
    name TEXT NOT NULL
)")?;

    // geo.field_boundary
    run_psql_in_db(su, db, "CREATE TABLE IF NOT EXISTS geo.field_boundary (
    field_id UUID PRIMARY KEY REFERENCES core.field(id),
    geom GEOMETRY(MULTIPOLYGON, 4326) NOT NULL
)")?;

    run_psql_in_db(
        su,
        db,
        "CREATE INDEX IF NOT EXISTS field_boundary_geom_idx ON geo.field_boundary USING GIST (geom)",
    )?;

    // ts.sensor_data
    run_psql_in_db(su, db, "CREATE TABLE IF NOT EXISTS ts.sensor_data (
    time        TIMESTAMPTZ NOT NULL,
    field_id    UUID NOT NULL,
    sensor_type TEXT,
    value       DOUBLE PRECISION,
    metadata    JSONB
)")?;

    // Create hypertable idempotently
    run_psql_in_db(
        su,
        db,
        "SELECT CASE
    WHEN NOT EXISTS (
        SELECT 1 FROM timescaledb_information.hypertables
        WHERE hypertable_schema = 'ts' AND hypertable_name = 'sensor_data'
    )
    THEN create_hypertable('ts.sensor_data', 'time', chunk_time_interval => INTERVAL '6 hours')::text
    ELSE 'already exists'
END",
    )?;

    run_psql_in_db(
        su,
        db,
        "CREATE INDEX IF NOT EXISTS sensor_data_field_time_idx ON ts.sensor_data (field_id, time DESC)",
    )?;

    run_psql_in_db(
        su,
        db,
        "ALTER TABLE ts.sensor_data SET (
    timescaledb.compress,
    timescaledb.compress_segmentby = 'field_id',
    timescaledb.compress_orderby = 'time DESC'
)",
    )?;

    run_psql_in_db(
        su,
        db,
        "SELECT add_compression_policy('ts.sensor_data', INTERVAL '3 days', if_not_exists => true)",
    )?;

    run_psql_in_db(
        su,
        db,
        "CREATE MATERIALIZED VIEW IF NOT EXISTS analytics.sensor_hourly
WITH (timescaledb.continuous) AS
SELECT
    field_id,
    time_bucket('1 hour', time) AS bucket,
    AVG(value) AS avg,
    MAX(value) AS max,
    MIN(value) AS min
FROM ts.sensor_data
GROUP BY field_id, bucket
WITH NO DATA",
    )?;

    run_psql_in_db(
        su,
        db,
        "SELECT add_continuous_aggregate_policy(
    'analytics.sensor_hourly',
    start_offset  => INTERVAL '3 days',
    end_offset    => INTERVAL '10 minutes',
    schedule_interval => INTERVAL '5 minutes',
    if_not_exists => true
)",
    )?;

    Ok(())
}
