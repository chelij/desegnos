//! An orderly termination from the operating system (logout, shutdown, `kill`, a closed
//! terminal) keeps unsaved work for recovery and is not reported as a crash next launch.
use std::{io, sync::Mutex};
use tauri::Manager;

/// Keeps every open tab's unsaved work for recovery, then clears this run's crash marker.
fn finish(app: &tauri::AppHandle) {
    if let Some(state) = app.try_state::<Mutex<crate::workspace::Workspace>>() {
        match state.lock() {
            Ok(workspace) => workspace.flush_recovery(),
            Err(error) => eprintln!("Could not keep unsaved work: {error}"),
        }
    }
    crate::crash_report::finish();
}

/// Listens for termination signals on a thread of its own, so a busy UI thread cannot delay
/// it and no work happens inside a signal handler. SIGKILL, power loss and real crashes still
/// leave the marker behind and are reported.
pub fn install(app: tauri::AppHandle) -> io::Result<()> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    // Register before returning so a signal sent right after startup is not missed.
    let wait = {
        let _entered = runtime.enter();
        termination()?
    };
    std::thread::Builder::new()
        .name("orderly-shutdown".into())
        .spawn(move || {
            runtime.block_on(wait);
            finish(&app);
            // Windows gives close/logoff/shutdown handlers only a few seconds, so exit here
            // rather than waiting for the event loop; everything is already flushed.
            std::process::exit(0);
        })?;
    Ok(())
}

#[cfg(unix)]
fn termination() -> io::Result<impl std::future::Future<Output = ()>> {
    use tokio::signal::unix::{SignalKind, signal};
    let mut term = signal(SignalKind::terminate())?;
    let mut interrupt = signal(SignalKind::interrupt())?;
    let mut hangup = signal(SignalKind::hangup())?;
    Ok(async move {
        tokio::select! {
            _ = term.recv() => {}
            _ = interrupt.recv() => {}
            _ = hangup.recv() => {}
        }
    })
}

#[cfg(windows)]
fn termination() -> io::Result<impl std::future::Future<Output = ()>> {
    use tokio::signal::windows;
    let mut interrupt = windows::ctrl_c()?;
    let mut ctrl_break = windows::ctrl_break()?;
    let mut close = windows::ctrl_close()?;
    let mut logoff = windows::ctrl_logoff()?;
    let mut shutdown = windows::ctrl_shutdown()?;
    Ok(async move {
        tokio::select! {
            _ = interrupt.recv() => {}
            _ = ctrl_break.recv() => {}
            _ = close.recv() => {}
            _ = logoff.recv() => {}
            _ = shutdown.recv() => {}
        }
    })
}

#[cfg(not(any(unix, windows)))]
fn termination() -> io::Result<impl std::future::Future<Output = ()>> {
    Ok(std::future::pending())
}
