use serde::__private::from_utf8_lossy;
use strip_ansi_escapes::strip;

#[derive(Default)]
pub struct TerminalWidgetState {
    pub stdout: String,
    pub stdin: String,
    pub offset: usize,
    pub input_offset: usize,
    pub is_elaborating: bool,
}

pub struct UnsafeTerminalWidgetState {
    pub terminal: *mut TerminalWidgetState,
}

impl UnsafeTerminalWidgetState {
    pub fn stdin(&mut self) -> String {
        unsafe {
            let stdin = (*self.terminal).stdin.clone();
            (*self.terminal).stdin.clear();
            stdin
        }
    }

    pub fn append_output(&mut self, output: Vec<u8>) {
        unsafe {
            (*self.terminal).stdout += &from_utf8_lossy(&strip(output).unwrap());
        }
    }

    pub fn limit_output(&mut self) {
        unsafe {
            let stdout = &mut (*self.terminal).stdout;
            if stdout.len() > 10000 {
                let mut chars = stdout.chars();
                for _ in 0..stdout.len() - 10000 {
                    chars.next();
                }
                (*self.terminal).stdout = chars.collect();
            }
        }
    }

    pub fn finish(&mut self) {
        unsafe {
            (*self.terminal).is_elaborating = false;
        }
    }
}

unsafe impl Sync for UnsafeTerminalWidgetState {}
unsafe impl Send for UnsafeTerminalWidgetState {}
