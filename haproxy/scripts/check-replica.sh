#!/bin/sh
# HAProxy external-check script for PostgreSQL replica detection
# Returns 0 if node is replica (pg_is_in_recovery = true), 1 otherwise
#
# HAProxy passes: $1=addr $2=port $3=server_name
# We use PGPORT from env (5432) not $2 (which is the check port)

HOST="$1"
PGPORT="${PGPORT:-5432}"

RESULT=$(PGCONNECT_TIMEOUT=5 psql -h "$HOST" -p "$PGPORT" -U "$PGUSER" -d postgres -tAc "SELECT pg_is_in_recovery()" 2>/dev/null)

if [ "$RESULT" = "t" ]; then
    exit 0  # Replica
else
    exit 1  # Primary or unreachable
fi
