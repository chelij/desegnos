//! Turns the result of a native file dialog into a command result.

/// A cancelled or failed dialog returns `None`. On Linux builds that use the desktop portal,
/// no portal at all also returns `None`, so that case is reported instead of looking like the
/// user changed their mind. Other platforms and the GTK dialog build pass the result through.
#[cfg(all(target_os = "linux", feature = "portal-dialogs"))]
pub fn picked<T>(picked: Option<T>) -> Result<Option<T>, String> {
    if picked.is_none() && !crate::portal::available() {
        return Err(crate::portal::MESSAGE.to_owned());
    }
    Ok(picked)
}

#[cfg(not(all(target_os = "linux", feature = "portal-dialogs")))]
pub fn picked<T>(picked: Option<T>) -> Result<Option<T>, String> {
    Ok(picked)
}

/// One startup line in the terminal log, so a missing portal is visible without a dialog.
pub fn note_missing_portal() {
    #[cfg(all(target_os = "linux", feature = "portal-dialogs"))]
    if !crate::portal::available() {
        eprintln!("{}", crate::portal::MESSAGE);
    }
}
