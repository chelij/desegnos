//! What generation, identity analysis and export share about a paid AI job: which attempt is
//! still the live one, how a provider failure maps to a job outcome, and the notices shown
//! when a job stops. Sign-out cancels every service from here.
use crate::{export::ExportService, generation::GenerationService, identity::IdentityService};
use desegnos_ai::{
    AuthenticationMethod, CancellationToken, ErrorKind, ProviderError, SubmissionOutcome,
};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, MutexGuard, PoisonError},
};
use uuid::Uuid;

/// Lock shared state even if another thread panicked while holding it. A job's outcome must
/// still be recorded, or it would stay running forever with no error.
pub fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[derive(Clone, Default)]
pub struct SwitchGuard(Arc<Mutex<BTreeMap<Uuid, CancellationToken>>>);

impl SwitchGuard {
    pub fn ensure_switch_allowed(&self) -> Result<(), String> {
        if lock(&self.0).values().any(|token| !token.is_cancelled()) {
            return Err("An AI request is still running. Wait for its result or cancel it before switching projects; local editing and saving remain available.".into());
        }
        Ok(())
    }

    pub fn track(&self, token: CancellationToken) -> RunningRequest {
        let id = Uuid::new_v4();
        lock(&self.0).insert(id, token);
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
        lock(&self.guard.0).remove(&self.id);
    }
}

#[derive(Default)]
pub struct ActiveJobs {
    jobs: BTreeMap<Uuid, (usize, CancellationToken, RunningRequest)>,
    switch_guard: SwitchGuard,
}

impl ActiveJobs {
    pub fn bind_switch_guard(&mut self, guard: SwitchGuard) {
        assert!(self.jobs.is_empty());
        self.switch_guard = guard;
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

/// Sign-out stops running work in every service. All services are attempted; the first
/// failure to record a cancellation is returned afterwards.
pub fn cancel_account(
    generation: &Mutex<GenerationService>,
    identity: &Mutex<IdentityService>,
    exports: &Mutex<ExportService>,
    account_id: Uuid,
) -> Result<(), String> {
    let results = [
        generation
            .lock()
            .map_err(|_| "AI settings are unavailable.".to_owned())
            .and_then(|mut service| service.cancel_account(account_id)),
        identity
            .lock()
            .map_err(|_| "Identity analysis is unavailable.".to_owned())
            .and_then(|mut service| service.cancel_account(account_id)),
        exports
            .lock()
            .map_err(|_| "Exports are unavailable.".to_owned())
            .map(|mut service| service.cancel_account(account_id)),
    ];
    results.into_iter().collect()
}
