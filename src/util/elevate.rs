use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::ptr;

use windows::core::PCWSTR;
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOW;

fn to_wide(s: &OsStr) -> Vec<u16> {
    s.encode_wide().chain(std::iter::once(0)).collect()
}

pub fn restart_as_admin() -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let exe_wide = to_wide(exe.as_os_str());
    let verb = to_wide(OsStr::new("runas"));
    let operation = PCWSTR(verb.as_ptr());

    unsafe {
        let result = ShellExecuteW(
            None,
            operation,
            PCWSTR(exe_wide.as_ptr()),
            PCWSTR(ptr::null()),
            PCWSTR(ptr::null()),
            SW_SHOW,
        );

        if (result.0 as isize) <= 32 {
            return Err(format!("无法以管理员身份启动 (code: {})", result.0 as isize));
        }
    }

    std::process::exit(0);
}
