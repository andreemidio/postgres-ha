#!/bin/sh
# HAProxy external-check script for PostgreSQL primary detection
# Returns 0 if node is primary (pg_is_in_recovery = false), 1 otherwise
#
# HAProxy passes: $1=addr $2=port $3=server_name
# We connect to the resolved IP ($1) on PGPORT (default 5432)
#
# Note: HAProxy external-check runs in restricted environment without container env vars
# The entrypoint writes credentials to /tmp/pg_env.sh which we source here

# Source PostgreSQL credentials (written by entrypoint)
[ -f /tmp/pg_env.sh ] && . /tmp/pg_env.sh

HOST="$1"
PGPORT="${PGPORT:-5432}"
PGUSER="${PGUSER:-postgres}"

RESULT=$(PGCONNECT_TIMEOUT=5 psql -h "$HOST" -p "$PGPORT" -U "$PGUSER" -d postgres -tAc "SELECT pg_is_in_recovery()" 2>/dev/null)

if [ "$RESULT" = "f" ]; then
    exit 0  # Primary
else
    exit 1  # Replica or unreachable
fi
