//! What generation, identity analysis and export share about a paid AI job: which attempt is
//! still the live one, how a provider failure maps to a job outcome, and the notices shown
//! when a job stops.
use crate::native_files::LockExt;
use desegnos_ai::{
    AuthenticationMethod, CancellationToken, ErrorKind, ProviderError, SubmissionOutcome,
};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};
use uuid::Uuid;

#[derive(Clone, Default)]
pub struct SwitchGuard(Arc<Mutex<BTreeMap<Uuid, CancellationToken>>>);

impl SwitchGuard {
    pub fn ensure_switch_allowed(&self) -> Result<(), String> {
        if self.0.locked().values().any(|token| !token.is_cancelled()) {
            return Err("An AI request is still running. Wait for its result or cancel it before switching projects; local editing and saving remain available.".into());
        }
        Ok(())
    }

    pub fn track(&self, token: CancellationToken) -> RunningRequest {
        let id = Uuid::new_v4();
        self.0.locked().insert(id, token);
        RunningRequest {
            guard: self.clone(),
            id,
        }
    }
}

pub struct RunningRequest {
    guard: SwitchGuard,
    id: Uuid,
}

impl Drop for RunningRequest {
    fn drop(&mut self) {
        self.guard.0.locked().remove(&self.id);
    }
}

pub struct ActiveJobs {
    jobs: BTreeMap<Uuid, (usize, CancellationToken, RunningRequest)>,
    switch_guard: SwitchGuard,
}

impl ActiveJobs {
    pub fn new(switch_guard: SwitchGuard) -> Self {
        Self {
            jobs: BTreeMap::new(),
            switch_guard,
        }
    }

    pub fn start(&mut self, job_id: Uuid, attempt: usize) -> CancellationToken {
        let token = CancellationToken::new();
        let running = self.switch_guard.track(token.clone());
        self.jobs.insert(job_id, (attempt, token.clone(), running));
        token
    }

    pub fn is_registered(&self, job_id: Uuid, attempt: usize) -> bool {
        self.jobs
            .get(&job_id)
            .is_some_and(|(live, _, _)| *live == attempt)
    }

    pub fn is_current(&self, job_id: Uuid, attempt: usize) -> bool {
        self.jobs
            .get(&job_id)
            .is_some_and(|(live, token, _)| *live == attempt && !token.is_cancelled())
    }

    pub fn is_cancelled(&self, job_id: Uuid) -> bool {
        self.jobs
            .get(&job_id)
            .is_some_and(|(_, token, _)| token.is_cancelled())
    }

    pub fn contains(&self, job_id: Uuid) -> bool {
        self.jobs.contains_key(&job_id)
    }

    pub fn cancel(&self, job_id: Uuid) {
        if let Some((_, token, _)) = self.jobs.get(&job_id) {
            token.cancel();
        }
    }

    pub fn remove(&mut self, job_id: Uuid) {
        self.jobs.remove(&job_id);
    }

    pub fn len(&self) -> usize {
        self.jobs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.jobs.is_empty()
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Failure {
    Cancelled,
    /// The request may have been accepted, so the outcome is unknown.
    Unknown,
    Failed,
}

impl Failure {
    pub fn of(error: &ProviderError) -> Self {
        if error.kind == ErrorKind::Cancelled {
            Self::Cancelled
        } else if error.outcome == SubmissionOutcome::Unknown
            && !error.http_status.is_some_and(|status| status >= 400)
        {
            Self::Unknown
        } else {
            Self::Failed
        }
    }
}

fn charge_warning(authentication: AuthenticationMethod) -> &'static str {
    if authentication == AuthenticationMethod::Chatgpt {
        "may still consume ChatGPT plan allowance or credits"
    } else {
        "may still incur a charge"
    }
}

pub fn stopped_notice(authentication: AuthenticationMethod) -> String {
    format!(
        "Stopped. A submitted request {}. Late output cannot be applied.",
        charge_warning(authentication)
    )
}

pub fn signed_out_notice(authentication: AuthenticationMethod) -> String {
    format!(
        "Account signed out. The request was cancelled and late output cannot be applied. A submitted request {}.",
        charge_warning(authentication)
    )
}

/// Shown when a job was stopped but its cancellation record could not be written.
pub fn unsaved_cancel_notice(error: &str) -> String {
    format!(
        "Cancelled and late output blocked, but its local cancellation record could not be saved: {error} Restore writable storage. A submitted request may still incur a charge; restart will mark it uncertain and never resubmit automatically."
    )
}

pub fn interrupted_notice(subject: &str) -> String {
    format!(
        "{subject} was interrupted. Its outcome may be uncertain; it will not be sent again automatically. Review before explicitly trying again."
    )
}
