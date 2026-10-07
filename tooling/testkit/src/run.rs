//! Running lane invocations: output is echoed live and kept in the lane's artifacts folder.

use crate::lanes::Invocation;
use std::io::{BufRead as _, BufReader, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};

/// What one invocation produced.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunOutput {
    /// Whether the process exited successfully.
    pub success: bool,
    /// Its exit code, when it exited normally.
    pub code: Option<i32>,
    /// Its stdout and stderr, interleaved in arrival order.
    pub output: String,
}

/// Runs invocations.
pub trait Runner {
    /// Runs `invocation` with `program` (its resolved path) in `workdir`, writing its output to
    /// `log` as well.
    fn run(
        &mut self,
        invocation: &Invocation,
        program: &Path,
        workdir: &Path,
        log: &Path,
    ) -> anyhow::Result<RunOutput>;
}

/// Runs invocations as child processes.
pub struct ProcessRunner;

impl Runner for ProcessRunner {
    #[allow(
        clippy::disallowed_methods,
        reason = "the testkit is a synchronous command-line runner; waiting on its child is its job"
    )]
    fn run(
        &mut self,
        invocation: &Invocation,
        program: &Path,
        workdir: &Path,
        log: &Path,
    ) -> anyhow::Result<RunOutput> {
        if let Some(dir) = log.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let log_file = Arc::new(Mutex::new(std::fs::File::create(log)?));
        writeln!(log_file.lock().unwrap(), "$ {}", invocation.command_line())?;
        let mut child = Command::new(program)
            .args(&invocation.args)
            .envs(invocation.env.iter().map(|(name, value)| (name, value)))
            .current_dir(workdir)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| anyhow::anyhow!("could not start `{}`: {error}", program.display()))?;
        let collected = Arc::new(Mutex::new(String::new()));
        let pump = |stream: Box<dyn std::io::Read + Send>, to_stderr: bool| {
            let collected = collected.clone();
            let log_file = log_file.clone();
            std::thread::spawn(move || {
                for line in BufReader::new(stream).lines() {
                    let Ok(line) = line else { break };
                    if to_stderr {
                        eprintln!("{line}");
                    } else {
                        println!("{line}");
                    }
                    writeln!(log_file.lock().unwrap(), "{line}").ok();
                    let mut collected = collected.lock().unwrap();
                    collected.push_str(&line);
                    collected.push('\n');
                }
            })
        };
        let stdout = pump(Box::new(child.stdout.take().expect("piped")), false);
        let stderr = pump(Box::new(child.stderr.take().expect("piped")), true);
        let status = child.wait()?;
        stdout.join().ok();
        stderr.join().ok();
        let output = std::mem::take(&mut *collected.lock().unwrap());
        Ok(RunOutput {
            success: status.success(),
            code: status.code(),
            output,
        })
    }
}

/// `artifacts-temp/testkit/<lane>/` under the workspace root.
pub fn artifacts_dir(root: &Path, lane: &str) -> PathBuf {
    root.join("artifacts-temp").join("testkit").join(lane)
}
