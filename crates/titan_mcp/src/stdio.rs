//! Interruptible, bounded stdio for the binary's shutdown path.
//!
//! Stdin itself can block indefinitely. A single worker owns that blocking read;
//! the protocol thread polls a bounded channel and can return on a signal even
//! when the MCP client holds stdin open. The process exits after manager cleanup,
//! so a worker blocked in stdin is not joined on shutdown.

use std::{
    io::{self, Cursor, Read, Write},
    sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender},
    thread,
    time::{Duration, Instant},
};
use titan_mcp::process::ProcessCancellation;

pub(crate) struct StdinReader {
    chunks: Receiver<io::Result<Vec<u8>>>,
    pending: Cursor<Vec<u8>>,
    cancellation: ProcessCancellation,
}

impl StdinReader {
    pub(crate) fn new(cancellation: ProcessCancellation) -> Self {
        let (sender, chunks) = mpsc::sync_channel(1);
        thread::spawn(move || {
            let mut stdin = io::stdin().lock();
            loop {
                let mut buffer = vec![0; 8192];
                match stdin.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(count) => {
                        buffer.truncate(count);
                        if sender.send(Ok(buffer)).is_err() {
                            break;
                        }
                    }
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                    Err(error) => {
                        let _ = sender.send(Err(error));
                        break;
                    }
                }
            }
        });
        Self {
            chunks,
            pending: Cursor::new(Vec::new()),
            cancellation,
        }
    }
}

enum OutputJob {
    Write(Vec<u8>, SyncSender<io::Result<()>>),
    Flush(SyncSender<io::Result<()>>),
}

/// One outstanding, at-most-8-KiB write, acknowledged by a blocking worker.
/// The protocol thread can abandon the worker on shutdown without waiting for
/// a client to drain stdout. Ordinary write/flush errors still propagate.
pub(crate) struct StdoutWriter {
    jobs: SyncSender<OutputJob>,
    cancellation: ProcessCancellation,
    deadline: Option<Instant>,
}

impl StdoutWriter {
    pub(crate) fn new(cancellation: ProcessCancellation) -> Self {
        let (jobs, receiver) = mpsc::sync_channel(1);
        thread::spawn(move || {
            let mut stdout = io::stdout().lock();
            while let Ok(job) = receiver.recv() {
                let (result, ack) = match job {
                    OutputJob::Write(bytes, ack) => (stdout.write_all(&bytes), ack),
                    OutputJob::Flush(ack) => (stdout.flush(), ack),
                };
                let failed = result.is_err();
                let _ = ack.send(result);
                if failed {
                    break;
                }
            }
        });
        Self {
            jobs,
            cancellation,
            deadline: None,
        }
    }

    fn complete(&mut self, job: OutputJob, ack: Receiver<io::Result<()>>) -> io::Result<()> {
        let disconnected = || {
            io::Error::new(
                io::ErrorKind::BrokenPipe,
                "MCP output closed or shutdown requested",
            )
        };
        if self.cancellation.is_requested() {
            return Err(disconnected());
        }
        // Previous jobs are acknowledged before another is sent: the queue can
        // contain at most one bounded chunk. Never block on queue backpressure.
        let deadline = *self
            .deadline
            .get_or_insert_with(|| Instant::now() + Duration::from_secs(10));
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "MCP output backpressure exceeded 10 seconds",
            ));
        }
        self.jobs.try_send(job).map_err(|_| disconnected())?;
        loop {
            if self.cancellation.is_requested() {
                return Err(disconnected());
            }
            if Instant::now() >= deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "MCP output backpressure exceeded 10 seconds",
                ));
            }
            match ack.recv_timeout(
                Duration::from_millis(25).min(deadline.saturating_duration_since(Instant::now())),
            ) {
                Ok(result) => return result,
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => return Err(disconnected()),
            }
        }
    }
}

impl Write for StdoutWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let count = bytes.len().min(8192);
        if count == 0 {
            return Ok(0);
        }
        let (sender, ack) = mpsc::sync_channel(1);
        self.complete(OutputJob::Write(bytes[..count].to_vec(), sender), ack)?;
        Ok(count)
    }

    fn flush(&mut self) -> io::Result<()> {
        let (sender, ack) = mpsc::sync_channel(1);
        self.complete(OutputJob::Flush(sender), ack)?;
        // All chunks between successful flushes share one response budget.
        self.deadline = None;
        Ok(())
    }
}

impl Read for StdinReader {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        loop {
            if self.cancellation.is_requested() {
                return Ok(0);
            }
            let count = self.pending.read(buffer)?;
            if count > 0 {
                return Ok(count);
            }
            match self.chunks.recv_timeout(Duration::from_millis(25)) {
                Ok(chunk) => self.pending = Cursor::new(chunk?),
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => return Ok(0),
            }
        }
    }
}
