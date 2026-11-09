CREATE TABLE package_managers (
    id               TEXT PRIMARY KEY,
    display_name     TEXT NOT NULL,
    detected         INTEGER NOT NULL DEFAULT 0,
    path             TEXT,
    version          TEXT,
    supports_updates INTEGER NOT NULL DEFAULT 0,
    last_error       TEXT,
    last_checked_at  INTEGER
);

-- One row per installation source. Rows from different managers are never merged.
CREATE TABLE installed_packages (
    id                TEXT PRIMARY KEY,
    manager_id        TEXT NOT NULL REFERENCES package_managers(id) ON DELETE CASCADE,
    name              TEXT NOT NULL,
    display_name      TEXT,
    version           TEXT,
    latest_version    TEXT,
    description       TEXT,
    category          TEXT,
    kind              TEXT NOT NULL,
    installation_source TEXT,
    installation_path TEXT,
    architecture      TEXT,
    installed_date    TEXT,
    update_available  INTEGER,            -- NULL = unknown
    update_status     TEXT NOT NULL,
    management_status TEXT NOT NULL,
    homepage          TEXT,
    license           TEXT,
    first_seen_at     INTEGER NOT NULL,
    last_seen_at      INTEGER NOT NULL
);
CREATE INDEX idx_installed_name ON installed_packages(name);
CREATE INDEX idx_installed_manager ON installed_packages(manager_id);

CREATE TABLE package_metadata (
    package_id TEXT NOT NULL REFERENCES installed_packages(id) ON DELETE CASCADE,
    key        TEXT NOT NULL,
    value      TEXT NOT NULL,
    PRIMARY KEY (package_id, key)
);

-- `confirmed` = reported by the package manager; 0 = inferred by UniManage.
CREATE TABLE package_dependencies (
    package_id TEXT NOT NULL REFERENCES installed_packages(id) ON DELETE CASCADE,
    depends_on TEXT NOT NULL,
    kind       TEXT NOT NULL DEFAULT 'runtime',
    confirmed  INTEGER NOT NULL DEFAULT 1,
    PRIMARY KEY (package_id, depends_on, kind)
);

CREATE TABLE operations (
    id               INTEGER PRIMARY KEY AUTOINCREMENT,
    started_at       INTEGER NOT NULL,
    finished_at      INTEGER,
    operation_type   TEXT NOT NULL,
    package_name     TEXT NOT NULL,
    manager_id       TEXT NOT NULL,
    previous_version TEXT,
    new_version      TEXT,
    command          TEXT,
    result           TEXT NOT NULL DEFAULT 'pending',
    error            TEXT
);

CREATE TABLE operation_logs (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    operation_id INTEGER NOT NULL REFERENCES operations(id) ON DELETE CASCADE,
    ts           INTEGER NOT NULL,
    stream       TEXT NOT NULL,           -- stdout | stderr | info
    line         TEXT NOT NULL
);
CREATE INDEX idx_oplogs_op ON operation_logs(operation_id);

CREATE TABLE environment_manifests (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    name       TEXT NOT NULL,
    platform   TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    content    TEXT NOT NULL              -- JSON manifest
);

CREATE TABLE application_settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE TABLE scan_history (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    started_at    INTEGER NOT NULL,
    finished_at   INTEGER,
    status        TEXT NOT NULL,          -- running | completed | partial | failed
    package_count INTEGER NOT NULL DEFAULT 0,
    update_count  INTEGER NOT NULL DEFAULT 0,
    details       TEXT                    -- JSON: per-manager outcome
);
