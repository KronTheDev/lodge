// Console compatibility — detects pseudo-terminals (Git Bash, mintty, Cygwin)
// and relaunches the process in a real Windows console when necessary.
//
// Crossterm's raw mode and alternate screen rely on the Windows Console API.
// When stdout is a pipe or pseudo-terminal (mintty/Git Bash), those API calls
// succeed silently but have no effect, producing doubled input and a blank screen.
// The fix is to detect the situation and reopen in a proper console window.

/// Returns `true` when stdout is attached to a real Windows console.
///
/// On non-Windows platforms this always returns `true` because crossterm
/// uses ANSI escape sequences everywhere else, which work in any terminal.
pub fn has_real_console() -> bool {
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::Console::{GetConsoleMode, GetStdHandle, STD_OUTPUT_HANDLE};
        unsafe {
            let handle = GetStdHandle(STD_OUTPUT_HANDLE);
            let mut mode = 0u32;
            GetConsoleMode(handle, &mut mode) != 0
        }
    }
    #[cfg(not(windows))]
    {
        true
    }
}

/// Ensures the process is running in a real Windows console.
///
/// Returns `true` if the caller should continue (already in a real console,
/// or relaunch failed and we proceed anyway).
/// Returns `false` if a new console window was successfully spawned and the
/// caller should exit immediately (the work continues in the new window).
///
/// On non-Windows platforms this is a no-op and always returns `true`.
pub fn ensure_console_or_relaunch() -> bool {
    #[cfg(windows)]
    {
        if has_real_console() {
            return true;
        }

        // Not a real console — try to relaunch in one.
        let exe = match std::env::current_exe() {
            Ok(e) => e,
            Err(_) => return true, // can't determine exe path, proceed and hope for the best
        };

        let args: Vec<String> = std::env::args().skip(1).collect();

        // Build the full command: exe + original args
        let exe_str = exe.to_string_lossy().to_string();
        let mut full_cmd: Vec<String> = vec![exe_str.clone()];
        full_cmd.extend(args.clone());

        // Try Windows Terminal first (keeps the aesthetic intact)
        let wt_ok = {
            let mut cmd = std::process::Command::new("wt.exe");
            cmd.arg("--");
            cmd.arg(&exe_str);
            cmd.args(&args);
            cmd.spawn().is_ok()
        };
        if wt_ok {
            return false;
        }

        // Fall back to a new cmd.exe window
        let cmd_ok = {
            let mut cmd = std::process::Command::new("cmd.exe");
            cmd.args(["/c", "start", "cmd.exe", "/k"]);
            cmd.arg(&exe_str);
            cmd.args(&args);
            cmd.spawn().is_ok()
        };
        if cmd_ok {
            return false;
        }

        // All relaunch attempts failed — proceed anyway
        true
    }
    #[cfg(not(windows))]
    {
        true
    }
}
