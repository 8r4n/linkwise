//! Stop events for foreground commands.
use std::io;

pub async fn stop() -> io::Result<i32> {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        let mut interrupt = signal(SignalKind::interrupt())?;
        let mut terminate = signal(SignalKind::terminate())?;
        tokio::select! {
            _ = interrupt.recv() => Ok(130),
            _ = terminate.recv() => Ok(143),
        }
    }
    #[cfg(windows)]
    {
        let mut interrupt = tokio::signal::windows::ctrl_c()?;
        let mut control_break = tokio::signal::windows::ctrl_break()?;
        tokio::select! {
            _ = interrupt.recv() => Ok(130),
            _ = control_break.recv() => Ok(130),
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        tokio::signal::ctrl_c().await?;
        Ok(130)
    }
}
