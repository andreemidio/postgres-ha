# Documento de Requisitos: TimescaleDB + PostGIS Extensions

## Introdução

Este documento define os requisitos formais para integração das extensões TimescaleDB 2.x e PostGIS 3.x ao container `postgres-patroni`, que executa PostgreSQL com Patroni HA. A integração abrange: instalação dos pacotes no Dockerfile de produção, configuração de `shared_preload_libraries` via Patroni DCS, execução de DDL de extensões/schemas/hypertables no post-bootstrap do nó primário, e consistência do cluster HA.

---

## Glossário

- **Dockerfile**: Arquivo de definição da imagem Docker de produção (`postgres-patroni/Dockerfile`).
- **Post_Bootstrap**: Binário Rust executado pelo Patroni após o bootstrap do nó primário (`post-bootstrap`).
- **Patroni_Config_Generator**: Função `generate_patroni_config()` em `postgres-patroni/src/patroni/yaml.rs` que gera o YAML de configuração do Patroni.
- **DCS**: Distributed Configuration Store — etcd usado pelo Patroni para propagar configurações a todos os nós do cluster.
- **Hypertable**: Tabela particionada por tempo gerenciada pelo TimescaleDB.
- **Continuous_Aggregate**: View materializada incremental do TimescaleDB (`analytics.sensor_hourly`).
- **Cluster**: Conjunto de nós PostgreSQL gerenciados pelo Patroni (1 primário + N réplicas).
- **Primary**: Nó primário do cluster Patroni, responsável por escritas e execução do post-bootstrap.
- **Replica**: Nó réplica do cluster Patroni, recebe dados via WAL streaming.
- **shared_preload_libraries**: Parâmetro PostgreSQL que define bibliotecas carregadas no startup do processo.

---

## Requisitos

### Requisito 1: Instalação de Pacotes no Dockerfile de Produção

**User Story:** Como operador de infraestrutura, quero que a imagem Docker de produção inclua TimescaleDB, PostGIS e extensões relacionadas, para que o cluster Patroni possa utilizar séries temporais e dados geoespaciais.

#### Critérios de Aceitação

1. THE Dockerfile SHALL install the `timescaledb-2-postgresql-${POSTGRES_VERSION}` package via apt from the TimescaleDB packagecloud repository in the `ha` build stage.
2. THE Dockerfile SHALL install `postgresql-${POSTGRES_VERSION}-postgis-3` and `postgresql-${POSTGRES_VERSION}-postgis-3-scripts` packages via apt in the `ha` build stage.
3. THE Dockerfile SHALL install `timescaledb-toolkit-postgresql-${POSTGRES_VERSION}` via apt in the `ha` build stage.
4. THE Dockerfile SHALL maintain the existing `postgresql-${POSTGRES_VERSION}-pgvector` package installation.
5. THE Dockerfile SHALL include an `ARG TIMESCALEDB_VERSION=2` build argument to allow future version parametrization.
6. WHEN the TimescaleDB repository is added, THE Dockerfile SHALL authenticate it via GPG key from packagecloud.io/timescale.

---

### Requisito 2: Configuração de shared_preload_libraries via Patroni DCS

**User Story:** Como administrador do cluster, quero que `timescaledb` seja configurado em `shared_preload_libraries` via DCS do Patroni, para que todos os nós do cluster (primário e réplicas) carreguem a extensão automaticamente no startup do PostgreSQL.

#### Critérios de Aceitação

1. WHEN `generate_patroni_config()` is called, THE Patroni_Config_Generator SHALL produce a YAML configuration where `shared_preload_libraries` begins with `"timescaledb"`.
2. WHEN `generate_patroni_config()` is called, THE Patroni_Config_Generator SHALL include `"timescaledb"` exactly once in `shared_preload_libraries`.
3. WHEN `generate_patroni_config()` is called, THE Patroni_Config_Generator SHALL include `pg_stat_statements` in `shared_preload_libraries` after `timescaledb`.
4. WHEN `generate_patroni_config()` is called, THE Patroni_Config_Generator SHALL include `timescaledb.max_background_workers: 16` in the DCS postgresql parameters.
5. WHEN `generate_patroni_config()` is called, THE Patroni_Config_Generator SHALL include `max_worker_processes: 32` in the DCS postgresql parameters.

---

### Requisito 3: Criação de Extensões no Post-Bootstrap

**User Story:** Como desenvolvedor, quero que todas as extensões PostgreSQL necessárias sejam criadas automaticamente no banco de dados da aplicação durante o bootstrap do nó primário, para que a aplicação possa utilizar funcionalidades de séries temporais, geoespaciais e vetoriais sem configuração manual.

#### Critérios de Aceitação

