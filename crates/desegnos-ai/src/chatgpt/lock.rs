use super::{ChatgptErrorKind as Kind, ChatgptResult};
use crate::CancellationToken;
use std::{
    fs::File,
    time::{Duration, Instant},
};

const POLL: Duration = Duration::from_millis(25);
// Metadata operations are short; refresh leases can cover a network round trip.
const METADATA_WAIT: Duration = Duration::from_secs(5);
const LEASE_WAIT: Duration = crate::net::IDLE;

fn acquired(file: &File) -> ChatgptResult<bool> {
    match file.try_lock() {
        Ok(()) => Ok(true),
        Err(std::fs::TryLockError::WouldBlock) => Ok(false),
        Err(_) => Err(Kind::Storage.into()),
    }
}

pub(super) fn metadata(file: File) -> ChatgptResult<File> {
    let deadline = Instant::now() + METADATA_WAIT;
    loop {
        if acquired(&file)? {
            return Ok(file);
        }
        if Instant::now() >= deadline {
            return Err(Kind::Busy.into());
        }
        std::thread::sleep(POLL.min(deadline.saturating_duration_since(Instant::now())));
    }
}

pub(super) async fn lease(file: File, cancel: &CancellationToken) -> ChatgptResult<File> {
    let deadline = tokio::time::Instant::now() + LEASE_WAIT;
    loop {
        if cancel.is_cancelled() {
            return Err(Kind::Cancelled.into());
        }
        if acquired(&file)? {
            return Ok(file);
        }
        tokio::select! {
            biased;
            _ = cancel.cancelled() => return Err(Kind::Cancelled.into()),
            _ = tokio::time::sleep_until(deadline) => return Err(Kind::Busy.into()),
            _ = tokio::time::sleep(POLL) => {},
        }
    }
}
