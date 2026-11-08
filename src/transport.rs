//! The only process boundary: private stdio, never a listening HTTP/TCP service.
use serde_json::Value;
use std::{
    io::{BufRead, BufReader, Write},
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::mpsc::{self, Receiver, Sender},
    thread,
};

pub enum Event {
    Json(Value),
    Stopped,
}
pub struct Bridge {
    child: Child,
    requests: Sender<Value>,
    pub events: Receiver<Event>,
}
impl Bridge {
    pub fn start(wake: rune::Waker) -> Result<Self, String> {
        let dir = data_dir()?;
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))
                .map_err(|e| e.to_string())?;
        }
        let exe = std::env::var_os("SIGNAL_RUNE_CLI")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                let app_exe = std::env::current_exe().unwrap_or_default();
                let base = app_exe.parent().unwrap_or(std::path::Path::new("."));
                let launcher = if cfg!(windows) {
                    "bin/signal-cli.bat"
                } else {
                    "bin/signal-cli"
                };
                let sibling = base.join("runtime").join(launcher);
                let resources = base
                    .parent()
                    .unwrap_or(base)
                    .join("Resources/runtime")
                    .join(launcher);
                if sibling.exists() {
                    sibling
                } else if cfg!(target_os = "macos") && resources.exists() {
                    resources
                } else {
                    PathBuf::from("signal-cli")
                }
            });
        let mut cmd = Command::new(&exe);
        let bundled_java = exe.parent().and_then(|p| p.parent()).map(|p| p.join("jre"));
        if let Some(java) = bundled_java.filter(|p| p.exists()) {
            cmd.env("JAVA_HOME", java);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x08000000);
        }
        let mut child = cmd.arg("--data-dir").arg(dir).arg("jsonRpc")
            .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn()
            .map_err(|_| "Signal engine could not start. Install signal-cli 0.14.10 and Java 25, or set SIGNAL_RUNE_CLI to its launcher. Then choose Retry connection.".to_string())?;
        let input = child.stdin.take().ok_or("No engine input")?;
        let output = child.stdout.take().ok_or("No engine output")?;
        let (requests, rx) = mpsc::channel::<Value>();
        let (events, event_rx) = mpsc::channel();
        thread::spawn(move || {
            let mut input = input;
            for request in rx {
                if writeln!(input, "{request}")
                    .and_then(|_| input.flush())
                    .is_err()
                {
                    break;
                }
            }
        });
        thread::spawn(move || {
            for line in BufReader::new(output).lines() {
                let Ok(line) = line else { break };
                if let Ok(value) = serde_json::from_str(&line) {
                    if events.send(Event::Json(value)).is_err() {
                        return;
                    }
                    wake();
                }
            }
            let _ = events.send(Event::Stopped);
            wake();
        });
        Ok(Self {
            child,
            requests,
            events: event_rx,
        })
    }
    pub fn send(&self, v: Value) -> Result<(), String> {
        self.requests
            .send(v)
            .map_err(|_| "Signal engine disconnected".into())
    }
}
impl Drop for Bridge {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
pub fn data_dir() -> Result<PathBuf, String> {
    directories::ProjectDirs::from("org", "signal-rune", "Signal Rune")
        .map(|d| d.data_local_dir().join("engine"))
        .ok_or("Cannot locate application data directory".into())
}
