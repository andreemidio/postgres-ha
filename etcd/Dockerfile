ARG ETCD_VERSION=3.6.6

# ============================================================
# Rust builder stage: Compile etcd-entrypoint binary (musl)
# Build context must be repo root (.) to access workspace
# ============================================================
FROM rust:1.83-alpine AS rust-builder

RUN apk add --no-cache musl-dev

WORKDIR /build

# Copy workspace root files for dependency resolution
COPY Cargo.toml Cargo.lock ./

# Copy common crate (shared dependency)
COPY common ./common

# Copy etcd crate
COPY etcd ./etcd

# Build statically linked release binary
RUN cargo build --release -p etcd-entrypoint

# ============================================================
# Final stage: etcd with Rust entrypoint
# ============================================================
FROM quay.io/coreos/etcd:v${ETCD_VERSION}

# Copy Rust binary (statically linked, no glibc needed)
COPY --from=rust-builder --chmod=755 /build/target/release/etcd-entrypoint /entrypoint

ENV ETCD_DATA_DIR=/var/lib/etcd
# Enable gRPC gateway for Patroni etcd3 client compatibility
ENV ETCD_ENABLE_GRPC_GATEWAY=true
# Retry settings (5 min total with defaults: 60 retries * 5s delay)
ENV ETCD_MAX_RETRIES=60
ENV ETCD_RETRY_DELAY=5
ENV ETCD_STABILIZE_TIME=30

EXPOSE 2379 2380

HEALTHCHECK --interval=10s --timeout=5s --start-period=60s --retries=3 \
  CMD etcdctl endpoint health --endpoints=http://127.0.0.1:2379 || exit 1

ENTRYPOINT ["/entrypoint"]
