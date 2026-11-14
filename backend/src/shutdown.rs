use std::sync::{Arc, OnceLock};
use tokio::sync::watch;

/// Единый токен отмены для координированной остановки фоновых задач (graceful shutdown).
#[derive(Clone, Debug)]
pub struct CancellationToken {
    tx: Arc<watch::Sender<bool>>,
    rx: watch::Receiver<bool>,
}

impl Default for CancellationToken {
    fn default() -> Self {
        Self::new()
    }
}

impl CancellationToken {
    pub fn new() -> Self {
        let (tx, rx) = watch::channel(false);
        Self {
            tx: Arc::new(tx),
            rx,
        }
    }

    /// Послать сигнал остановки всем держателям клонированного токена.
    pub fn cancel(&self) {
        let _ = self.tx.send(true);
    }

    /// Проверить без ожидания, был ли уже подан сигнал остановки.
    pub fn is_cancelled(&self) -> bool {
        *self.rx.borrow()
    }

    /// Асинхронно дождаться отмены (возвращается мгновенно, если уже отменён).
    pub async fn cancelled(&self) {
        let mut rx = self.rx.clone();
        if *rx.borrow_and_update() {
            return;
        }
        while rx.changed().await.is_ok() {
            if *rx.borrow() {
                return;
            }
        }
    }
}

static GLOBAL_SHUTDOWN: OnceLock<CancellationToken> = OnceLock::new();

/// Глобальный токен остановки процесса XKeen Route.
pub fn global() -> &'static CancellationToken {
    GLOBAL_SHUTDOWN.get_or_init(CancellationToken::new)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn cancellation_token_notifies_all_clones_immediately() {
        let root = CancellationToken::new();
        let child_a = root.clone();
        let child_b = root.clone();

        assert!(!root.is_cancelled());
        assert!(!child_a.is_cancelled());

        let h_a = tokio::spawn(async move {
            child_a.cancelled().await;
            true
        });
        let h_b = tokio::spawn(async move {
            tokio::select! {
                _ = child_b.cancelled() => true,
                _ = tokio::time::sleep(std::time::Duration::from_secs(5)) => false,
            }
        });

        tokio::task::yield_now().await;
        root.cancel();

        assert!(root.is_cancelled());
        assert!(h_a.await.unwrap());
        assert!(h_b.await.unwrap());

        // Повторный вызов cancelled() после отмены возвращается мгновенно
        tokio::time::timeout(std::time::Duration::from_millis(50), root.cancelled())
            .await
            .expect("already-cancelled token must resolve immediately");
    }
}
