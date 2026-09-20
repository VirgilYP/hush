//! Console mode guard for native Windows terminals, including OpenSSH ConPTY.
use std::io;
use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::System::Console::*;

pub struct TtyGuard {
    input: HANDLE,
    output: HANDLE,
    input_mode: u32,
    output_mode: u32,
}
impl TtyGuard {
    pub fn enter_raw() -> io::Result<Self> {
        unsafe {
            let input = GetStdHandle(STD_INPUT_HANDLE);
            let output = GetStdHandle(STD_OUTPUT_HANDLE);
            let mut input_mode = 0;
            let mut output_mode = 0;
            if GetConsoleMode(input, &mut input_mode) == 0
                || GetConsoleMode(output, &mut output_mode) == 0
            {
                return Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "interactive hush requires a Windows console (use ssh -t or Windows Terminal)",
                ));
            }
            let guard = Self {
                input,
                output,
                input_mode,
                output_mode,
            };
            let raw = (input_mode
                & !(ENABLE_ECHO_INPUT
                    | ENABLE_LINE_INPUT
                    | ENABLE_PROCESSED_INPUT
                    | ENABLE_QUICK_EDIT_MODE))
                | ENABLE_EXTENDED_FLAGS
                | ENABLE_VIRTUAL_TERMINAL_INPUT;
            if SetConsoleMode(input, raw) == 0
                || SetConsoleMode(
                    output,
                    output_mode | ENABLE_PROCESSED_OUTPUT | ENABLE_VIRTUAL_TERMINAL_PROCESSING,
                ) == 0
            {
                return Err(io::Error::last_os_error());
            }
            Ok(guard)
        }
    }
}
impl Drop for TtyGuard {
    fn drop(&mut self) {
        unsafe {
            SetConsoleMode(self.input, self.input_mode);
            SetConsoleMode(self.output, self.output_mode);
        }
    }
}
