-- Extensões requeridas — ordem obrigatória (timescaledb antes de postgis)

-- Time-series (deve ser o primeiro)
CREATE EXTENSION IF NOT EXISTS timescaledb CASCADE;

-- Geo
CREATE EXTENSION IF NOT EXISTS postgis CASCADE;
CREATE EXTENSION IF NOT EXISTS postgis_raster;

-- Time-series toolkit (depende de timescaledb já instalado)
CREATE EXTENSION IF NOT EXISTS timescaledb_toolkit;

-- Vetorial
CREATE EXTENSION IF NOT EXISTS pgvector;

-- Observabilidade
CREATE EXTENSION IF NOT EXISTS pg_stat_statements;

-- Utils
CREATE EXTENSION IF NOT EXISTS pgcrypto;
CREATE EXTENSION IF NOT EXISTS "uuid-ossp";

-- Busca
CREATE EXTENSION IF NOT EXISTS pg_trgm;

-- Índices avançados
CREATE EXTENSION IF NOT EXISTS btree_gin;
CREATE EXTENSION IF NOT EXISTS btree_gist;
