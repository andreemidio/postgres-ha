#!/bin/sh
# HAProxy external-check script for PostgreSQL replica detection
# Returns 0 if node is replica (pg_is_in_recovery = true), 1 otherwise
#
# HAProxy passes: $1=addr $2=port $3=server_name
# We connect to the resolved IP ($1) on PGPORT (default 5432)

HOST="$1"
PGPORT="${PGPORT:-5432}"
PGUSER="${PGUSER:-postgres}"

# Log to stderr for debugging (HAProxy captures this)
echo "check-replica: host=$HOST port=$PGPORT user=$PGUSER" >&2

RESULT=$(PGCONNECT_TIMEOUT=5 psql -h "$HOST" -p "$PGPORT" -U "$PGUSER" -d postgres -tAc "SELECT pg_is_in_recovery()" 2>&1)
EXIT_CODE=$?

echo "check-replica: result='$RESULT' exit=$EXIT_CODE" >&2

if [ "$EXIT_CODE" -ne 0 ]; then
    exit 1  # psql failed
fi

if [ "$RESULT" = "t" ]; then
    exit 0  # Replica
else
    exit 1  # Primary
fi