1. WHEN post-bootstrap runs on the Primary node, THE Post_Bootstrap SHALL execute `CREATE EXTENSION IF NOT EXISTS timescaledb CASCADE` as the first extension creation statement.
2. WHEN post-bootstrap runs on the Primary node, THE Post_Bootstrap SHALL execute `CREATE EXTENSION IF NOT EXISTS postgis CASCADE` after timescaledb.
3. WHEN post-bootstrap runs on the Primary node, THE Post_Bootstrap SHALL execute `CREATE EXTENSION IF NOT EXISTS postgis_raster` after postgis.
4. WHEN post-bootstrap runs on the Primary node, THE Post_Bootstrap SHALL execute `CREATE EXTENSION IF NOT EXISTS timescaledb_toolkit` after timescaledb.
5. WHEN post-bootstrap runs on the Primary node, THE Post_Bootstrap SHALL create all 11 required extensions: `timescaledb`, `postgis`, `postgis_raster`, `timescaledb_toolkit`, `pgvector`, `pg_stat_statements`, `pgcrypto`, `uuid-ossp`, `pg_trgm`, `btree_gin`, `btree_gist`.
6. WHEN `run_extensions()` is called N times on the same database, THE Post_Bootstrap SHALL produce the same final extension state as calling it once, without returning errors on subsequent executions.

---

### Requisito 4: Criação de Schemas, Tabelas e Hypertables

**User Story:** Como desenvolvedor, quero que os schemas, tabelas e hypertables sejam criados automaticamente no post-bootstrap, para que a estrutura de dados esteja pronta para uso imediato após o bootstrap do cluster.

#### Critérios de Aceitação

1. WHEN `run_schemas()` is called, THE Post_Bootstrap SHALL create schemas `core`, `geo`, `ts`, and `analytics` using `CREATE SCHEMA IF NOT EXISTS`.
2. WHEN `run_hypertables()` is called, THE Post_Bootstrap SHALL create `ts.sensor_data` as a TimescaleDB hypertable partitioned by the `time` column with `chunk_time_interval` of 6 hours.
3. WHEN `run_hypertables()` is called, THE Post_Bootstrap SHALL configure compression on `ts.sensor_data` with `compress_segmentby = 'field_id'` and a compression policy for data older than 3 days using `add_compression_policy(..., if_not_exists => true)`.
4. WHEN `run_hypertables()` is called, THE Post_Bootstrap SHALL create the `analytics.sensor_hourly` continuous aggregate with `time_bucket('1 hour', time)` and a refresh policy with `start_offset = 3 days`, `end_offset = 10 minutes`, and `schedule_interval = 5 minutes`.
5. WHEN `run_hypertables()` is called, THE Post_Bootstrap SHALL create a GIST index on `geo.field_boundary(geom)`.
6. WHEN `run_hypertables()` is called, THE Post_Bootstrap SHALL create an index on `ts.sensor_data(field_id, time DESC)`.
7. WHEN `run_hypertables()` is called on a database where hypertables already exist, THE Post_Bootstrap SHALL complete without error (idempotent execution).

---

### Requisito 5: Integração no Binário post-bootstrap e Tratamento de Erros

**User Story:** Como operador do cluster, quero que o binário `post-bootstrap` execute todas as etapas de inicialização em ordem e trate falhas adequadamente, para que o Patroni possa detectar e recuperar de falhas no bootstrap.

#### Critérios de Aceitação

1. WHEN post-bootstrap runs, THE Post_Bootstrap SHALL call `run_extensions()`, then `run_schemas()`, then `run_hypertables()` in that order after the application database is created.
2. IF `run_extensions()` returns an error, THEN THE Post_Bootstrap SHALL emit a `TelemetryEvent::BootstrapFailed` event and exit with code 1.
3. IF `run_schemas()` returns an error, THEN THE Post_Bootstrap SHALL emit a `TelemetryEvent::BootstrapFailed` event and exit with code 1.
4. IF `run_hypertables()` returns an error, THEN THE Post_Bootstrap SHALL emit a `TelemetryEvent::BootstrapFailed` event and exit with code 1.
5. WHEN post-bootstrap exits with code 1, THE Patroni SHALL mark the bootstrap as failed and retry the process.

---

### Requisito 6: Consistência do Cluster HA

**User Story:** Como operador do cluster, quero que as réplicas recebam automaticamente todas as extensões e dados via WAL streaming, para que o cluster permaneça consistente sem intervenção manual nas réplicas.

#### Critérios de Aceitação

1. WHEN a Replica node starts, THE PostgreSQL_Replica SHALL receive all extension catalog entries via WAL streaming from the Primary without executing post-bootstrap independently.
2. WHILE the cluster is in a stable state, THE Cluster SHALL ensure that every Replica node has the same `pg_extension` catalog as the Primary node.
3. WHILE the cluster is in a stable state, THE Cluster SHALL ensure that `timescaledb` is present in `shared_preload_libraries` on every node (Primary and Replicas) via DCS propagation.
4. WHEN a failover occurs, THE Patroni SHALL promote a Replica that already has `timescaledb` in `shared_preload_libraries` without requiring manual reconfiguration.
