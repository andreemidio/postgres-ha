-- EXTENSÕES (ordem obrigatória: timescaledb antes de postgis)
CREATE EXTENSION IF NOT EXISTS timescaledb CASCADE;
CREATE EXTENSION IF NOT EXISTS postgis CASCADE;
CREATE EXTENSION IF NOT EXISTS postgis_raster;
CREATE EXTENSION IF NOT EXISTS timescaledb_toolkit;
CREATE EXTENSION IF NOT EXISTS pgvector;
CREATE EXTENSION IF NOT EXISTS pg_stat_statements;
CREATE EXTENSION IF NOT EXISTS pgcrypto;
CREATE EXTENSION IF NOT EXISTS "uuid-ossp";
CREATE EXTENSION IF NOT EXISTS pg_trgm;
CREATE EXTENSION IF NOT EXISTS btree_gin;
CREATE EXTENSION IF NOT EXISTS btree_gist;

-- SCHEMAS
CREATE SCHEMA IF NOT EXISTS core;
CREATE SCHEMA IF NOT EXISTS geo;
CREATE SCHEMA IF NOT EXISTS ts;
CREATE SCHEMA IF NOT EXISTS analytics;

-- CORE
CREATE TABLE IF NOT EXISTS core.field (
    id UUID PRIMARY KEY,
    name TEXT NOT NULL
);

-- GEO
CREATE TABLE IF NOT EXISTS geo.field_boundary (
    field_id UUID PRIMARY KEY REFERENCES core.field(id),
    geom GEOMETRY(MULTIPOLYGON, 4326) NOT NULL
);

CREATE INDEX IF NOT EXISTS field_boundary_geom_idx ON geo.field_boundary USING GIST (geom);

-- TS (sensor data)
CREATE TABLE IF NOT EXISTS ts.sensor_data (
    time TIMESTAMPTZ NOT NULL,
    field_id UUID NOT NULL,
    sensor_type TEXT,
    value DOUBLE PRECISION,
    metadata JSONB
);

SELECT CASE
    WHEN NOT EXISTS (
        SELECT 1 FROM timescaledb_information.hypertables
        WHERE hypertable_schema = 'ts'
        AND hypertable_name = 'sensor_data'
    )
    THEN create_hypertable(
        'ts.sensor_data',
        'time',
        chunk_time_interval => INTERVAL '6 hours'
    )::text
    ELSE 'already exists'
END;

CREATE INDEX IF NOT EXISTS sensor_data_field_time_idx ON ts.sensor_data (field_id, time DESC);

-- Compressão
ALTER TABLE ts.sensor_data SET (
    timescaledb.compress,
    timescaledb.compress_segmentby = 'field_id',
    timescaledb.compress_orderby = 'time DESC'
);

SELECT add_compression_policy('ts.sensor_data', INTERVAL '3 days', if_not_exists => true);

-- Continuous aggregate (WITH NO DATA evita populate imediato no bootstrap)
CREATE MATERIALIZED VIEW IF NOT EXISTS analytics.sensor_hourly
WITH (timescaledb.continuous) AS
SELECT
    field_id,
    time_bucket('1 hour', time) AS bucket,
    AVG(value) avg,
    MAX(value) max,
    MIN(value) min
FROM ts.sensor_data
GROUP BY field_id, bucket
WITH NO DATA;

SELECT add_continuous_aggregate_policy(
    'analytics.sensor_hourly',
    start_offset => INTERVAL '3 days',
    end_offset   => INTERVAL '10 minutes',
    schedule_interval => INTERVAL '5 minutes',
    if_not_exists => true
);
