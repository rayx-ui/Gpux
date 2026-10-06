# 02: `gpux-zed-util` does not compile on macOS (`adopt_raw_pid`)

**This is the next blocker for the macOS CI job** once file 01 is in place.

## The Failure

Real macOS CI, run [37520997553](https://github.com/rayx-ui/Gpux/actions/runs/37520997553), PR #1:

```
error[E0599]: no function or associated item named `adopt_raw_pid` found for struct `smol::async_process::Child`
   --> crates/util/src/command/darwin.rs:497:43
497 |         let inner = smol::process::Child::adopt_raw_pid(pid as u32, true, kill_on_drop)?;
error: could not compile `gpux-zed-util` (lib) due to 1 previous error
```

## Root Cause

`crates/util/src/command/darwin.rs` (package `gpux-zed-util`, imported from upstream Zed in 0eeeb87,
Zed `c24358d`) spawns children with `posix_spawnp` and then hands the pid to async-process through
`Child::adopt_raw_pid`. **That function exists only in Zed's git fork of `async-process`.** Upstream
Zed's root `Cargo.toml` (at d544470d1d) has:

```toml
[patch.crates-io]
async-process = { git = "https://github.com/zed-industries/async-process.git", rev = "0b6d6713570af61806e1e5cb40e0f757cb93fd9d" }
```

gpux forbids git dependencies and `[patch]`, and the newest crates.io `async-process` (2.5.0) has
no such API. So this file has **never compiled in gpux on macOS**. Nobody noticed because macOS was
never built; Windows and Linux use the non-macOS branch of `crates/util/src/command.rs`, which wraps
`smol::process::Command`.

In the fork (`src/lib.rs`, line ~302), `adopt_raw_pid(pid, reap_on_drop, kill_on_drop)` registers
the pid with async-process's private `Reaper` (SIGCHLD or pidfd-driven) and returns an ordinary
`Child`. That reaper is private, so the behavior cannot be reproduced from outside the crate.

## Do Not Simply Drop `darwin.rs`

Routing macOS through the generic `smol::process` path would compile, but it loses deliberate
behavior that the file's comments and tests document:

- `POSIX_SPAWN_CLOEXEC_DEFAULT`, plus `FD_CLOEXEC` on its pipes. Without them, unrelated concurrent
  spawns (std and async-process both use `posix_spawn` without CLOEXEC_DEFAULT on Apple) inherit
  the pipe fds; a piped git child then deadlocks on `read()`.
  Test: `test_create_pipe_not_inherited_by_unrelated_spawn`.
- Resetting exception ports (`posix_spawnattr_setexceptionports_np`) and `SIGPIPE` to `SIG_DFL`.
- `chdir` through `posix_spawn_file_actions_addchdir_np`, and PATH resolution against the child's
  environment (the `test_bare_program_*` tests).

## Proposed Fix (written and type-checked, NOT committed)

Keep `darwin.rs` and replace only the fork-only call with a small gpux-owned handle in a new
submodule, `crates/util/src/command/darwin/process.rs`. That keeps the upstream file diff to three
lines, so future Zed imports stay reviewable. The handle:

- `id()` returns the pid.
- `kill()` sends `SIGKILL` unless the child is already reaped. It holds the status lock, so the pid
  cannot be reaped and recycled in between.
- `try_status()` reaps with `waitpid(WNOHANG)` under the lock.
- `status()` returns a `Send + 'static` future. It runs `smol::unblock`, blocks in
  `waitid(P_PID, WEXITED | WNOWAIT)` (which waits **without** reaping), then reaps under the lock.
  Every reap therefore happens exactly once under one mutex. An `ECHILD` from another waiter that
  reaped first returns the cached status.
- `Drop`: if the child is not reaped yet, it sends `SIGKILL` when `kill_on_drop` is set, then
  detaches a background `smol::unblock` waiter that reaps it, so no zombie is left. This preserves
  the fork's `reap_on_drop = true` semantics that `darwin.rs` relied on.

Status of the proposal:

- With it applied, `cargo check --target aarch64-apple-darwin -p gpux-zed-util --all-targets`
  passes with zero warnings. Without it, the same command reproduces CI's E0599. That check ran on
  Windows, using a stand-in C compiler so `zstd-sys`'s build script would pass; `cargo check` does
  not link.
- **Not yet run anywhere.** The handle's own tests and the existing `darwin.rs` tests
  (`test_drop_reaps_child`, `test_kill_on_drop_kills_and_reaps_child`, `test_spawn_status`,
  `test_spawn_*`, `test_stdio_*` ...) only run on macOS. An attempt to run `process.rs`'s tests on
  Linux in WSL was interrupted before it ran.
- `libc` 0.2.186 provides `waitid`, `P_PID`, `WEXITED`, `WNOWAIT` and `id_t` for Apple targets;
  Darwin's `waitid` documents `WNOWAIT`.

### `darwin.rs` diff

```diff
@@ -14,6 +14,10 @@ use std::path::{Path, PathBuf};
 use std::process::{ExitStatus, Output};
 use std::ptr;
 
+mod process;
+
+use process::Process;
+
 #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
 pub enum Stdio {
     /// A new pipe should be arranged to connect the parent and child processes.
@@ -229,7 +233,7 @@ impl Command {
 
 #[derive(Debug)]
 pub struct Child {
-    inner: smol::process::Child,
+    inner: Process,
     pub stdin: Option<Async<std::fs::File>>,
     pub stdout: Option<Async<std::fs::File>>,
     pub stderr: Option<Async<std::fs::File>>,
@@ -494,7 +498,7 @@ fn spawn_posix_spawn(
 
         cvt_nz(spawn_result)?;
 
-        let inner = smol::process::Child::adopt_raw_pid(pid as u32, true, kill_on_drop)?;
+        let inner = Process::new(pid, kill_on_drop);
 
         Ok(Child {
             inner,
```

### New file `crates/util/src/command/darwin/process.rs`

```rust
//! Waits on and reaps the pid that `posix_spawnp` returns.
//!
//! Upstream Zed adopts the pid into async-process through its git fork
//! (`Child::adopt_raw_pid`). gpux takes every dependency from crates.io, so it
//! reaps the pid itself. Every reap happens under one lock, so a pid is reaped
//! exactly once and cannot be recycled while `kill` signals it.

use std::future::Future;
use std::io;
use std::os::unix::process::ExitStatusExt as _;
use std::process::ExitStatus;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

#[derive(Debug)]
pub(super) struct Process {
    pid: libc::pid_t,
    status: Arc<Mutex<Option<ExitStatus>>>,
    kill_on_drop: bool,
}

impl Process {
    pub(super) fn new(pid: libc::pid_t, kill_on_drop: bool) -> Self {
        Self {
            pid,
            status: Arc::new(Mutex::new(None)),
            kill_on_drop,
        }
    }

    pub(super) fn id(&self) -> u32 {
        self.pid as u32
    }

    pub(super) fn kill(&mut self) -> io::Result<()> {
        let status = lock(&self.status);
        if status.is_some() {
            return Ok(());
        }
        // The pid stays unreaped while the lock is held, so it is still ours.
        if unsafe { libc::kill(self.pid, libc::SIGKILL) } == -1 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    pub(super) fn try_status(&mut self) -> io::Result<Option<ExitStatus>> {
        try_reap(self.pid, &mut lock(&self.status))
    }

    pub(super) fn status(&self) -> impl Future<Output = io::Result<ExitStatus>> + Send + 'static {
        let pid = self.pid;
        let status = self.status.clone();
        smol::unblock(move || wait(pid, &status))
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        if lock(&self.status).is_some() {
            return;
        }
        if self.kill_on_drop {
            self.kill().ok();
        }
        // Reap in the background so an exited child never lingers as a zombie.
        let pid = self.pid;
        let status = self.status.clone();
        smol::unblock(move || wait(pid, &status)).detach();
    }
}

fn lock(status: &Mutex<Option<ExitStatus>>) -> MutexGuard<'_, Option<ExitStatus>> {
    status.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Blocks until the process exits, then returns its status.
fn wait(pid: libc::pid_t, status: &Mutex<Option<ExitStatus>>) -> io::Result<ExitStatus> {
    loop {
        if let Some(exit_status) = *lock(status) {
            return Ok(exit_status);
        }
        // `WNOWAIT` leaves the exited child waitable, so the reap below stays
        // under the lock.
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        let result = unsafe {
            libc::waitid(
                libc::P_PID,
                pid as libc::id_t,
                &mut info,
                libc::WEXITED | libc::WNOWAIT,
            )
        };
        if result == -1 {
            let error = io::Error::last_os_error();
            match error.raw_os_error() {
                Some(libc::EINTR) => continue,
                // Another waiter reaped the child and cached its status.
                Some(libc::ECHILD) => return lock(status).ok_or(error),
                _ => return Err(error),
            }
        }
        if let Some(exit_status) = try_reap(pid, &mut lock(status))? {
            return Ok(exit_status);
        }
    }
}

/// Reaps the process if it has exited, caching its status.
fn try_reap(pid: libc::pid_t, status: &mut Option<ExitStatus>) -> io::Result<Option<ExitStatus>> {
    if status.is_some() {
        return Ok(*status);
    }
    let mut raw = 0;
    loop {
        match unsafe { libc::waitpid(pid, &mut raw, libc::WNOHANG) } {
            0 => return Ok(None),
            -1 => {
                let error = io::Error::last_os_error();
                if error.raw_os_error() != Some(libc::EINTR) {
                    return Err(error);
                }
            }
            _ => {
                *status = Some(ExitStatus::from_raw(raw));
                return Ok(*status);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spawn(program: &str, args: &[&str], kill_on_drop: bool) -> Process {
        // `std::process::Child` never reaps on drop, so its pid becomes ours.
        let child = std::process::Command::new(program)
            .args(args)
            .spawn()
            .expect("failed to spawn");
        Process::new(child.id() as libc::pid_t, kill_on_drop)
    }

    #[test]
    fn process_status_waiters_share_one_reaped_status() {
        let mut process = spawn("/bin/sh", &["-c", "exit 3"], false);

        let (first, second) = smol::block_on(futures_lite::future::zip(
            process.status(),
            process.status(),
        ));

        assert_eq!(first.unwrap().code(), Some(3));
        assert_eq!(second.unwrap().code(), Some(3));
        assert_eq!(process.try_status().unwrap().unwrap().code(), Some(3));
    }

    #[test]
    fn process_kill_ends_a_running_process() {
        let mut process = spawn("/bin/sleep", &["60"], false);
        assert!(process.try_status().unwrap().is_none());

        process.kill().unwrap();
        let status = smol::block_on(process.status()).unwrap();

        assert_eq!(status.signal(), Some(libc::SIGKILL));
        process.kill().unwrap();
    }
}
```

## Steps On The Mac

1. Apply the diff and new file above on `fix/metal-paint-contract`, or write something better. An
   alternative worth weighing is a crates.io release of Zed's async-process fork, if one ever
   appears (the gpux skill lists `zed-reqwest`, `zed-scap`, `zed-font-kit` and `zed-xim` as the
   precedent).
2. Run `cargo test -p gpux-zed-util command::darwin`. That covers the existing spawn, stdio, env,
   PATH, CLOEXEC, drop-reaping and kill-on-drop tests plus the two new `process_*` tests. Run them a
   few times (`-- --test-threads=1` and the default) to shake out reaping races.
3. Run `cargo clippy -p gpux-zed-util --all-targets --no-deps -- -D warnings`.
4. Run `cargo check --workspace --all-targets`. CI fails at this step; keep going until it is green.
   Crates the Windows cross-checks could not reach (`gpux-zed-util`, `gpux-bench-metrics`,
   `gpux-http-client-tls`, `gpux-reqwest-client`, and `gpux`/`gpux-platform` dev and bench targets)
   may still hide more macOS-only errors.
5. Commit, for example as `fix(util): reap posix_spawn children without async-process's git fork`,
   with a body explaining the crates.io-only rule. Push it on `fix/metal-paint-contract` or a
   separate branch.
6. Record the replay rule in the import guide
   (`.claude/skills/gpux/references/update-from-zed-gpui-closure.md`): upstream `darwin.rs` calls
   the fork-only `adopt_raw_pid`, and gpux replaces it with `darwin/process.rs`.
