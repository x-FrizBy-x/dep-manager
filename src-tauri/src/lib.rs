pub mod adapters;
pub mod commands;
pub mod db;
pub mod error;
pub mod exec;
pub mod model;
pub mod ops;
pub mod scanner;

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::{Arc, Mutex};
use tauri::Manager;

pub struct AppState {
    pub db: Arc<Mutex<db::Db>>,
    /// Prevents overlapping scans.
    pub scanning: Arc<AtomicBool>,
    /// Global write lock: at most one operation at a time.
    pub op_running: Arc<AtomicBool>,
    pub cancel: Arc<AtomicBool>,
    /// Previewed plans awaiting confirmation.
    pub plans: Mutex<HashMap<String, ops::OperationPlan>>,
    pub plan_counter: AtomicU64,
}

pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&dir)?;
            let db = db::Db::open(&dir.join("unimanage.sqlite"))?;
            db.fail_interrupted_scans()?;
            db.fail_interrupted_operations()?;
            app.manage(AppState {
                db: Arc::new(Mutex::new(db)),
                scanning: Arc::new(AtomicBool::new(false)),
                op_running: Arc::new(AtomicBool::new(false)),
                cancel: Arc::new(AtomicBool::new(false)),
                plans: Mutex::new(HashMap::new()),
                plan_counter: AtomicU64::new(0),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::start_scan,
            commands::get_packages,
            commands::get_managers,
            commands::get_scan_history,
            commands::preview_operation,
            commands::run_operation,
            commands::cancel_operation,
            commands::get_operations,
            commands::get_operation_logs,
        ])
        .run(tauri::generate_context!())
        .expect("error while running UniManage");
}
