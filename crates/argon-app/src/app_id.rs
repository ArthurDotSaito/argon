pub const APP_USER_MODEL_ID: &str = "br.com.nuria.argon";

#[cfg(windows)]
pub fn register_process() {
    use windows_sys::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID;

    let wide: Vec<u16> = APP_USER_MODEL_ID
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let result = unsafe { SetCurrentProcessExplicitAppUserModelID(wide.as_ptr()) };
    if result < 0 {
        tracing::warn!(result, "failed to set app user model id");
    }
}

#[cfg(not(windows))]
pub fn register_process() {}
