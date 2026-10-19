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
    use std::time::{Duration, Instant};

    // The tests need a raw pid to hand to `Process`, which reaps it: `std::process::Child`
    // never reaps on drop, so its pid becomes ours.
    #[allow(clippy::disallowed_methods, clippy::zombie_processes)]
    fn spawn(program: &str, args: &[&str], kill_on_drop: bool) -> Process {
        let child = std::process::Command::new(program)
            .args(args)
            .spawn()
            .expect("failed to spawn");
        Process::new(child.id() as libc::pid_t, kill_on_drop)
    }

    /// Whether `pid` is gone and reaped. A zombie still answers `kill(pid, 0)`; only a reaped
    /// pid fails with `ESRCH`. Unlike `waitpid`, the probe never reaps the child itself.
    fn reaped(pid: libc::pid_t) -> bool {
        let result = unsafe { libc::kill(pid, 0) };
        result == -1 && io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
    }

    fn eventually(mut condition: impl FnMut() -> bool) -> bool {
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if condition() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        condition()
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

    #[test]
    fn process_drop_reaps_an_exited_child() {
        let process = spawn("/bin/sh", &["-c", "exit 0"], false);
        let pid = process.pid;
        drop(process);

        assert!(eventually(|| reaped(pid)), "pid {pid} was left a zombie");
    }

    #[test]
    fn process_drop_kills_and_reaps_with_kill_on_drop() {
        let process = spawn("/bin/sleep", &["60"], true);
        let pid = process.pid;
        drop(process);

        assert!(
            eventually(|| reaped(pid)),
            "pid {pid} survived its kill-on-drop handle"
        );
    }
}
