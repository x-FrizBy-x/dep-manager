use crate::error::Result;
use crate::model::*;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::Path;

const MIGRATIONS: &[&str] = &[include_str!("../migrations/001_init.sql")];

pub struct Db {
    pub conn: Connection,
}

/// Outcome of one manager during a scan, persisted into `scan_history.details`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagerOutcome {
    pub id: String,
    pub detected: bool,
    pub ok: bool,
    pub package_count: u32,
    pub update_count: u32,
    pub error: Option<String>,
    pub update_error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanRecord {
    pub id: i64,
    pub started_at: i64,
    pub finished_at: Option<i64>,
    pub status: String,
    pub package_count: u32,
    pub update_count: u32,
    pub details: serde_json::Value,
}


#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationRecord {
    pub id: i64,
    pub started_at: i64,
    pub finished_at: Option<i64>,
    pub operation_type: String,
    pub package_name: String,
    pub manager_id: String,
    pub previous_version: Option<String>,
    pub new_version: Option<String>,
    pub command: Option<String>,
    pub result: String,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationLogLine {
    pub ts: i64,
    pub stream: String,
    pub line: String,
}

const OP_SELECT_ONE: &str = "SELECT id, started_at, finished_at, operation_type, package_name, manager_id, previous_version, new_version, command, result, error FROM operations WHERE id=?";
const OP_SELECT_LIST: &str = "SELECT id, started_at, finished_at, operation_type, package_name, manager_id, previous_version, new_version, command, result, error FROM operations ORDER BY id DESC LIMIT ?";

fn op_row(r: &rusqlite::Row) -> rusqlite::Result<OperationRecord> {
    Ok(OperationRecord {
        id: r.get(0)?,
        started_at: r.get(1)?,
        finished_at: r.get(2)?,
        operation_type: r.get(3)?,
        package_name: r.get(4)?,
        manager_id: r.get(5)?,
        previous_version: r.get(6)?,
        new_version: r.get(7)?,
        command: r.get(8)?,
        result: r.get(9)?,
        error: r.get(10)?,
    })
}

pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

