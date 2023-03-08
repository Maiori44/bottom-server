use crate::{app::App, BottomEvent};
use serde::__private::from_utf8_lossy;
use std::{
    collections::VecDeque,
    sync::{mpsc::Sender, Mutex, MutexGuard},
};
use strip_ansi_escapes::strip;

pub struct TerminalWidgetState {
    pub stdout: String,
    pub stdin: VecDeque<String>,
    pub offset: usize,
    pub input_offset: usize,
    pub selected_input: usize,
    pub is_working: bool,
    pub sender: Option<*const Sender<BottomEvent>>,
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
            sender: None,
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

unsafe impl Sync for TerminalWidgetState {}
unsafe impl Send for TerminalWidgetState {}

pub struct UnsafeTerminalWidgetState {
    pub id: u64,
    pub app: &'static Mutex<Option<App>>,
    pub sender: *const Sender<BottomEvent>,
}

impl UnsafeTerminalWidgetState {
    fn lock(&self) -> MutexGuard<'_, Option<App>> {
        self.app.lock().unwrap()
    }

    fn get_tws<'a>(
        &self, app_lock: &'a mut MutexGuard<'_, Option<App>>,
    ) -> &'a mut TerminalWidgetState {
        let app = app_lock.as_mut().unwrap();
        app.terminal_state
            .widget_states
            .get_mut(&self.id)
            .unwrap()
    }

    pub fn stdin(&mut self) -> String {
        let mut app_lock = self.lock();
        let t = self.get_tws(&mut app_lock);
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
            t.stdout += &format!("$ {trimmed}\n");
        }
        stdin
    }

    pub fn append_output(&mut self, output: &[u8]) {
        let mut app_lock = self.lock();
        let t = self.get_tws(&mut app_lock);
        t.stdout += &from_utf8_lossy(&strip(output).unwrap());
        unsafe {
            (*self.sender).send(BottomEvent::Resize).unwrap_unchecked();
        }
    }

    pub fn limit_output(&mut self) {
        let mut app_lock = self.lock();
        let t = self.get_tws(&mut app_lock);
        let stdout = &mut t.stdout;
        if stdout.len() > 100000 {
            let mut chars = stdout.chars();
            for _ in 0..stdout.len() - 100000 {
                chars.next();
            }
            t.stdout = chars.collect();
        }
    }

    pub fn finish(&mut self) {
        unsafe {
            let mut app_lock = self.lock();
            let t = self.get_tws(&mut app_lock);
            t.is_working = false;
            (*self.sender).send(BottomEvent::Resize).unwrap_unchecked();
        }
    }
}

unsafe impl Sync for UnsafeTerminalWidgetState {}
unsafe impl Send for UnsafeTerminalWidgetState {}
