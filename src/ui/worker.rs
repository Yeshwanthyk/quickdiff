//! Background worker for loading file content and computing diffs.

use std::sync::mpsc::{self, Receiver, Sender, SyncSender};
use std::thread::{self, JoinHandle};

use crate::core::{
    get_pr_diff, list_prs, load_diff_contents, ChangedFile, DiffResult, DiffSource, PRFilter,
    PullRequest, RepoRoot, TextBuffer,
};

#[derive(Debug, Clone)]
pub(crate) struct DiffLoadRequest {
    pub id: u64,
    pub source: DiffSource,
    pub cached_merge_base: Option<String>,
    pub file: ChangedFile,
}

#[derive(Debug)]
pub(crate) enum DiffLoadResponse {
    Loaded {
        id: u64,
        old_buffer: TextBuffer,
        new_buffer: TextBuffer,
        diff: Option<DiffResult>,
        is_binary: bool,
    },
    Error {
        id: u64,
        message: String,
    },
}

pub(crate) struct DiffWorker {
    /// Wrapped in Option so we can drop it before joining the thread.
    pub request_tx: Option<SyncSender<DiffLoadRequest>>,
    pub response_rx: Receiver<DiffLoadResponse>,
    handle: Option<JoinHandle<()>>,
}

impl std::fmt::Debug for DiffWorker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DiffWorker")
            .field("request_tx", &self.request_tx)
            .field("response_rx", &self.response_rx)
            .field("handle", &self.handle.as_ref().map(|_| "..."))
            .finish()
    }
}

pub(crate) fn spawn_diff_worker(repo: RepoRoot) -> DiffWorker {
    // Bounded channel: only latest request matters (worker drains queue anyway)
    let (request_tx, request_rx) = mpsc::sync_channel::<DiffLoadRequest>(1);
    let (response_tx, response_rx) = mpsc::channel::<DiffLoadResponse>();

    let handle = thread::spawn(move || worker_loop(repo, request_rx, response_tx));

    DiffWorker {
        request_tx: Some(request_tx),
        response_rx,
        handle: Some(handle),
    }
}

impl Drop for DiffWorker {
    fn drop(&mut self) {
        // Drop the sender first to close the channel and unblock recv()
        drop(self.request_tx.take());
        // Now the worker thread will exit, so we can join it
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn worker_loop(
    repo: RepoRoot,
    request_rx: Receiver<DiffLoadRequest>,
    response_tx: Sender<DiffLoadResponse>,
) {
    while let Ok(req) = request_rx.recv() {
        let req = drain_latest_request(req, &request_rx);

        let id = req.id;
        let response = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            compute_diff_payload(&repo, req)
        })) {
            Ok(resp) => resp,
            Err(panic) => {
                // Extract panic message if possible
                let message = if let Some(s) = panic.downcast_ref::<&str>() {
                    format!("worker panic: {}", s)
                } else if let Some(s) = panic.downcast_ref::<String>() {
                    format!("worker panic: {}", s)
                } else {
                    "worker panic: unknown error".to_string()
                };
                DiffLoadResponse::Error { id, message }
            }
        };
        let _ = response_tx.send(response);
    }
}

fn drain_latest_request(
    mut req: DiffLoadRequest,
    request_rx: &Receiver<DiffLoadRequest>,
) -> DiffLoadRequest {
    while let Ok(next) = request_rx.try_recv() {
        req = next;
    }
    req
}

fn compute_diff_payload(repo: &RepoRoot, req: DiffLoadRequest) -> DiffLoadResponse {
    let DiffLoadRequest {
        id,
        source,
        cached_merge_base,
        file,
    } = req;

    let (old_bytes, new_bytes) =
        match load_diff_contents(repo, &source, &file, cached_merge_base.as_deref()) {
            Ok(pair) => pair,
            Err(e) => {
                return DiffLoadResponse::Error {
                    id,
                    message: e.to_string(),
                };
            }
        };

    let old_buffer = TextBuffer::new(&old_bytes);
    let new_buffer = TextBuffer::new(&new_bytes);

    let is_binary = old_buffer.is_binary() || new_buffer.is_binary();
    let diff = if is_binary {
        None
    } else {
        Some(DiffResult::compute(&old_buffer, &new_buffer))
    };

    DiffLoadResponse::Loaded {
        id,
        old_buffer,
        new_buffer,
        diff,
        is_binary,
    }
}

