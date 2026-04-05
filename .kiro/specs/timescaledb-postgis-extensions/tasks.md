# Plano de Implementação: TimescaleDB + PostGIS Extensions

## Visão Geral

Integrar TimescaleDB 2.x e PostGIS 3.x ao container `postgres-patroni` (produção com Patroni HA), configurando `shared_preload_libraries` via DCS e executando DDL de extensões, schemas e hypertables no post-bootstrap do nó primário.

## Tasks

- [x] 1. Atualizar `postgres-patroni/Dockerfile` com repositório e pacotes das extensões
  - Adicionar repositório packagecloud do TimescaleDB (GPG key + sources.list) no stage `ha`
  - Instalar `timescaledb-2-postgresql-${POSTGRES_VERSION}`, `postgresql-${POSTGRES_VERSION}-postgis-3`, `postgresql-${POSTGRES_VERSION}-postgis-3-scripts`, `timescaledb-toolkit-postgresql-${POSTGRES_VERSION}` via apt
  - Manter `postgresql-${POSTGRES_VERSION}-pgvector` já presente
  - Adicionar `ARG TIMESCALEDB_VERSION=2` para parametrizar versão futura
  - _Requirements: 1.1, 1.2, 1.3_

- [x] 2. Atualizar `shared_preload_libraries` no Patroni YAML e adicionar parâmetros de tuning
  - [x] 2.1 Modificar `generate_patroni_config()` em `postgres-patroni/src/patroni/yaml.rs`
    - Alterar `shared_preload_libraries: pg_stat_statements` para `shared_preload_libraries: "timescaledb,pg_stat_statements"`
    - Adicionar `timescaledb.max_background_workers: 16` nos parâmetros DCS
    - Adicionar `max_worker_processes: 32` nos parâmetros DCS
    - _Requirements: 2.1, 2.2_

  - [ ]* 2.2 Escrever testes de unidade para `generate_patroni_config()`
    - Verificar que o YAML gerado contém `timescaledb` como primeiro item de `shared_preload_libraries`
    - Verificar que `timescaledb` aparece exatamente uma vez
    - Verificar que `timescaledb.max_background_workers` está presente
    - _Requirements: 2.1, 2.2_

  - [ ]* 2.3 Escrever property test para `generate_patroni_config()`
    - **Property 2: Ordem de preload** — para qualquer `Config` válida, `generate_patroni_config(config)` sempre produz YAML com `timescaledb` no início de `shared_preload_libraries`
    - Usar `proptest` para gerar configs com valores arbitrários nos campos de string
    - **Validates: Requirements 2.1**

- [x] 3. Implementar função `run_extensions()` no post-bootstrap
  - [x] 3.1 Criar função `run_extensions(creds: &Credentials) -> Result<()>` em `postgres-patroni/src/bootstrap/sql.rs` ou novo módulo `extensions.rs`
    - Executar `CREATE EXTENSION IF NOT EXISTS timescaledb CASCADE` primeiro
    - Executar `CREATE EXTENSION IF NOT EXISTS postgis CASCADE`
    - Executar `CREATE EXTENSION IF NOT EXISTS postgis_raster`
    - Executar `CREATE EXTENSION IF NOT EXISTS timescaledb_toolkit`
    - Executar para cada extensão restante: `pgvector`, `pg_stat_statements`, `pgcrypto`, `uuid-ossp`, `pg_trgm`, `btree_gin`, `btree_gist`
    - Todas as chamadas via `run_psql_in_db(&creds.superuser, &creds.app_db, sql)`
    - _Requirements: 3.1, 3.2, 3.3_

  - [ ]* 3.2 Escrever property test para idempotência de `run_extensions()`
    - **Property 1: Idempotência** — executar `run_extensions()` N vezes produz o mesmo estado que executar 1 vez (sem erros em re-execuções)
    - Testar com mock de `run_psql_in_db` que conta chamadas e verifica que `IF NOT EXISTS` está presente em todos os SQLs
    - **Validates: Requirements 3.4**