impl Db {
    pub fn open(path: &Path) -> Result<Db> {
        let conn = Connection::open(path)?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> Result<Db> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Db> {
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        let db = Db { conn };
        db.migrate()?;
        Ok(db)
    }

    /// Applies pending migrations, tracked via `PRAGMA user_version`.
    fn migrate(&self) -> Result<()> {
        let current: usize = self.conn.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))? as usize;
        for (i, sql) in MIGRATIONS.iter().enumerate().skip(current) {
            let tx = self.conn.unchecked_transaction()?;
            tx.execute_batch(sql)?;
            tx.execute_batch(&format!("PRAGMA user_version = {}", i + 1))?;
            tx.commit()?;
        }
        Ok(())
    }

    pub fn begin_scan(&self) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO scan_history (started_at, status) VALUES (?, 'running')",
            [now()],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Persists one manager's result. On failure the previous rows are kept
    /// (stale data beats silently empty data) and the error is recorded.
    pub fn save_manager(&self, info: &ManagerInfo, packages: Option<&[Package]>) -> Result<()> {
        let ts = now();
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "INSERT INTO package_managers (id, display_name, detected, path, version, supports_updates, last_error, last_checked_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8)
             ON CONFLICT(id) DO UPDATE SET display_name=?2, detected=?3, path=?4, version=?5,
               supports_updates=?6, last_error=?7, last_checked_at=?8",
            params![info.id, info.display_name, info.detected, info.path, info.version, info.supports_updates, info.last_error, ts],
        )?;
        let replace = packages.is_some() || !info.detected;
        if replace {
            tx.execute("DELETE FROM installed_packages WHERE manager_id = ?", [&info.id])?;
        }
        for p in packages.unwrap_or(&[]) {
            tx.execute(
                "INSERT INTO installed_packages (id, manager_id, name, display_name, version, latest_version, description,
                   category, kind, installation_source, installation_path, architecture, installed_date, update_available,
                   update_status, management_status, homepage, license, first_seen_at, last_seen_at)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?19)
                 ON CONFLICT(id) DO UPDATE SET last_seen_at=?19",
                params![
                    p.id, p.package_manager, p.name, p.display_name, p.version, p.latest_version, p.description,
                    p.category, enum_str(&p.kind), p.installation_source, p.installation_path, p.architecture,
                    p.installed_date, p.update_available, enum_str(&p.update_status), enum_str(&p.management_status),
                    p.homepage, p.license, ts
                ],
            )?;
            for (k, v) in &p.metadata {
                tx.execute("INSERT INTO package_metadata (package_id, key, value) VALUES (?,?,?)", params![p.id, k, v])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn finish_scan(&self, id: i64, status: &str, pkgs: u32, updates: u32, details: &[ManagerOutcome]) -> Result<()> {
        self.conn.execute(
            "UPDATE scan_history SET finished_at=?, status=?, package_count=?, update_count=?, details=? WHERE id=?",
            params![now(), status, pkgs, updates, serde_json::to_string(details)?, id],
        )?;
        Ok(())
    }

    pub fn list_packages(&self) -> Result<Vec<Package>> {
        let mut meta: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
        let mut st = self.conn.prepare("SELECT package_id, key, value FROM package_metadata")?;
        for row in st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?)))? {
            let (id, k, v) = row?;
            meta.entry(id).or_default().insert(k, v);
        }

        let mut st = self.conn.prepare(
            "SELECT id, manager_id, name, display_name, version, latest_version, description, category, kind,
                    installation_source, installation_path, architecture, installed_date, update_available,
                    update_status, management_status, homepage, license, first_seen_at
             FROM installed_packages ORDER BY name COLLATE NOCASE",
        )?;
        let rows = st.query_map([], |r| {
            let id: String = r.get(0)?;
            let mut p = Package::new(&r.get::<_, String>(1)?, id.clone(), r.get::<_, String>(2)?);
            p.display_name = r.get(3)?;
            p.version = r.get(4)?;
            p.latest_version = r.get(5)?;
            p.description = r.get(6)?;
            p.category = r.get(7)?;
            p.kind = parse_enum(&r.get::<_, String>(8)?).unwrap_or(PackageKind::Unknown);
            p.installation_source = r.get(9)?;
            p.installation_path = r.get(10)?;
            p.architecture = r.get(11)?;
            p.installed_date = r.get(12)?;
            p.update_available = r.get(13)?;
            p.update_status = parse_enum(&r.get::<_, String>(14)?).unwrap_or(UpdateStatus::Unknown);
            p.management_status = parse_enum(&r.get::<_, String>(15)?).unwrap_or(ManagementStatus::Unknown);
            p.homepage = r.get(16)?;
            p.license = r.get(17)?;
            Ok(p)
        })?;
        let mut pkgs = Vec::new();
        for p in rows {
            let mut p = p?;
            p.metadata = meta.remove(&p.id).unwrap_or_default();
            pkgs.push(p);
        }
        annotate_overlaps(&mut pkgs);
        Ok(pkgs)
    }

    pub fn list_managers(&self) -> Result<Vec<ManagerInfo>> {
        let mut st = self.conn.prepare(
            "SELECT m.id, m.display_name, m.detected, m.path, m.version, m.supports_updates, m.last_error,
                    (SELECT COUNT(*) FROM installed_packages p WHERE p.manager_id = m.id)
             FROM package_managers m ORDER BY m.display_name COLLATE NOCASE",
        )?;
        let rows = st.query_map([], |r| {
            Ok(ManagerInfo {
                id: r.get(0)?,
                display_name: r.get(1)?,
                detected: r.get(2)?,
                path: r.get(3)?,
                version: r.get(4)?,
                supports_updates: r.get(5)?,
                last_error: r.get(6)?,
                package_count: r.get(7)?,
            })
        })?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    pub fn recent_scans(&self, limit: u32) -> Result<Vec<ScanRecord>> {
        let mut st = self.conn.prepare(
            "SELECT id, started_at, finished_at, status, package_count, update_count, details
             FROM scan_history ORDER BY id DESC LIMIT ?",
        )?;
        let rows = st.query_map([limit], |r| {
            let details: Option<String> = r.get(6)?;
            Ok(ScanRecord {
                id: r.get(0)?,
                started_at: r.get(1)?,
                finished_at: r.get(2)?,
                status: r.get(3)?,
                package_count: r.get(4)?,
                update_count: r.get(5)?,
                details: details.and_then(|d| serde_json::from_str(&d).ok()).unwrap_or(serde_json::Value::Null),
            })
        })?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    /// Marks scans left `running` by a crash as failed.
    pub fn fail_interrupted_scans(&self) -> Result<()> {
        self.conn.execute("UPDATE scan_history SET status='failed', finished_at=? WHERE status='running'", [now()])?;
        Ok(())
    }

    // ---- operations -------------------------------------------------------

    pub fn begin_operation(
        &self,
        kind: &str,
        package: &str,
        manager_id: &str,
        previous_version: Option<&str>,
        command: &str,
    ) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO operations (started_at, operation_type, package_name, manager_id, previous_version, command, result)              VALUES (?,?,?,?,?,?, 'running')",
            params![now(), kind, package, manager_id, previous_version, command],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn append_log(&self, op_id: i64, stream: &str, line: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO operation_logs (operation_id, ts, stream, line) VALUES (?,?,?,?)",
            params![op_id, now(), stream, line],
        )?;
        Ok(())
    }

    /// `result`: success | failed | cancelled | timeout
    pub fn finish_operation(&self, id: i64, result: &str, new_version: Option<&str>, error: Option<&str>) -> Result<()> {
        self.conn.execute(
            "UPDATE operations SET finished_at=?, result=?, new_version=?, error=? WHERE id=?",
            params![now(), result, new_version, error, id],
        )?;
        Ok(())
    }

    pub fn get_operation(&self, id: i64) -> Result<Option<OperationRecord>> {
        Ok(self
            .conn
            .query_row(OP_SELECT_ONE, [id], op_row)
            .optional()?)
    }

    pub fn list_operations(&self, limit: u32) -> Result<Vec<OperationRecord>> {
        let mut st = self.conn.prepare(OP_SELECT_LIST)?;
        let rows = st.query_map([limit], op_row)?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    pub fn operation_logs(&self, op_id: i64) -> Result<Vec<OperationLogLine>> {
        let mut st = self
            .conn
            .prepare("SELECT ts, stream, line FROM operation_logs WHERE operation_id=? ORDER BY id")?;
        let rows = st.query_map([op_id], |r| Ok(OperationLogLine { ts: r.get(0)?, stream: r.get(1)?, line: r.get(2)? }))?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    /// Operations left `running` by a crash are marked `interrupted`.
    pub fn fail_interrupted_operations(&self) -> Result<()> {
        self.conn.execute(
            "UPDATE operations SET result='interrupted', finished_at=?, error='application exited while the operation was running' WHERE result='running'",
            [now()],
        )?;
        Ok(())
    }

    #[allow(dead_code)]
    pub fn setting(&self, key: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row("SELECT value FROM application_settings WHERE key=?", [key], |r| r.get(0))
            .optional()?)
    }
}

fn enum_str<T: Serialize>(v: &T) -> String {
    serde_json::to_value(v).ok().and_then(|v| v.as_str().map(String::from)).unwrap_or_default()
}

fn parse_enum<T: serde::de::DeserializeOwned>(s: &str) -> Option<T> {
    serde_json::from_value(serde_json::Value::String(s.to_string())).ok()
}

pub fn normalize_name(name: &str) -> String {
    name.to_lowercase().replace(['_', '.'], "-")
}

/// Fills `also_found_in` for packages whose normalized name appears under more
/// than one manager. Rows stay separate: names alone are not proof of identity.
pub fn annotate_overlaps(pkgs: &mut [Package]) {
    let mut by_name: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for p in pkgs.iter() {
        let e = by_name.entry(normalize_name(&p.name)).or_default();
        if !e.contains(&p.package_manager) {
            e.push(p.package_manager.clone());
        }
    }
    for p in pkgs.iter_mut() {
        p.also_found_in = by_name[&normalize_name(&p.name)]
            .iter()
            .filter(|m| **m != p.package_manager)
            .cloned()
            .collect();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(id: &str) -> ManagerInfo {
        ManagerInfo {
            id: id.into(),
            display_name: id.into(),
            detected: true,
            path: Some("/usr/bin/x".into()),
            version: Some("1".into()),
            supports_updates: true,
            package_count: 0,
            last_error: None,
        }
    }

    #[test]
    fn operation_lifecycle_and_interruption() {
        let db = Db::open_in_memory().unwrap();
        let a = db.begin_operation("update", "typescript", "npm", Some("1.0.0"), "npm install -g typescript@latest").unwrap();
        db.append_log(a, "stdout", "hello").unwrap();
        db.finish_operation(a, "success", Some("2.0.0"), None).unwrap();
        let b = db.begin_operation("install", "x", "npm", None, "npm install -g x@latest").unwrap();
        db.fail_interrupted_operations().unwrap();
        let ops = db.list_operations(10).unwrap();
        assert_eq!(ops[0].id, b);
        assert_eq!(ops[0].result, "interrupted");
        assert_eq!(ops[1].result, "success");
        assert_eq!(ops[1].new_version.as_deref(), Some("2.0.0"));
        assert_eq!(db.operation_logs(a).unwrap()[0].line, "hello");
    }

    #[test]
    fn migrations_create_all_tables() {
        let db = Db::open_in_memory().unwrap();
        for t in [
            "package_managers", "installed_packages", "package_metadata", "package_dependencies",
            "operations", "operation_logs", "environment_manifests", "application_settings", "scan_history",
        ] {
            let n: i64 = db
                .conn
                .query_row("SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?", [t], |r| r.get(0))
                .unwrap();
            assert_eq!(n, 1, "missing table {t}");
        }
    }

    #[test]
    fn roundtrip_and_failure_keeps_stale_rows() {
        let db = Db::open_in_memory().unwrap();
        let mut p = Package::new("npm", "npm:typescript".into(), "typescript");
        p.version = Some("5.4.0".into());
        p.metadata.insert("scope".into(), "global".into());
        db.save_manager(&info("npm"), Some(&[p])).unwrap();

        let got = db.list_packages().unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].version.as_deref(), Some("5.4.0"));
        assert_eq!(got[0].homepage, None);
        assert_eq!(got[0].update_available, None);
        assert_eq!(got[0].metadata["scope"], "global");

        // Failed query: detected but no packages -> old rows retained.
        let mut failed = info("npm");
        failed.last_error = Some("boom".into());
        db.save_manager(&failed, None).unwrap();
        assert_eq!(db.list_packages().unwrap().len(), 1);
        assert_eq!(db.list_managers().unwrap()[0].last_error.as_deref(), Some("boom"));

        // Manager disappeared -> rows removed.
        let mut gone = info("npm");
        gone.detected = false;
        db.save_manager(&gone, None).unwrap();
        assert!(db.list_packages().unwrap().is_empty());
    }

    #[test]
    fn overlap_is_annotated_not_merged() {
        let mut v = vec![
            Package::new("apt", "apt:git:amd64".into(), "git"),
            Package::new("snap", "snap:git".into(), "Git"),
            Package::new("npm", "npm:left-pad".into(), "left-pad"),
        ];
        annotate_overlaps(&mut v);
        assert_eq!(v[0].also_found_in, vec!["snap"]);
        assert_eq!(v[1].also_found_in, vec!["apt"]);
        assert!(v[2].also_found_in.is_empty());
    }
}
