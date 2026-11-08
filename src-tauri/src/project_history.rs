//! Renderer-facing history and save-conflict values, independent of database records.
use desegnos_storage::project_db as db;
use serde::Serialize;
use uuid::Uuid;

#[derive(ts_rs::TS, Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileStamp {
    pub project_id: Uuid,
    pub version: i64,
}

#[derive(ts_rs::TS, Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SaveFailureKind {
    Changed,
    DifferentProject,
    Unreadable,
    InvalidProject,
    Io,
}

#[derive(ts_rs::TS, Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionSummary {
    pub id: i64,
    pub parent: Option<i64>,
    pub label: String,
    pub created_at: i64,
    /// This version is where the branch currently is.
    pub head: bool,
}

#[derive(ts_rs::TS, Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BranchSummary {
    pub name: String,
    pub head: i64,
    pub current: bool,
    pub primary: bool,
}

#[derive(ts_rs::TS, Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectHistoryView {
    pub branch: String,
    pub primary_branch: String,
    pub branches: Vec<BranchSummary>,
    pub versions: Vec<VersionSummary>,
}

#[derive(ts_rs::TS, Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompactReport {
    pub removed_versions: usize,
    pub removed_blobs: usize,
}

impl From<db::FileStamp> for FileStamp {
    fn from(value: db::FileStamp) -> Self {
        let db::FileStamp {
            project_id,
            version,
        } = value;
        Self {
            project_id,
            version,
        }
    }
}

impl From<db::VersionSummary> for VersionSummary {
    fn from(value: db::VersionSummary) -> Self {
        let db::VersionSummary {
            id,
            parent,
            label,
            created_at,
            head,
        } = value;
        Self {
            id,
            parent,
            label,
            created_at,
            head,
        }
    }
}

impl From<db::BranchSummary> for BranchSummary {
    fn from(value: db::BranchSummary) -> Self {
        let db::BranchSummary {
            name,
            head,
            current,
            primary,
        } = value;
        Self {
            name,
            head,
            current,
            primary,
        }
    }
}

impl From<db::CompactReport> for CompactReport {
    fn from(value: db::CompactReport) -> Self {
        let db::CompactReport {
            removed_versions,
            removed_blobs,
        } = value;
        Self {
            removed_versions,
            removed_blobs,
        }
    }
}

impl From<db::SaveFailureKind> for SaveFailureKind {
    fn from(value: db::SaveFailureKind) -> Self {
        match value {
            db::SaveFailureKind::Changed => Self::Changed,
            db::SaveFailureKind::DifferentProject => Self::DifferentProject,
            db::SaveFailureKind::Unreadable => Self::Unreadable,
            db::SaveFailureKind::InvalidProject => Self::InvalidProject,
            db::SaveFailureKind::Io => Self::Io,
        }
    }
}
impl From<db::ProjectHistoryView> for ProjectHistoryView {
    fn from(value: db::ProjectHistoryView) -> Self {
        let db::ProjectHistoryView {
            branch,
            primary_branch,
            branches,
            versions,
        } = value;
        Self {
            branch,
            primary_branch,
            branches: branches.into_iter().map(Into::into).collect(),
            versions: versions.into_iter().map(Into::into).collect(),
        }
    }
}
