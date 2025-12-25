//! Background worker for loading file content and computing diffs.

use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use crate::core::{load_diff_contents, ChangedFile, DiffResult, DiffSource, RepoRoot, TextBuffer};

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

#[derive(Debug)]
pub(crate) struct DiffWorker {
    pub request_tx: Sender<DiffLoadRequest>,
    pub response_rx: Receiver<DiffLoadResponse>,
}

pub(crate) fn spawn_diff_worker(repo: RepoRoot) -> DiffWorker {
    let (request_tx, request_rx) = mpsc::channel::<DiffLoadRequest>();
    let (response_tx, response_rx) = mpsc::channel::<DiffLoadResponse>();

    thread::spawn(move || worker_loop(repo, request_rx, response_tx));

    DiffWorker {
        request_tx,
        response_rx,
    }
}

fn worker_loop(
    repo: RepoRoot,
    request_rx: Receiver<DiffLoadRequest>,
    response_tx: Sender<DiffLoadResponse>,
) {
    while let Ok(mut req) = request_rx.recv() {
        // Drain queued requests so we always work on the latest selection.
        while let Ok(next) = request_rx.try_recv() {
            req = next;
        }

        let response = compute_diff_payload(&repo, req);
        let _ = response_tx.send(response);
    }
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