- [x] 4. Implementar funções `run_schemas()` e `run_hypertables()` no post-bootstrap
  - [x] 4.1 Criar função `run_schemas(creds: &Credentials) -> Result<()>`
    - Criar schemas `core`, `geo`, `ts`, `analytics` com `CREATE SCHEMA IF NOT EXISTS`
    - _Requirements: 4.1_

  - [x] 4.2 Criar função `run_hypertables(creds: &Credentials) -> Result<()>`
    - Criar `core.field` e `geo.field_boundary` com `CREATE TABLE IF NOT EXISTS`
    - Criar índice GIST em `geo.field_boundary(geom)`
    - Criar `ts.sensor_data` com `CREATE TABLE IF NOT EXISTS`
    - Criar hypertable via verificação prévia em `timescaledb_information.hypertables` (idempotente)
    - Criar índice em `ts.sensor_data(field_id, time DESC)`
    - Configurar compressão com `ALTER TABLE ... SET (timescaledb.compress, ...)`
    - Adicionar política de compressão com `add_compression_policy(..., if_not_exists => true)`
    - Criar continuous aggregate `analytics.sensor_hourly` com `CREATE MATERIALIZED VIEW IF NOT EXISTS`
    - Adicionar política de continuous aggregate com `add_continuous_aggregate_policy(...)`
    - _Requirements: 4.2, 4.3, 4.4_

  - [ ]* 4.3 Escrever testes de unidade para os SQLs gerados
    - Verificar que todos os DDLs contêm `IF NOT EXISTS` ou equivalente idempotente
    - Verificar que `create_hypertable` usa verificação prévia via `timescaledb_information.hypertables`
    - _Requirements: 4.2, 4.3_

- [x] 5. Checkpoint — Garantir que todos os testes passam
  - Garantir que todos os testes passam, perguntar ao usuário se houver dúvidas.

- [x] 6. Integrar as novas funções no binário `post-bootstrap`
  - [x] 6.1 Modificar `postgres-patroni/src/bin/post_bootstrap.rs` para chamar as novas funções
    - Após criação do banco de dados da aplicação, chamar `run_extensions(&creds)`
    - Chamar `run_schemas(&creds)` após extensões
    - Chamar `run_hypertables(&creds)` após schemas
    - Remover a chamada avulsa de `pg_stat_statements` já existente (coberta por `run_extensions`)
    - Propagar erros com `telemetry.send(TelemetryEvent::BootstrapFailed {...})` e `std::process::exit(1)`
    - _Requirements: 3.1, 4.1, 4.2, 5.1_

  - [x] 6.2 Exportar as novas funções em `postgres-patroni/src/bootstrap/mod.rs`
    - Adicionar `pub use extensions::{run_extensions, run_schemas, run_hypertables}` (ou equivalente conforme módulo criado)
    - _Requirements: 3.1_

- [x] 7. Atualizar `docker/db/Dockerfile` (modo standalone) para consistência
  - Adicionar `timescaledb-toolkit-postgresql-16` ao apt install (já tem os demais pacotes)
  - Verificar que `docker/db/init/00-init.sql` usa `CREATE SCHEMA IF NOT EXISTS` (idempotência)
  - _Requirements: 1.1, 1.2_

- [x] 8. Checkpoint final — Garantir que todos os testes passam
  - Garantir que todos os testes passam, perguntar ao usuário se houver dúvidas.

## Notas

- Tasks marcadas com `*` são opcionais e podem ser puladas para MVP mais rápido
- `timescaledb` DEVE ser o primeiro em `shared_preload_libraries` (requisito do TimescaleDB)
- Todos os DDLs DEVEM usar `IF NOT EXISTS` ou verificação prévia para garantir idempotência
- O post-bootstrap roda SEM variáveis de ambiente — credenciais lidas de `/etc/patroni/patroni.yml`
- Réplicas recebem extensões via WAL streaming, não executam post-bootstrap
