//! Text-only project-local history, derived from frozen job requests and outcomes.
use crate::limits::{MAX_RECORDS_PER_LAYER, REQUEST_HISTORY_CHARS};
use crate::{
    Result, generation::RequestOutcome, generation::RequestStatus, model::Project, model::Rect,
    requirements::RequirementsSnapshot, validation::ensure,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub(crate) const HEADING: &str = "\n## Earlier requests for these layers (context only — the current requests are the authority)\n\n```jsonl\n";
pub(crate) const RETRY_HEADING: &str = "\n## Previous validation rejections for retried layers\n\nThese are app checks, not user rejections. Correct each listed layer while preserving the current scope.\n\n```jsonl\n";
const REJECTION_CHARS: usize = 600;
pub(crate) const RETRY_HISTORY_CHARS: usize = crate::limits::MAX_GENERATION_REQUESTS * 4500;
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EarlierRequest {
    pub project_id: Uuid,
    pub job_id: Uuid,
    pub page_id: Uuid,
    pub element_id: Uuid,
    pub rect: Rect,
    pub requested_at: i64,
    pub request: String,
    pub outcome: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub validation_rejection: Option<String>,
    #[serde(default)]
    pub rejection_kind: RejectionKind,
    pub summary: String,
    /// The complete original scope of the job-wide summary.
    #[serde(default)]
    pub summary_targets: Vec<(Uuid, Uuid)>,
}

#[derive(ts_rs::TS, Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FrozenLayerRequest {
    pub page_id: Uuid,
    pub element_id: Uuid,
    pub rect: Rect,
    pub request: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum RequestHistoryOutcome {
    Applied,
    Drafted,
    Undone,
    RejectedByUser,
    Failed,
}
impl RequestHistoryOutcome {
    fn label(self) -> &'static str {
        match self {
            Self::Applied => "applied",
            Self::Drafted => "drafted",
            Self::Undone => "undone",
            Self::RejectedByUser => "rejected by user",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum RejectionKind {
    OutsideRequest,
    OutsideFrame,
    Pointer,
    Image,
    #[default]
    Other,
}

pub fn earlier_requests(
    snapshot: &RequirementsSnapshot,
    requested_at: i64,
    outcome: RequestHistoryOutcome,
    summary: &str,
    requests: &[RequestOutcome],
) -> Vec<EarlierRequest> {
    let summary_targets = snapshot
        .layer_requests
        .iter()
        .map(|r| (r.page_id, r.element_id))
        .collect::<Vec<_>>();
    snapshot
        .layer_requests
        .iter()
        .map(|frozen| {
            let question = requests.iter().find(|r| {
                r.element_id == Some(frozen.element_id) && r.status == RequestStatus::Conflict
            });
            let outcome = match outcome {
                RequestHistoryOutcome::Applied | RequestHistoryOutcome::Drafted => question
                    .map(|r| format!("needs details: {}", clip(&r.explanation, 300)))
                    .unwrap_or_else(|| outcome.label().into()),
                _ => outcome.label().into(),
            };
            EarlierRequest {
                project_id: snapshot.project_id,
                job_id: snapshot.job_id,
                page_id: frozen.page_id,
                element_id: frozen.element_id,
                rect: frozen.rect,
                requested_at,
                request: clip(&frozen.request, 300),
                outcome,
                validation_rejection: None,
                rejection_kind: RejectionKind::Other,
                summary: clip(
                    &summary.split_whitespace().collect::<Vec<_>>().join(" "),
                    200,
                ),
                summary_targets: summary_targets.clone(),
            }
        })
        .collect()
}

pub fn compact_request_history(records: &mut Vec<EarlierRequest>) {
    records.sort_by_key(|r| {
        (
            std::cmp::Reverse(r.requested_at),
            std::cmp::Reverse(r.job_id),
        )
    });
    let mut counts = std::collections::BTreeMap::new();
    let mut seen = std::collections::BTreeSet::new();
    records.retain(|r| {
        let count = counts
            .entry((r.project_id, r.page_id, r.element_id))
            .or_insert(0);
        if *count >= MAX_RECORDS_PER_LAYER || !seen.insert((r.project_id, r.job_id, r.element_id)) {
            return false;
        }
        *count += 1;
        true
    });
}

fn clip(value: &str, max: usize) -> String {
    let mut text: String = value.chars().take(max).collect();
    if value.chars().count() > max {
        text.pop();
        text.push('…');
    }
    text
}
pub fn record_validation_rejection(
    request: &mut EarlierRequest,
    kind: RejectionKind,
    reason: &str,
) {
    request.rejection_kind = kind;
    request.outcome = "rejected by the app's checks".into();
    request.validation_rejection = Some(clip(reason, REJECTION_CHARS));
}

fn prevention_rule(kind: RejectionKind) -> &'static str {
    match kind {
        RejectionKind::OutsideRequest => {
            "Change only this layer's subtree and the targets its change pointers grant."
        }
        RejectionKind::OutsideFrame => {
            "Keep every new or changed element inside its parent's box, in the parent's coordinates."
        }
        RejectionKind::Pointer => {
            "Classify every pointer with its current comment, and use only the listed Play actions."
        }
        RejectionKind::Image => {
            "Reference generated pictures by their image request keys, and keep every image element in the reply's images list."
        }
        RejectionKind::Other => {
            "Change only the requested layers and their allowed targets, and follow the reply schema exactly."
        }
    }
}

fn nearby(a: Rect, b: Rect) -> bool {
    const GAP: f64 = 32.;
    a.x <= b.x + b.width + GAP
        && b.x <= a.x + a.width + GAP
        && a.y <= b.y + b.height + GAP
        && b.y <= a.y + a.height + GAP
}
fn relative(now: i64, then: i64) -> String {
    let seconds = now.saturating_sub(then).max(0);
    let (n, unit) = if seconds < 60 {
        return "just now".into();
    } else if seconds < 3600 {
        (seconds / 60, "minute")
    } else if seconds < 86400 {
        (seconds / 3600, "hour")
    } else {
        (seconds / 86400, "day")
    };
    format!("{n} {unit}{} ago", if n == 1 { "" } else { "s" })
}
impl RequirementsSnapshot {
    pub fn with_earlier_requests(
        mut self,
        project: &Project,
        records: &[EarlierRequest],
        now: i64,
    ) -> Result<Self> {
        ensure(
            self.project_id == project.id,
            "History belongs to another project.",
        )?;

        let ids = self.target_ids()?;
        let mut targets = Vec::new();
        for (page_id, element_id) in &ids {
            if let Ok(page) = project.page(*page_id)
                && let Some(element) = crate::elements::find_element(&page.elements, *element_id)
            {
                targets.push((page, element));
            }
        }
        let mut records: Vec<_> = records
            .iter()
            .filter(|r| r.project_id == project.id && r.job_id != self.job_id)
            .collect();
        records.sort_by_key(|r| {
            (
                std::cmp::Reverse(r.requested_at),
                std::cmp::Reverse(r.job_id),
                r.element_id,
            )
        });
        // Exact layer identity, never proximity: a nearby layer's failure is not this retry's.
        // Reserve separate bounded space so five older records cannot crowd out a rejection.
        let mut retry = String::new();
        for (page, element) in &targets {
            if let Some(record) = records
                .iter()
                .find(|r| r.page_id == page.id && r.element_id == element.id)
                .filter(|r| r.validation_rejection.is_some())
            {
                let reason = clip(
                    record
                        .validation_rejection
                        .as_deref()
                        .expect("filtered rejection"),
                    REJECTION_CHARS,
                );
                let instruction = format!(
                    "Your previous attempt for this layer was rejected because: {reason} Avoid this by: {}",
                    prevention_rule(record.rejection_kind)
                );
                retry.push_str(&format!(
                    "{}\n",
                    serde_json::json!({"page":page.id,"element":element.id,"instruction":instruction})
                ));
            }
        }
        let retry = if retry.is_empty() {
            retry
        } else {
            format!("{RETRY_HEADING}{retry}```\n")
        };
        ensure(
            retry.chars().count() <= RETRY_HISTORY_CHARS,
            "The retry history exceeds its limit.",
        )?;
        let mut counts = vec![0; targets.len()];
        let mut seen = std::collections::BTreeSet::new();
        let mut section = String::new();
        for r in records {
            let matched: Vec<_> = targets
                .iter()
                .enumerate()
                .filter(|(i, (p, element))| {
                    counts[*i] < MAX_RECORDS_PER_LAYER
                        && r.page_id == p.id
                        && (r.element_id == element.id || nearby(element.rect, r.rect))
                })
                .map(|(i, _)| i)
                .collect();
            if matched.is_empty() || !seen.insert((r.job_id, r.element_id)) {
                continue;
            }
            // JSON quoting keeps exact user text as data, including line breaks and fences.
            // A per-layer record still carries a job-wide summary. Never expose it to a
            // narrower request or one whose summary scope is not established.
            let summary = (!r.summary_targets.is_empty()
                && r.summary_targets.iter().all(|target| ids.contains(target)))
            .then(|| clip(&r.summary, 200));
            let entry = serde_json::json!({"page":r.page_id,"element":r.element_id,"when":relative(now,r.requested_at),
                "request":clip(&r.request,300),"outcome":clip(&r.outcome,320),"summary":summary,
                "validationReason":r.validation_rejection.as_deref().map(|reason| clip(reason, REJECTION_CHARS))});
            let line = format!("{}\n", entry);
            if HEADING.chars().count() + section.chars().count() + line.chars().count() + 4
                > REQUEST_HISTORY_CHARS
            {
                break;
            }
            section.push_str(&line);
            for i in matched {
                counts[i] += 1;
            }
        }
        self.set_request_history(if section.is_empty() {
            retry
        } else {
            format!("{retry}{HEADING}{section}```\n")
        })?;
        Ok(self)
    }
}
