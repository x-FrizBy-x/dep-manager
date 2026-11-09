//! Safe process execution. Programs are launched directly with argument
//! arrays — never through a shell — and with a hard timeout.

use crate::error::{AppError, Result};
use std::io::{BufRead, BufReader, Read};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Default)]
pub struct CmdOutput {
    pub status: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

impl CmdOutput {
    pub fn success(&self) -> bool {
        self.status == Some(0)
    }
    /// Error unless exit status is 0.
    pub fn ok(self) -> Result<CmdOutput> {
        if self.success() {
            Ok(self)
        } else {
            Err(AppError::NonZero {
                status: self.status.unwrap_or(-1),
                stderr: self.stderr.trim().chars().take(500).collect(),
            })
        }
    }
}

/// Abstraction over process execution so adapters can be tested with fixtures.
pub trait Runner: Send + Sync {
    fn which(&self, program: &str) -> Option<PathBuf>;
    fn run(&self, program: &str, args: &[&str]) -> Result<CmdOutput>;
}

pub struct SystemRunner {
    pub timeout: Duration,
}

impl Default for SystemRunner {
    fn default() -> Self {
        SystemRunner { timeout: Duration::from_secs(60) }
    }
}

fn drain<R: Read + Send + 'static>(mut r: R) -> std::thread::JoinHandle<String> {
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = r.read_to_end(&mut buf);
        String::from_utf8_lossy(&buf).into_owned()
    })
}

impl Runner for SystemRunner {
    fn which(&self, program: &str) -> Option<PathBuf> {
        which::which(program).ok()
    }

    fn run(&self, program: &str, args: &[&str]) -> Result<CmdOutput> {
        let path = self
            .which(program)
            .ok_or_else(|| AppError::Spawn(format!("{program} not found in PATH")))?;
        let mut cmd = Command::new(&path);
        cmd.args(args)
            // Stable, parseable, untranslated output.
            .env("LC_ALL", "C")
            .env("LANG", "C")
            .env("NO_COLOR", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }
        let display = format!("{program} {}", args.join(" "));
        let mut child = cmd.spawn().map_err(|e| AppError::Spawn(format!("{display}: {e}")))?;
        let out = drain(child.stdout.take().expect("piped"));
        let err = drain(child.stderr.take().expect("piped"));

        let start = Instant::now();
        let status = loop {
            match child.try_wait().map_err(|e| AppError::Spawn(e.to_string()))? {
                Some(s) => break s,
                None if start.elapsed() > self.timeout => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(AppError::Timeout(self.timeout.as_secs(), display));
                }
                None => std::thread::sleep(Duration::from_millis(20)),
            }
        };
        Ok(CmdOutput {
            status: status.code(),
            stdout: out.join().unwrap_or_default(),
            stderr: err.join().unwrap_or_default(),
        })
    }
}

