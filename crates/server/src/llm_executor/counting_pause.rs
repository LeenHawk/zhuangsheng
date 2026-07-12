pub(crate) struct CountPause {
    armed: std::sync::atomic::AtomicBool,
    pub started: tokio::sync::Notify,
    pub release: tokio::sync::Notify,
}

impl CountPause {
    pub fn new() -> Self {
        Self {
            armed: std::sync::atomic::AtomicBool::new(true),
            started: tokio::sync::Notify::new(),
            release: tokio::sync::Notify::new(),
        }
    }

    pub async fn wait_once(&self) {
        if self.armed.swap(false, std::sync::atomic::Ordering::SeqCst) {
            self.started.notify_one();
            self.release.notified().await;
        }
    }
}
