//! Background packfile writer for [`MemOdb`](crate::mem_odb::MemOdb).
//!
//! Writing a packfile (zlib-compressing every buffered object and fsyncing the result) is the
//! expensive tail of a flush. Running it inline would block the filter hot path — the mid-run
//! overflow flush most of all, which fires repeatedly during a large rewrite. So all packing is
//! funnelled to one process-global worker thread:
//!
//! * [`enqueue_chunk`] hands the worker a store to pack and returns immediately. It is best-effort
//!   (fire-and-forget), used from the write path when a store overflows its size limit.
//! * [`drain`] hands the worker a store and blocks until it is packed and evicted, used at
//!   transaction and external-git boundaries where the objects must be durable on disk before the
//!   caller proceeds.
//!
//! A single worker processes jobs FIFO, so a store's queued overflow chunks always complete before
//! its drain — and no two packbuilders ever run against the same store concurrently. Stores are
//! per-operation, so a job carries the store's `Arc` and its repo path (a `git2::Repository` is not
//! `Send`); the worker opens its own repository handle to run the packbuilder.

use std::sync::Arc;
use std::sync::LazyLock;
use std::sync::mpsc::{Sender, SyncSender, channel, sync_channel};

use crate::mem_odb::MemOdb;

/// A unit of packing work for the background worker. Each carries the store's `Arc` (the packbuilder
/// reads objects back out of it) rather than a repository handle, which is not `Send`.
enum Job {
    /// Pack the store's currently-buffered objects and evict them. Best-effort; no acknowledgement.
    Chunk { store: Arc<MemOdb> },
    /// Pack and evict, then acknowledge, so a boundary caller can block until the objects are on
    /// disk.
    Drain {
        store: Arc<MemOdb>,
        ack: SyncSender<Result<(), String>>,
    },
}

struct Flusher {
    sender: Sender<Job>,
}

/// The process-global worker, spawned on first use.
static FLUSHER: LazyLock<Flusher> = LazyLock::new(Flusher::spawn);

impl Flusher {
    fn spawn() -> Flusher {
        let (sender, receiver) = channel::<Job>();
        std::thread::Builder::new()
            .name("josh-mem-odb-flusher".to_string())
            .spawn(move || {
                while let Ok(job) = receiver.recv() {
                    match job {
                        Job::Chunk { store } => {
                            if let Err(e) = store.pack_to_disk() {
                                log::error!("background chunk flush failed: {e}");
                            }
                            // Cleared only now (after packing + eviction), so the write path does
                            // not enqueue a fresh chunk until this store's size reflects the drain.
                            store.clear_chunk_in_flight();
                        }
                        Job::Drain { store, ack } => {
                            let _ = ack.send(store.pack_to_disk().map_err(|e| e.to_string()));
                        }
                    }
                }
            })
            .expect("failed to spawn josh-mem-odb-flusher thread");
        Flusher { sender }
    }
}

/// Enqueue a best-effort background pack of `store`. Returns immediately; if the worker is gone the
/// request is dropped (the next overflow, or the drain at drop, retries).
pub(crate) fn enqueue_chunk(store: Arc<MemOdb>) {
    let _ = FLUSHER.sender.send(Job::Chunk { store });
}

/// Pack `store` to disk and block until it is done, so the objects are durable before the caller
/// proceeds. Any queued chunks for the same store complete first (FIFO on the single worker).
pub(crate) fn drain(store: Arc<MemOdb>) -> Result<(), git2::Error> {
    let (ack_tx, ack_rx) = sync_channel::<Result<(), String>>(1);
    if FLUSHER
        .sender
        .send(Job::Drain { store, ack: ack_tx })
        .is_err()
    {
        return Err(git2::Error::from_str(
            "mem-odb flusher channel disconnected",
        ));
    }
    match ack_rx.recv() {
        Ok(Ok(())) => Ok(()),
        Ok(Err(msg)) => Err(git2::Error::from_str(&msg)),
        Err(_) => Err(git2::Error::from_str(
            "mem-odb flusher ack channel disconnected",
        )),
    }
}
