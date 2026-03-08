-- PostgreSQL health check for HAProxy
-- Queries pg_is_in_recovery() to determine primary vs replica
-- Uses trust authentication (no password)

local PGUSER = os.getenv("PGUSER") or "postgres"
local PGPORT = tonumber(os.getenv("PGPORT") or "5432")

local function pack_int32(n)
    return string.char(
        bit.band(bit.rshift(n, 24), 0xFF),
        bit.band(bit.rshift(n, 16), 0xFF),
        bit.band(bit.rshift(n, 8), 0xFF),
        bit.band(n, 0xFF)
    )
end

local function unpack_int32(data, offset)
    offset = offset or 1
    local b1, b2, b3, b4 = string.byte(data, offset, offset + 3)
    return bit.bor(
        bit.lshift(b1, 24),
        bit.lshift(b2, 16),
        bit.lshift(b3, 8),
        b4
    )
end

-- Build PostgreSQL startup message
local function build_startup_message(user, database)
    local params = "user\0" .. user .. "\0database\0" .. database .. "\0\0"
    local protocol_version = pack_int32(196608) -- 3.0
    local length = pack_int32(4 + 4 + #params)
    return length .. protocol_version .. params
end

-- Build PostgreSQL simple query message
local function build_query_message(query)
    local msg = query .. "\0"
    local length = pack_int32(4 + #msg)
    return "Q" .. length .. msg
end

-- Read until we get ReadyForQuery ('Z')
local function read_until_ready(tcp)
    while true do
        local msg_type = tcp:receive(1)
        if not msg_type then return false end

        local len_data = tcp:receive(4)
        if not len_data then return false end

        local len = unpack_int32(len_data) - 4
        local data = ""
        if len > 0 then
            data = tcp:receive(len)
            if not data then return false end
        end

        if msg_type == "Z" then
            return true
        elseif msg_type == "E" then
            return false
        end
    end
end

-- Query pg_is_in_recovery() and return result
local function query_recovery_status(tcp)
    local query_msg = build_query_message("SELECT pg_is_in_recovery()")
    tcp:send(query_msg)

    local is_recovery = nil

    while true do
        local msg_type = tcp:receive(1)
        if not msg_type then return nil end

        local len_data = tcp:receive(4)
        if not len_data then return nil end

        local len = unpack_int32(len_data) - 4
        local data = ""
        if len > 0 then
            data = tcp:receive(len)
            if not data then return nil end
        end

        if msg_type == "D" then
            -- DataRow - field_count(2) + field_len(4) + value
            if #data >= 7 then
                is_recovery = (string.sub(data, 7, 7) == "t")
            end
        elseif msg_type == "Z" then
            return is_recovery
        elseif msg_type == "E" then
            return nil
        end
    end
end

-- Check PostgreSQL and return is_in_recovery status
local function check_postgres(host, port)
    local tcp = core.tcp()
    tcp:settimeout(5)

    local ok, err = tcp:connect(host, port)
    if not ok then
        return nil, "connect failed: " .. (err or "unknown")
    end

    local startup = build_startup_message(PGUSER, "postgres")
    tcp:send(startup)

    if not read_until_ready(tcp) then
        tcp:close()
        return nil, "startup failed"
    end

    local is_recovery = query_recovery_status(tcp)
    tcp:close()

    return is_recovery
end

-- HTTP service: GET /primary/<host> or /replica/<host>
core.register_service("pgsql_health", "http", function(applet)
    local path = applet.path or ""

    -- Parse path: /primary/<host> or /replica/<host>
    local check_type, host = path:match("^/(%w+)/(.+)$")

    if not check_type or not host then
        applet:set_status(400)
        applet:start_response()
        applet:send("Invalid path. Use /primary/<host> or /replica/<host>")
        return
    end

    local is_recovery, err = check_postgres(host, PGPORT)

    if is_recovery == nil then
        applet:set_status(503)
        applet:start_response()
        applet:send("ERROR: " .. (err or "unknown"))
        return
    end

    local is_healthy = false
    if check_type == "primary" then
        is_healthy = (is_recovery == false)
    elseif check_type == "replica" then
        is_healthy = (is_recovery == true)
    end

    if is_healthy then
        applet:set_status(200)
        applet:start_response()
        applet:send("OK")
    else
        applet:set_status(503)
        applet:start_response()
        applet:send("NOT " .. check_type:upper())
    end
end)

core.Info("PostgreSQL Lua health check loaded (user=" .. PGUSER .. ", port=" .. PGPORT .. ")")
