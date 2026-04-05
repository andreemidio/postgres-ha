ARG POSTGRES_VERSION=16

FROM postgres:${POSTGRES_VERSION}-bookworm

ARG POSTGRES_VERSION=16
ENV DEBIAN_FRONTEND=noninteractive

# ---------------------------------------------------------------------------
# Base + locales
# ---------------------------------------------------------------------------
RUN apt-get update && apt-get install -y --no-install-recommends \
    gnupg curl ca-certificates lsb-release locales \
    && sed -i '/en_US.UTF-8/s/^# //g' /etc/locale.gen \
    && locale-gen \
    && rm -rf /var/lib/apt/lists/*

ENV LANG=en_US.UTF-8
ENV LC_ALL=en_US.UTF-8

# ---------------------------------------------------------------------------
# TimescaleDB repo
# ---------------------------------------------------------------------------
RUN curl -fsSL https://packagecloud.io/timescale/timescaledb/gpgkey \
    | gpg --dearmor > /usr/share/keyrings/timescaledb.gpg \
    && echo "deb [signed-by=/usr/share/keyrings/timescaledb.gpg] https://packagecloud.io/timescale/timescaledb/debian/ bookworm main" \
    > /etc/apt/sources.list.d/timescaledb.list

# ---------------------------------------------------------------------------
# Instala PostGIS + Timescale + extras
# ---------------------------------------------------------------------------
RUN apt-get update && apt-get install -y --no-install-recommends \
    timescaledb-2-postgresql-${POSTGRES_VERSION} \
    postgresql-${POSTGRES_VERSION}-postgis-3 \
    postgresql-${POSTGRES_VERSION}-postgis-3-scripts \
    postgresql-${POSTGRES_VERSION}-pgvector \
    timescaledb-toolkit-postgresql-${POSTGRES_VERSION} \
    && rm -rf /var/lib/apt/lists/*

# ---------------------------------------------------------------------------
# timescaledb MUST be in shared_preload_libraries before PostgreSQL starts.
# The entrypoint script sources this file before initdb runs.
# ---------------------------------------------------------------------------
RUN echo "shared_preload_libraries = 'timescaledb,pg_stat_statements'" \
    >> /usr/share/postgresql/${POSTGRES_VERSION}/postgresql.conf.sample

# ---------------------------------------------------------------------------
# Init scripts (run by docker-entrypoint in alphabetical order)
# ---------------------------------------------------------------------------
COPY docker/db/init/ /docker-entrypoint-initdb.d/
