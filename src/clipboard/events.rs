use super::ClipboardEvent;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, SendError, SyncSender, TryRecvError};
use std::sync::{Arc, Mutex};

const CAPTURE_QUEUE_CAPACITY: usize = 3;

#[derive(Default)]
struct Notifications {
    skipped: Mutex<Option<&'static str>>,
    stopped: AtomicBool,
}

#[derive(Clone)]
pub struct ClipboardEventSender {
    tx: SyncSender<ClipboardEvent>,
    notifications: Arc<Notifications>,
}

pub struct ClipboardEvents {
    rx: Receiver<ClipboardEvent>,
    notifications: Arc<Notifications>,
}

pub fn channel() -> (ClipboardEventSender, ClipboardEvents) {
    let (tx, rx) = mpsc::sync_channel(CAPTURE_QUEUE_CAPACITY);
    let notifications = Arc::new(Notifications::default());
    (
        ClipboardEventSender {
            tx,
            notifications: notifications.clone(),
        },
        ClipboardEvents { rx, notifications },
    )
}

impl ClipboardEventSender {
    pub fn send(&self, event: ClipboardEvent) -> Result<(), SendError<ClipboardEvent>> {
        match event {
            ClipboardEvent::Skipped(reason) => {
                // Coalesce status messages without blocking the OS watcher when
                // the UI is busy. The latest reason survives a full data queue.
                if let Ok(mut pending) = self.notifications.skipped.lock() {
                    *pending = Some(reason);
                }
                Ok(())
            }
            ClipboardEvent::WatcherStopped => {
                self.notifications.stopped.store(true, Ordering::Release);
                Ok(())
            }
            // Only the encoder worker sends these. Backpressure bounds completed
            // images as well as raw snapshots, including while a file dialog is open.
            event => self.tx.send(event),
        }
    }
}

impl ClipboardEvents {
    pub fn try_recv(&self) -> Result<ClipboardEvent, TryRecvError> {
        match self.rx.try_recv() {
            Ok(event) => Ok(event),
            Err(error) => {
                if self.notifications.stopped.swap(false, Ordering::AcqRel) {
                    return Ok(ClipboardEvent::WatcherStopped);
                }
                if let Ok(mut pending) = self.notifications.skipped.lock()
                    && let Some(reason) = pending.take()
                {
                    return Ok(ClipboardEvent::Skipped(reason));
                }
                Err(error)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn completed_capture_queue_applies_backpressure() {
        let (tx, rx) = channel();
        for generation in 0..CAPTURE_QUEUE_CAPACITY {
            tx.send(ClipboardEvent::ConsumedIgnoreNext(generation as u64))
                .unwrap_or_else(|_| panic!("receiver closed"));
        }
        let (started_tx, started_rx) = mpsc::channel();
        let (finished_tx, finished_rx) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            started_tx.send(()).unwrap();
            tx.send(ClipboardEvent::ConsumedIgnoreNext(99))
                .unwrap_or_else(|_| panic!("receiver closed"));
            finished_tx.send(()).unwrap();
        });
        started_rx.recv().unwrap();
        assert!(finished_rx.recv_timeout(Duration::from_millis(25)).is_err());
        assert!(matches!(
            rx.try_recv(),
            Ok(ClipboardEvent::ConsumedIgnoreNext(0))
        ));
        finished_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        worker.join().unwrap();
    }

    #[test]
    fn full_queue_preserves_coalesced_status_and_watcher_exit() {
        let (tx, rx) = channel();
        for generation in 0..CAPTURE_QUEUE_CAPACITY {
            tx.send(ClipboardEvent::ConsumedIgnoreNext(generation as u64))
                .unwrap_or_else(|_| panic!("receiver closed"));
        }
        tx.send(ClipboardEvent::Skipped("earlier"))
            .unwrap_or_else(|_| panic!("receiver closed"));
        tx.send(ClipboardEvent::Skipped("latest"))
            .unwrap_or_else(|_| panic!("receiver closed"));
        tx.send(ClipboardEvent::WatcherStopped)
            .unwrap_or_else(|_| panic!("receiver closed"));
        for _ in 0..CAPTURE_QUEUE_CAPACITY {
            assert!(matches!(
                rx.try_recv(),
                Ok(ClipboardEvent::ConsumedIgnoreNext(_))
            ));
        }
        assert!(matches!(rx.try_recv(), Ok(ClipboardEvent::WatcherStopped)));
        assert!(matches!(
            rx.try_recv(),
            Ok(ClipboardEvent::Skipped("latest"))
        ));
        assert!(matches!(rx.try_recv(), Err(TryRecvError::Empty)));
    }

    #[test]
    fn closing_receiver_unblocks_encoder_worker() {
        let (tx, rx) = channel();
        for _ in 0..CAPTURE_QUEUE_CAPACITY {
            tx.send(ClipboardEvent::ConsumedIgnoreNext(0))
                .unwrap_or_else(|_| panic!("receiver closed"));
        }
        let worker =
            std::thread::spawn(move || tx.send(ClipboardEvent::ConsumedIgnoreNext(1)).is_err());
        drop(rx);
        assert!(worker.join().unwrap());
    }
}
