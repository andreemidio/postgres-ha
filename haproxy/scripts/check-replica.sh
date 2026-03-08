#!/bin/sh
# HAProxy external-check script for PostgreSQL replica detection
# Returns 0 if node is replica (pg_is_in_recovery = true), 1 otherwise
#
# HAProxy passes: $1=NOT_USED $2=NOT_USED $3=addr $4=port
# We connect to the resolved IP ($3) on the check port ($4)
#
# Note: HAProxy external-check runs in restricted environment without container env vars
# The entrypoint writes credentials to /tmp/pg_env.sh which we source here

# Ensure PATH is set (external-check has minimal environment)
export PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin

# Source PostgreSQL credentials (written by entrypoint)
[ -f /tmp/pg_env.sh ] && . /tmp/pg_env.sh

HOST="$3"
PGPORT="$4"
PGUSER="${PGUSER:-postgres}"

RESULT=$(PGCONNECT_TIMEOUT=5 psql -h "$HOST" -p "$PGPORT" -U "$PGUSER" -d postgres -tAc "SELECT pg_is_in_recovery()" 2>/dev/null)

if [ "$RESULT" = "t" ]; then
    exit 0  # Replica
else
    exit 1  # Primary or unreachable
fi
