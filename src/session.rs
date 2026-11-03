use crate::error::AppError;
use std::sync::Arc;
use thirtyfour::WebDriver;
use tokio::sync::Mutex;

#[derive(Clone, Default)]
pub struct SessionSlot(Arc<Mutex<Option<WebDriver>>>);

impl SessionSlot {
    pub fn new() -> Self {
        Self(Arc::new(Mutex::new(None)))
    }

    /// Returns the driver back on failure so the caller can quit() it —
    /// avoids relying on drop-to-kill semantics for cleanup.
    pub async fn start(&self, driver: WebDriver) -> Result<(), (AppError, WebDriver)> {
        let mut guard = self.0.lock().await;
        if guard.is_some() {
            return Err((AppError::SessionAlreadyActive, driver));
        }
        *guard = Some(driver);
        Ok(())
    }

    pub async fn end(&self) -> Result<(), AppError> {
        let driver = self
            .0
            .lock()
            .await
            .take()
            .ok_or(AppError::NoActiveSession)?;
        driver.quit().await?;
        Ok(())
    }

    /// Holds the lock for the duration of `f` — this serializes every tool
    /// call against the one active session instead of racing session_end
    /// against an in-flight action.
    pub async fn with_driver<F, Fut, T>(&self, f: F) -> Result<T, AppError>
    where
        F: FnOnce(WebDriver) -> Fut,
        Fut: std::future::Future<Output = Result<T, AppError>>,
    {
        let guard = self.0.lock().await;
        let driver = guard.clone().ok_or(AppError::NoActiveSession)?;
        f(driver).await
    }
}
