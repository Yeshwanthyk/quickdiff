use super::super::worker::{
    spawn_diff_worker, spawn_pr_worker, DiffLoadRequest, DiffWorker, PrWorker,
};
use crate::core::{PullRequest, RepoRoot, RepoWatcher};

pub(super) struct WorkerState {
    pub(super) diff: DiffWorker,
    pub(super) next_request_id: u64,
    pub(super) pending_request_id: Option<u64>,
    pub(super) queued_request: Option<DiffLoadRequest>,
    pub(super) loading: bool,
    pub(super) pr_worker: PrWorker,
    pub(super) next_pr_request_id: u64,
    pub(super) pending_pr_list_id: Option<u64>,
    pub(super) pending_pr_load_id: Option<u64>,
    pub(super) pending_pr: Option<PullRequest>,
    pub(super) watcher: Option<RepoWatcher>,
}

impl WorkerState {
    pub(super) fn new(repo: &RepoRoot) -> Self {
        Self {
            diff: spawn_diff_worker(repo.clone()),
            next_request_id: 1,
            pending_request_id: None,
            queued_request: None,
            loading: false,
            pr_worker: spawn_pr_worker(repo.clone()),
            next_pr_request_id: 1,
            pending_pr_list_id: None,
            pending_pr_load_id: None,
            pending_pr: None,
            watcher: None,
        }
    }
}