#[derive(Debug, Clone)]
pub(crate) enum PrRequest {
    List { id: u64, filter: PRFilter },
    LoadDiff { id: u64, pr_number: u32 },
}

#[derive(Debug)]
pub(crate) enum PrResponse {
    List { id: u64, prs: Vec<PullRequest> },
    ListError { id: u64, message: String },
    Diff { id: u64, diff: String },
    DiffError { id: u64, message: String },
}

pub(crate) struct PrWorker {
    /// Wrapped in Option so we can drop it before joining the thread.
    pub request_tx: Option<Sender<PrRequest>>,
    pub response_rx: Receiver<PrResponse>,
    handle: Option<JoinHandle<()>>,
}

impl std::fmt::Debug for PrWorker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PrWorker")
            .field("request_tx", &self.request_tx)
            .field("response_rx", &self.response_rx)
            .field("handle", &self.handle.as_ref().map(|_| "..."))
            .finish()
    }
}

pub(crate) fn spawn_pr_worker(repo: RepoRoot) -> PrWorker {
    let (request_tx, request_rx) = mpsc::channel::<PrRequest>();
    let (response_tx, response_rx) = mpsc::channel::<PrResponse>();

    let handle = thread::spawn(move || pr_worker_loop(repo, request_rx, response_tx));

    PrWorker {
        request_tx: Some(request_tx),
        response_rx,
        handle: Some(handle),
    }
}

impl Drop for PrWorker {
    fn drop(&mut self) {
        drop(self.request_tx.take());
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn pr_worker_loop(
    repo: RepoRoot,
    request_rx: Receiver<PrRequest>,
    response_tx: Sender<PrResponse>,
) {
    while let Ok(req) = request_rx.recv() {
        let (id, is_list) = match &req {
            PrRequest::List { id, .. } => (*id, true),
            PrRequest::LoadDiff { id, .. } => (*id, false),
        };

        let response = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            compute_pr_response(&repo, req)
        })) {
            Ok(resp) => resp,
            Err(panic) => {
                let message = if let Some(s) = panic.downcast_ref::<&str>() {
                    format!("worker panic: {}", s)
                } else if let Some(s) = panic.downcast_ref::<String>() {
                    format!("worker panic: {}", s)
                } else {
                    "worker panic: unknown error".to_string()
                };
                if is_list {
                    PrResponse::ListError { id, message }
                } else {
                    PrResponse::DiffError { id, message }
                }
            }
        };
        let _ = response_tx.send(response);
    }
}

fn compute_pr_response(repo: &RepoRoot, req: PrRequest) -> PrResponse {
    match req {
        PrRequest::List { id, filter } => match list_prs(repo.path(), filter) {
            Ok(prs) => PrResponse::List { id, prs },
            Err(e) => PrResponse::ListError {
                id,
                message: e.to_string(),
            },
        },
        PrRequest::LoadDiff { id, pr_number } => match get_pr_diff(repo.path(), pr_number) {
            Ok(diff) => PrResponse::Diff { id, diff },
            Err(e) => PrResponse::DiffError {
                id,
                message: e.to_string(),
            },
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    fn make_request(id: u64) -> DiffLoadRequest {
        DiffLoadRequest {
            id,
            source: DiffSource::WorkingTree,
            cached_merge_base: None,
            file: ChangedFile::new(
                crate::core::RelPath::new("src/main.rs"),
                crate::core::FileChangeKind::Modified,
            ),
        }
    }

    #[test]
    fn drain_latest_request_coalesces() {
        let (tx, rx) = mpsc::channel();

        tx.send(make_request(1)).unwrap();
        tx.send(make_request(2)).unwrap();
        tx.send(make_request(3)).unwrap();

        let first = rx.recv().unwrap();
        let latest = drain_latest_request(first, &rx);

        assert_eq!(latest.id, 3);
    }
}
