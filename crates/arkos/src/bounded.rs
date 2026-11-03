//! Runs a child process under a memory ceiling and a time limit, and hands back what it printed.
//! The ceiling is the child's physical footprint, the figure Activity Monitor shows, polled
//! while it runs; a child over either limit is killed.

use std::io::Read;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// The limits a child runs under.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub memory: u64,
    pub time: Duration,
}

/// Why a child gave back nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ended {
    /// Its footprint passed the ceiling.
    Memory,
    /// It ran past the time limit.
    Time,
    /// It could not start, or exited other than cleanly.
    Failed(String),
}

/// How often the child's footprint and the clock are checked. A child filling memory as fast
/// as it can gets a few hundred MB past the ceiling in this time at most.
const POLL: Duration = Duration::from_millis(10);

/// Runs `command` with its standard output captured and its input and errors discarded, and
/// waits for it. The output is read on its own thread while the child runs, so a child printing
/// more than a pipe holds never stalls.
pub fn run(mut command: Command, limits: Limits) -> Result<Vec<u8>, Ended> {
    command.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
    let mut child = command.spawn().map_err(|e| Ended::Failed(e.to_string()))?;
    let mut stdout = child.stdout.take().ok_or_else(|| Ended::Failed("no output pipe".to_string()))?;
    let reader = std::thread::spawn(move || {
        let mut out = Vec::new();
        stdout.read_to_end(&mut out).map(|_| out)
    });
    let start = Instant::now();
    let stopped = loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => break None,
            Ok(Some(status)) => break Some(Ended::Failed(status.to_string())),
            Ok(None) => {}
            Err(e) => break Some(stop(&mut child, Ended::Failed(e.to_string()))),
        }
        if footprint(child.id()).is_some_and(|bytes| bytes > limits.memory) {
            break Some(stop(&mut child, Ended::Memory));
        }
        if start.elapsed() > limits.time {
            break Some(stop(&mut child, Ended::Time));
        }
        std::thread::sleep(POLL);
    };
    // The pipe closes when the child is gone, so the reader always finishes.
    let out = reader.join().map_err(|_| Ended::Failed("the output reader panicked".to_string()))?;
    match stopped {
        Some(why) => Err(why),
        None => out.map_err(|e| Ended::Failed(e.to_string())),
    }
}

/// Kills and reaps the child, and passes on why.
fn stop(child: &mut Child, why: Ended) -> Ended {
    let _ = child.kill();
    let _ = child.wait();
    why
}

/// A live process's physical footprint in bytes: its dirty memory, compressed memory counted at
/// full size, the figure the kernel's own memory limits use.
fn footprint(pid: u32) -> Option<u64> {
    let mut info: libc::rusage_info_v2 = unsafe { std::mem::zeroed() };
    let buffer = (&mut info as *mut libc::rusage_info_v2).cast::<libc::rusage_info_t>();
    // SAFETY: `info` is a writable rusage_info_v2, the struct the V2 flavour fills.
    let status = unsafe { libc::proc_pid_rusage(pid as libc::c_int, libc::RUSAGE_INFO_V2, buffer) };
    (status == 0).then_some(info.ri_phys_footprint)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sh(script: &str) -> Command {
        let mut c = Command::new("/bin/sh");
        c.args(["-c", script]);
        c
    }

    const ROOMY: Limits = Limits { memory: 1 << 30, time: Duration::from_secs(20) };

    #[test]
    fn a_child_that_finishes_hands_back_what_it_printed() {
        assert_eq!(run(sh("printf hello"), ROOMY), Ok(b"hello".to_vec()));
    }

    #[test]
    fn a_child_that_exits_with_an_error_has_failed() {
        assert!(matches!(run(sh("exit 3"), ROOMY), Err(Ended::Failed(_))));
    }

    #[test]
    fn a_child_that_cannot_start_has_failed() {
        assert!(matches!(run(Command::new("/nonexistent/arkos"), ROOMY), Err(Ended::Failed(_))));
    }

    #[test]
    fn a_child_over_the_memory_ceiling_is_stopped_at_once() {
        // 400 MB of string against a 100 MB ceiling, then a long sleep the kill cuts short.
        let mut perl = Command::new("/usr/bin/perl");
        perl.args(["-e", "my $x = 'x' x (400 * 1024 * 1024); sleep 30"]);
        let start = Instant::now();
        let limits = Limits { memory: 100 << 20, time: Duration::from_secs(20) };
        assert_eq!(run(perl, limits), Err(Ended::Memory));
        assert!(start.elapsed() < Duration::from_secs(10), "took {:?}", start.elapsed());
    }

    #[test]
    fn a_child_past_the_time_limit_is_stopped_at_once() {
        let start = Instant::now();
        let limits = Limits { memory: 1 << 30, time: Duration::from_millis(300) };
        assert_eq!(run(sh("sleep 30"), limits), Err(Ended::Time));
        assert!(start.elapsed() < Duration::from_secs(5), "took {:?}", start.elapsed());
    }

    #[test]
    fn a_child_that_prints_more_than_a_pipe_holds_is_read_in_full() {
        // 1 MB is well past the 64 KB a pipe buffers, so the child blocks unless it is drained.
        let out = run(sh("head -c 1048576 /dev/zero"), ROOMY).unwrap();
        assert_eq!(out.len(), 1 << 20);
    }
}
