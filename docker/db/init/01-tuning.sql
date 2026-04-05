-- ---------------------------------------------------------------------------
-- Performance tuning for TimescaleDB + PostGIS workloads
--
-- WARNING: shared_buffers and effective_cache_size are intentionally NOT set
-- here because they depend on available RAM. Configure them via environment
-- variable POSTGRES_EXTRA_CONF or a mounted postgresql.conf instead.
--
-- Recommended values (adjust to your hardware):
--   shared_buffers        = 25% of RAM  (e.g. '4GB' for 16GB RAM)
--   effective_cache_size  = 75% of RAM  (e.g. '12GB' for 16GB RAM)
-- ---------------------------------------------------------------------------

-- queries complexas (geo + agregações temporais)
ALTER SYSTEM SET work_mem = '64MB';

-- paralelismo — max_worker_processes deve ser >= timescaledb.max_background_workers + max_parallel_workers
ALTER SYSTEM SET max_worker_processes = 32;
ALTER SYSTEM SET max_parallel_workers = 16;
ALTER SYSTEM SET max_parallel_workers_per_gather = 8;

-- IO pesado (GIS + scans grandes) — ajustar para HDD: effective_io_concurrency = 2
ALTER SYSTEM SET effective_io_concurrency = 256;
ALTER SYSTEM SET random_page_cost = 1.1;

-- WAL tuning (ingestão alta)
ALTER SYSTEM SET wal_buffers = '32MB';
ALTER SYSTEM SET checkpoint_completion_target = 0.9;

-- autovacuum agressivo (ESSENCIAL para hypertables com muitos chunks)
ALTER SYSTEM SET autovacuum_max_workers = 8;
ALTER SYSTEM SET autovacuum_naptime = '10s';

-- TimescaleDB background workers (compressão + continuous aggregates)
ALTER SYSTEM SET timescaledb.max_background_workers = 16;

SELECT pg_reload_conf();
