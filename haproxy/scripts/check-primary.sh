#!/bin/sh
# HAProxy external-check script for PostgreSQL primary detection
# Returns 0 if node is primary (pg_is_in_recovery = false), 1 otherwise
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

if [ "$RESULT" = "f" ]; then
    exit 0  # Primary
else
    exit 1  # Replica or unreachable
fi
