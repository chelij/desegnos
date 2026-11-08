//! What generation, identity analysis and export share about a paid AI job: which attempt is
//! still the live one, how a provider failure maps to a job outcome, and the notices shown
//! when a job stops. Sign-out cancels every service from here.
use crate::{
    export::ExportService, generation::GenerationService, identity::IdentityService,
};
use desegnos_ai::{
    AuthenticationMethod, CancellationToken, ErrorKind, ProviderError, SubmissionOutcome,
};
use std::{
    collections::BTreeMap,
    sync::{Mutex, MutexGuard, PoisonError},
};
use uuid::Uuid;

/// Lock shared state even if another thread panicked while holding it. A job's outcome must
/// still be recorded, or it would stay running forever with no error.
pub fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Running provider requests, each bound to the attempt that started it.
#[derive(Default)]
pub struct ActiveJobs(BTreeMap<Uuid, (usize, CancellationToken)>);

impl ActiveJobs {
    pub fn start(&mut self, job_id: Uuid, attempt: usize) -> CancellationToken {
        let token = CancellationToken::new();
        self.0.insert(job_id, (attempt, token.clone()));
        token
    }

    /// The attempt is the one registered for this job, whether or not it was cancelled.
    pub fn is_registered(&self, job_id: Uuid, attempt: usize) -> bool {
        self.0.get(&job_id).is_some_and(|(live, _)| *live == attempt)
    }

    /// The attempt is registered and nobody has cancelled it.
    pub fn is_current(&self, job_id: Uuid, attempt: usize) -> bool {
        self.0
            .get(&job_id)
            .is_some_and(|(live, token)| *live == attempt && !token.is_cancelled())
    }

    pub fn is_cancelled(&self, job_id: Uuid) -> bool {
        self.0.get(&job_id).is_some_and(|(_, token)| token.is_cancelled())
    }

    pub fn contains(&self, job_id: Uuid) -> bool {
        self.0.contains_key(&job_id)
    }

    pub fn cancel(&self, job_id: Uuid) {
        if let Some((_, token)) = self.0.get(&job_id) {
            token.cancel();
        }
    }

    pub fn remove(&mut self, job_id: Uuid) {
        self.0.remove(&job_id);
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn any_running(&self) -> bool {
        self.0.values().any(|(_, token)| !token.is_cancelled())
    }
}

/// How a failed provider call ends a job.
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

/// Shown when the user stops a job.
pub fn stopped_notice(authentication: AuthenticationMethod) -> String {
    format!(
        "Stopped. A submitted request {}. Late output cannot be applied.",
        charge_warning(authentication)
    )
}

/// Shown when signing out of the account the job uses stops it.
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

/// Shown for a job that was running when the app stopped. It is never resubmitted.
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
