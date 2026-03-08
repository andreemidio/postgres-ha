#!/bin/sh
# HAProxy external-check script for PostgreSQL primary detection
# Returns 0 if node is primary (pg_is_in_recovery = false), 1 otherwise
#
# HAProxy passes: $1=addr $2=port $3=server_name
# We use PGPORT from env (5432) not $2 (which is the check port)

HOST="$1"
PGPORT="${PGPORT:-5432}"

RESULT=$(PGCONNECT_TIMEOUT=2 psql -h "$HOST" -p "$PGPORT" -U "$PGUSER" -d postgres -tAc "SELECT pg_is_in_recovery()" 2>/dev/null)

if [ "$RESULT" = "f" ]; then
    exit 0  # Primary
else
    exit 1  # Replica or unreachable
fi