/// Callback receiving each output line: `(stream, line)` where stream is
/// `"stdout"` or `"stderr"`.
pub type LineSink<'a> = &'a (dyn Fn(&'static str, &str) + Sync);

#[derive(Debug, Clone, Default)]
pub struct StreamResult {
    pub status: Option<i32>,
    pub cancelled: bool,
    pub timed_out: bool,
}

/// Runs a program (no shell, argument array) and streams its output line by
/// line. Honors `cancel` and `timeout` by killing the direct child process.
pub fn run_streaming(
    program: &str,
    args: &[&str],
    timeout: Duration,
    cancel: &AtomicBool,
    on_line: LineSink,
) -> Result<StreamResult> {
    let path = which::which(program).map_err(|_| AppError::Spawn(format!("{program} not found in PATH")))?;
    let mut cmd = Command::new(&path);
    cmd.args(args)
        .env("LC_ALL", "C")
        .env("LANG", "C")
        .env("NO_COLOR", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let mut child = cmd.spawn().map_err(|e| AppError::Spawn(format!("{program}: {e}")))?;

    let (tx, rx) = mpsc::channel::<(&'static str, String)>();
    fn reader<R: Read + Send + 'static>(stream: &'static str, r: R, tx: mpsc::Sender<(&'static str, String)>) {
        std::thread::spawn(move || {
            let mut br = BufReader::new(r);
            let mut buf = Vec::new();
            loop {
                buf.clear();
                match br.read_until(b'\n', &mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {
                        let line = String::from_utf8_lossy(&buf).trim_end_matches(['\n', '\r']).to_string();
                        if tx.send((stream, line)).is_err() {
                            break;
                        }
                    }
                }
            }
        });
    }
    reader("stdout", child.stdout.take().expect("piped"), tx.clone());
    reader("stderr", child.stderr.take().expect("piped"), tx);

    let start = Instant::now();
    let mut res = StreamResult::default();
    loop {
        match rx.recv_timeout(Duration::from_millis(50)) {
            Ok((stream, line)) => on_line(stream, &line),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                res.status = child.wait().map_err(|e| AppError::Spawn(e.to_string()))?.code();
                break;
            }
        }
        if let Some(s) = child.try_wait().map_err(|e| AppError::Spawn(e.to_string()))? {
            res.status = s.code();
            while let Ok((stream, line)) = rx.recv_timeout(Duration::from_millis(200)) {
                on_line(stream, &line);
            }
            break;
        }
        if cancel.load(Ordering::Acquire) {
            let _ = child.kill();
            let _ = child.wait();
            res.cancelled = true;
            break;
        }
        if start.elapsed() > timeout {
            let _ = child.kill();
            let _ = child.wait();
            res.timed_out = true;
            break;
        }
    }
    Ok(res)
}

#[cfg(test)]
mod stream_tests {
    use super::*;
    use std::sync::Mutex;

    #[test]
    fn streams_lines_and_exit_code() {
        let lines = Mutex::new(Vec::new());
        let cancel = AtomicBool::new(false);
        let r = run_streaming("cargo", &["--version"], Duration::from_secs(30), &cancel, &|s, l| {
            lines.lock().unwrap().push((s, l.to_string()))
        })
        .unwrap();
        assert_eq!(r.status, Some(0));
        assert!(lines.lock().unwrap().iter().any(|(s, l)| *s == "stdout" && l.starts_with("cargo")));
    }

    #[test]
    fn nonzero_exit_and_missing_program() {
        let cancel = AtomicBool::new(false);
        let r = run_streaming("cargo", &["--definitely-not-a-flag"], Duration::from_secs(30), &cancel, &|_, _| {}).unwrap();
        assert_ne!(r.status, Some(0));
        assert!(run_streaming("no-such-program-xyz", &[], Duration::from_secs(1), &cancel, &|_, _| {}).is_err());
    }

    #[test]
    fn pre_cancelled_run_is_killed() {
        let cancel = AtomicBool::new(true);
        let r = run_streaming("cargo", &["--list"], Duration::from_secs(30), &cancel, &|_, _| {}).unwrap();
        assert!(r.cancelled || r.status.is_some());
    }
}

#[cfg(test)]
pub mod fake {
    use super::*;
    use std::collections::HashMap;

    /// Fixture-driven runner: key is `"program arg1 arg2"`.
    #[derive(Default)]
    pub struct FakeRunner {
        pub programs: Vec<&'static str>,
        pub outputs: HashMap<String, CmdOutput>,
    }

    impl FakeRunner {
        pub fn new(programs: &[&'static str]) -> Self {
            FakeRunner { programs: programs.to_vec(), outputs: HashMap::new() }
        }
        pub fn on(mut self, cmdline: &str, status: i32, stdout: &str) -> Self {
            self.outputs.insert(
                cmdline.to_string(),
                CmdOutput { status: Some(status), stdout: stdout.to_string(), stderr: String::new() },
            );
            self
        }
    }

    impl Runner for FakeRunner {
        fn which(&self, program: &str) -> Option<PathBuf> {
            self.programs.contains(&program).then(|| PathBuf::from(format!("/usr/bin/{program}")))
        }
        fn run(&self, program: &str, args: &[&str]) -> Result<CmdOutput> {
            let key = format!("{program} {}", args.join(" ")).trim().to_string();
            self.outputs
                .get(&key)
                .cloned()
                .ok_or_else(|| AppError::Spawn(format!("no fixture for `{key}`")))
        }
    }
}
