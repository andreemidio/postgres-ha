#!/bin/sh
# HAProxy external-check script for PostgreSQL replica detection
# Returns 0 if node is replica (pg_is_in_recovery = true), 1 otherwise
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

if [ "$RESULT" = "t" ]; then
    exit 0  # Replica
else
    exit 1  # Primary or unreachable
fi
