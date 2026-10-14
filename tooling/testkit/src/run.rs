//! Running lane invocations: output is echoed live and kept in the lane's artifacts folder.

use crate::lanes::{Invocation, OutputKind};
use crate::timings::TimedLine;
use std::io::{BufRead as _, BufReader, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Instant;

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

/// One invocation of a concurrent run.
pub struct RunJob<'a> {
    /// What to run.
    pub invocation: &'a Invocation,
    /// Its resolved program.
    pub program: &'a Path,
    /// Where to run it.
    pub workdir: &'a Path,
    /// Where its output is kept.
    pub log: PathBuf,
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

    /// Runs `jobs` side by side and returns each one's result and timed output, in order. Their
    /// output is not echoed (the caller prints each as one block). The default runs them one
    /// after the other.
    fn run_concurrently(
        &mut self,
        jobs: &[RunJob<'_>],
    ) -> Vec<(anyhow::Result<RunOutput>, Vec<TimedLine>)> {
        jobs.iter()
            .map(|job| {
                let result = self.run(job.invocation, job.program, job.workdir, &job.log);
                (result, self.take_timeline())
            })
            .collect()
    }

    /// The timed output lines of the last [`Runner::run`], taken (a second call returns nothing).
    /// Runners that do not time their output return none.
    fn take_timeline(&mut self) -> Vec<TimedLine> {
        Vec::new()
    }
}

/// Runs invocations as child processes, timing each output line.
#[derive(Default)]
pub struct ProcessRunner {
    timeline: Vec<TimedLine>,
    /// Keeps output out of the terminal (a concurrent run prints each block when it is done).
    quiet: bool,
}

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
        let timeline = Arc::new(Mutex::new(Vec::new()));
        let started = Instant::now();
        let echo = invocation.output != OutputKind::Listing && !self.quiet;
        let pump = |stream: Box<dyn std::io::Read + Send>, to_stderr: bool| {
            let collected = collected.clone();
            let log_file = log_file.clone();
            let timeline = timeline.clone();
            std::thread::spawn(move || {
                for line in BufReader::new(stream).lines() {
                    let Ok(line) = line else { break };
                    let at = started.elapsed();
                    if !echo {
                        // A listing is parsed and printed by the caller.
                    } else if to_stderr {
                        eprintln!("{line}");
                    } else {
                        println!("{line}");
                    }
                    writeln!(log_file.lock().unwrap(), "{line}").ok();
                    let mut collected = collected.lock().unwrap();
                    collected.push_str(&line);
                    collected.push('\n');
                    timeline.lock().unwrap().push(TimedLine { at, line });
                }
            })
        };
        let stdout = pump(Box::new(child.stdout.take().expect("piped")), false);
        let stderr = pump(Box::new(child.stderr.take().expect("piped")), true);
        let status = child.wait()?;
        stdout.join().ok();
        stderr.join().ok();
        let output = std::mem::take(&mut *collected.lock().unwrap());
        self.timeline = std::mem::take(&mut *timeline.lock().unwrap());
        Ok(RunOutput {
            success: status.success(),
            code: status.code(),
            output,
        })
    }

    fn run_concurrently(
        &mut self,
        jobs: &[RunJob<'_>],
    ) -> Vec<(anyhow::Result<RunOutput>, Vec<TimedLine>)> {
        std::thread::scope(|scope| {
            let handles: Vec<_> = jobs
                .iter()
                .map(|job| {
                    scope.spawn(move || {
                        let mut runner = ProcessRunner {
                            quiet: true,
                            ..ProcessRunner::default()
                        };
                        let result = runner.run(job.invocation, job.program, job.workdir, &job.log);
                        (result, runner.take_timeline())
                    })
                })
                .collect();
            handles
                .into_iter()
                .map(|handle| {
                    handle.join().unwrap_or_else(|_| {
                        (Err(anyhow::anyhow!("a lane worker panicked")), Vec::new())
                    })
                })
                .collect()
        })
    }

    fn take_timeline(&mut self) -> Vec<TimedLine> {
        std::mem::take(&mut self.timeline)
    }
}

/// `artifacts-temp/testkit/<lane>/` under the workspace root.
pub fn artifacts_dir(root: &Path, lane: &str) -> PathBuf {
    root.join("artifacts-temp").join("testkit").join(lane)
}

/// Runs `invocation` with `program` and hands each output line to `on_line` until it returns
/// `false`, the process exits, or `timeout` passes; then stops the process. Lines are also kept
/// in `log`. Returns whether `on_line` asked to stop (the awaited line arrived).
#[allow(
    clippy::disallowed_methods,
    reason = "the testkit is a synchronous command-line runner; following its child is its job"
)]
pub fn follow_lines(
    invocation: &Invocation,
    program: &Path,
    workdir: &Path,
    log: &Path,
    timeout: std::time::Duration,
    on_line: &mut dyn FnMut(&str) -> bool,
) -> anyhow::Result<bool> {
    if let Some(dir) = log.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut log_file = std::fs::File::create(log)?;
    writeln!(log_file, "$ {}", invocation.command_line())?;
    let mut child = Command::new(program)
        .args(&invocation.args)
        .envs(invocation.env.iter().map(|(name, value)| (name, value)))
        .current_dir(workdir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| anyhow::anyhow!("could not start `{}`: {error}", program.display()))?;
    let stdout = child.stdout.take().expect("piped");
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let Ok(line) = line else { break };
            if sender.send(line).is_err() {
                break;
            }
        }
    });
    let deadline = std::time::Instant::now() + timeout;
    let mut stopped = false;
    loop {
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        match receiver.recv_timeout(left) {
            Ok(line) => {
                writeln!(log_file, "{line}").ok();
                if !on_line(&line) {
                    stopped = true;
                    break;
                }
            }
            Err(_) => break,
        }
    }
    child.kill().ok();
    child.wait().ok();
    Ok(stopped)
}

/// Runs `program` with `args` and writes its raw standard output to `destination`, byte for
/// byte (for binary files `adb exec-out` streams).
#[allow(
    clippy::disallowed_methods,
    reason = "the testkit is a synchronous command-line runner; waiting on its child is its job"
)]
pub fn save_output(program: &Path, args: &[String], destination: &Path) -> anyhow::Result<()> {
    if let Some(dir) = destination.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let output = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|error| anyhow::anyhow!("could not start `{}`: {error}", program.display()))?;
    anyhow::ensure!(
        output.status.success(),
        "`{} {}` failed: {}",
        program.display(),
        args.join(" "),
        String::from_utf8_lossy(&output.stderr).trim()
    );
    std::fs::write(destination, output.stdout)?;
    Ok(())
}
