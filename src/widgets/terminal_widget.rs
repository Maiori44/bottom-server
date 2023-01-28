use crate::BottomEvent;
use serde::__private::from_utf8_lossy;
use std::{collections::VecDeque, sync::mpsc::Sender};
use strip_ansi_escapes::strip;

pub struct TerminalWidgetState {
    pub stdout: String,
    pub stdin: VecDeque<String>,
    pub offset: usize,
    pub input_offset: usize,
    pub selected_input: usize,
    pub is_working: bool,
}

impl Default for TerminalWidgetState {
    fn default() -> Self {
        Self {
            stdout: String::new(),
            stdin: VecDeque::from([String::new()]),
            offset: 0,
            input_offset: 0,
            selected_input: 0,
            is_working: false,
        }
    }
}

impl TerminalWidgetState {
    pub fn current_input(&self) -> &String {
        self.stdin.get(self.selected_input).unwrap()
    }

    pub fn current_input_mut(&mut self) -> &mut String {
        self.stdin.get_mut(self.selected_input).unwrap()
    }
}

pub struct UnsafeTerminalWidgetState {
    pub terminal: *mut TerminalWidgetState,
    pub sender: *const Sender<BottomEvent>,
}

impl UnsafeTerminalWidgetState {
    pub fn stdin(&mut self) -> String {
        unsafe {
            let t = &mut (*self.terminal);
            let stdin = t.current_input().clone();
            if !stdin.is_empty() {
                if t.selected_input > 0 {
                    t.stdin.pop_front();
                    t.stdin.push_front(stdin.clone());
                }
                t.stdin.push_front(String::new());
                while t.stdin.len() > 500 {
                    t.stdin.pop_back();
                }
            }
            t.selected_input = 0;
            let trimmed = stdin.trim();
            if !trimmed.is_empty() {
                (*self.terminal).stdout += &format!("$ {trimmed}\n");
            }
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
            if stdout.len() > 100000 {
                let mut chars = stdout.chars();
                for _ in 0..stdout.len() - 100000 {
                    chars.next();
                }
                (*self.terminal).stdout = chars.collect();
            }
        }
    }

    pub fn finish(&mut self) {
        unsafe {
            (*self.terminal).is_working = false;
            (*self.sender).send(BottomEvent::Resize).unwrap_unchecked();
        }
    }
}

unsafe impl Sync for UnsafeTerminalWidgetState {}
unsafe impl Send for UnsafeTerminalWidgetState {}
