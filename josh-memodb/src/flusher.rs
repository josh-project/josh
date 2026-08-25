//! Background packfile writer for [`MemOdb`](crate::mem_odb::MemOdb).
//!
//! Writing a packfile (zlib-compressing every buffered object and indexing the result) is the
//! expensive tail of a flush. Running it inline would block the filter hot path — the mid-run
//! overflow flush most of all, which fires repeatedly during a large rewrite. So all packing is
//! funnelled to one process-global worker thread:
//!
//! * [`enqueue_chunk`] hands the worker a store to pack and returns immediately. It is best-effort
//!   (fire-and-forget), used from the write path when a store overflows its size limit.
//! * [`drain`] hands the worker a store and blocks until it is packed and evicted, used at the
//!   boundaries where the objects must be durable on disk before the caller proceeds: an external
//!   `git`, and the end of a transaction that published a ref.
//!
//! A single worker processes jobs FIFO, so a store's queued overflow chunks always complete before
//! its drain — and no two packs are ever written from the same store concurrently. A job carries
//! the store's `Arc`; the worker snapshots the buffered objects and packs them with gix-pack
//! straight into the store's object directory (see [`crate::pack`]) — no repository handle
//! involved.

use std::sync::Arc;
use std::sync::LazyLock;
use std::sync::mpsc::{Sender, SyncSender, channel, sync_channel};

use crate::mem_odb::MemOdb;

/// A unit of packing work for the background worker. Each carries the store's `Arc`; the worker
/// snapshots and packs its buffered objects.
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
                            let packed = match store.pack_to_disk() {
                                Ok(()) => true,
                                Err(e) => {
                                    log::error!("background chunk flush failed: {e}");
                                    false
                                }
                            };
                            store.finish_chunk(packed);
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
pub(crate) fn drain(store: Arc<MemOdb>) -> anyhow::Result<()> {
    let (ack_tx, ack_rx) = sync_channel::<Result<(), String>>(1);
    if FLUSHER
        .sender
        .send(Job::Drain { store, ack: ack_tx })
        .is_err()
    {
        return Err(anyhow::anyhow!("mem-odb flusher channel disconnected"));
    }
    match ack_rx.recv() {
        Ok(Ok(())) => Ok(()),
        Ok(Err(msg)) => Err(anyhow::anyhow!(msg)),
        Err(_) => Err(anyhow::anyhow!("mem-odb flusher ack channel disconnected")),
    }
}
