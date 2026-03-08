//! HAProxy configuration template generation

use crate::config::Config;
use crate::nodes::PostgresNode;

/// Generate server entries for backend configuration
fn generate_server_entries(
    nodes: &[PostgresNode],
    single_node_mode: bool,
    use_pgsql_check: bool,
    check_type: &str,
) -> String {
    nodes
        .iter()
        .map(|node| {
            if single_node_mode {
                format!(
                    "    server {} {}:{} check resolvers railway resolve-prefer ipv4",
                    node.name, node.host, node.pg_port
                )
            } else if use_pgsql_check {
                // Lua check mode: track the health check backend for this server
                format!(
                    "    server {} {}:{} track health_{}_{}/chk resolvers railway resolve-prefer ipv4",
                    node.name, node.host, node.pg_port, check_type, node.name
                )
            } else {
                // HTTP check mode: check Patroni API port
                format!(
                    "    server {} {}:{} check port {} resolvers railway resolve-prefer ipv4",
                    node.name, node.host, node.pg_port, node.health_port
                )
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Generate health check backends for Lua-based checks
fn generate_health_backends(nodes: &[PostgresNode], check_type: &str, config: &Config) -> String {
    nodes
        .iter()
        .map(|node| {
            format!(
                r#"backend health_{check_type}_{name}
    mode http
    option httpchk
    http-check send meth GET uri /{check_type}/{host} ver HTTP/1.1 hdr Host localhost
    http-check expect status 200
    default-server inter {interval} fall 3 rise 2 fastinter {fastinter} downinter {downinter}
    server chk 127.0.0.1:8009 check"#,
                check_type = check_type,
                name = node.name,
                host = node.host,
                interval = config.check_interval,
                fastinter = config.check_fastinter,
                downinter = config.check_downinter,
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// Generate primary backend configuration
fn generate_primary_backend(
    config: &Config,
    server_entries: &str,
    single_node_mode: bool,
) -> String {
    if single_node_mode || config.use_pgsql_check {
        // Single node or Lua check mode: no backend-level health check
        format!(
            r#"backend postgresql_primary_backend
    default-server on-marked-down shutdown-sessions
{}"#,
            server_entries
        )
    } else {
        // HTTP check mode: use Patroni REST API
        format!(
            r#"backend postgresql_primary_backend
    option httpchk
    http-check connect linger
    http-check send meth GET uri /primary ver HTTP/1.1 hdr Host localhost
    http-check expect status 200
    default-server inter {} fall 3 rise 2 fastinter {} downinter {} on-marked-down shutdown-sessions
{}"#,
            config.check_interval, config.check_fastinter, config.check_downinter, server_entries
        )
    }
}

/// Generate replica backend configuration
fn generate_replica_backend(
    config: &Config,
    server_entries: &str,
    single_node_mode: bool,
) -> String {
    if single_node_mode || config.use_pgsql_check {
        // Single node or Lua check mode: no backend-level health check
        format!(
            r#"backend postgresql_replicas_backend
    balance leastconn
    default-server on-marked-down shutdown-sessions
{}"#,
            server_entries
        )
    } else {
        // HTTP check mode: use Patroni REST API
        format!(
            r#"backend postgresql_replicas_backend
    balance leastconn
    option httpchk
    http-check connect linger
    http-check send meth GET uri /replica ver HTTP/1.1 hdr Host localhost
    http-check expect status 200
    default-server inter {} fall 3 rise 2 fastinter {} downinter {} on-marked-down shutdown-sessions
{}"#,
            config.check_interval, config.check_fastinter, config.check_downinter, server_entries
        )
    }
}

/// Generate complete HAProxy configuration
pub fn generate_config(config: &Config, nodes: &[PostgresNode]) -> String {
    let single_node_mode = nodes.len() == 1;
    let primary_servers = generate_server_entries(nodes, single_node_mode, config.use_pgsql_check, "primary");
    let replica_servers = generate_server_entries(nodes, single_node_mode, config.use_pgsql_check, "replica");
    let primary_backend = generate_primary_backend(config, &primary_servers, single_node_mode);
    let replica_backend = generate_replica_backend(config, &replica_servers, single_node_mode);

    // Lua health check section
    let lua_section = if config.use_pgsql_check && !single_node_mode {
        let primary_health = generate_health_backends(nodes, "primary", config);
        let replica_health = generate_health_backends(nodes, "replica", config);
        format!(
            r#"
# Lua health check service
frontend lua_health
    bind 127.0.0.1:8009
    mode http
    http-request use-service lua.pgsql_health

# Health check backends for primary detection
{}

# Health check backends for replica detection
{}
"#,
            primary_health, replica_health
        )
    } else {
        String::new()
    };

    // Global Lua loading
    let lua_load = if config.use_pgsql_check && !single_node_mode {
        "\n    lua-load /usr/local/share/haproxy/pgsql_check.lua"
    } else {
        ""
    };

    format!(
        r#"global
    maxconn {}
    log stdout format raw local0{}

defaults
    log global
    mode tcp
    option tcpka
    option clitcpka
    option srvtcpka
    option redispatch
    retries 3
    timeout connect {}
    timeout client {}
    timeout server {}
    timeout check {}

resolvers railway
    parse-resolv-conf
    resolve_retries 3
    timeout resolve 1s
    timeout retry   1s
    hold other      10s
    hold refused    10s
    hold nx         10s
    hold timeout    10s
    hold valid      10s
    hold obsolete   10s

# Stats page for monitoring
listen stats
    bind :::8404 v4v6
    mode http
    stats enable
    stats uri /stats
    stats refresh 10s
{}
# Primary PostgreSQL (read-write)
frontend postgresql_primary
    bind :::5432 v4v6
    default_backend postgresql_primary_backend

{}

# Replica PostgreSQL (read-only)
frontend postgresql_replicas
    bind :::5433 v4v6
    default_backend postgresql_replicas_backend

{}
"#,
        config.max_conn,
        lua_load,
        config.timeout_connect,
        config.timeout_client,
        config.timeout_server,
        config.timeout_check,
        lua_section,
        primary_backend,
        replica_backend
    )
}
