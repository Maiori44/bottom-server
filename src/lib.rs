//! A customizable cross-platform graphical process/system monitor for the terminal.
//! Supports Linux, macOS, and Windows. Inspired by gtop, gotop, and htop.
//!
//! **Note:** The following documentation is primarily intended for people to refer to for development purposes rather
//! than the actual usage of the application. If you are instead looking for documentation regarding the *usage* of
//! bottom, refer to [here](https://clementtsang.github.io/bottom/stable/).

#![warn(rust_2018_idioms)]
#![allow(clippy::uninlined_format_args)]
#![deny(clippy::missing_safety_doc)]
#[allow(unused_imports)]
#[cfg(feature = "log")]
#[macro_use]
extern crate log;

// TODO: Deny unused imports.

use std::{
    boxed::Box,
    fs,
    io::{stderr, stdout, Read, Write},
    path::PathBuf,
    process::{Command, Stdio},
    sync::Mutex,
    sync::{
        mpsc::{Receiver, Sender},
        Arc, Condvar,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use app::{
    data_harvester,
    frozen_state::FrozenState,
    layout_manager::{UsedWidgets, WidgetDirection},
    App,
};
use constants::*;
use crossterm::{
    event::{
        poll, read, DisableBracketedPaste, DisableMouseCapture, Event, KeyCode, KeyEvent,
        KeyModifiers, MouseEvent, MouseEventKind,
    },
    execute,
    terminal::{disable_raw_mode, LeaveAlternateScreen},
};
use data_conversion::*;
use options::*;
use utils::error;
use widgets::UnsafeTerminalWidgetState;

pub mod app;
pub mod utils {
    pub mod error;
    pub mod gen_util;
    pub mod logging;
}
pub mod canvas;
pub mod clap;
pub mod components;
pub mod constants;
pub mod data_conversion;
pub mod options;
pub mod units;
pub mod widgets;

#[cfg(target_family = "windows")]
pub type Pid = usize;

#[cfg(target_family = "unix")]
pub type Pid = libc::pid_t;

#[derive(Debug)]
pub enum BottomEvent {
    Resize,
    KeyInput(KeyEvent),
    MouseInput(MouseEvent),
    PasteEvent(String),
    Update(Box<data_harvester::Data>),
    Clean,
}

#[derive(Debug)]
pub enum ThreadControlEvent {
    Reset,
    UpdateConfig(Box<app::AppConfigFields>),
    UpdateUsedWidgets(Box<UsedWidgets>),
    UpdateUpdateTime(u64),
}

pub fn handle_mouse_event(event: MouseEvent, app: &mut App) {
    match event.kind {
        MouseEventKind::ScrollUp => app.handle_scroll_up(),
        MouseEventKind::ScrollDown => app.handle_scroll_down(),
        MouseEventKind::Down(button) => {
            let (x, y) = (event.column, event.row);
            if !app.app_config_fields.disable_click {
                match button {
                    crossterm::event::MouseButton::Left => {
                        // Trigger left click widget activity
                        app.on_left_mouse_up(x, y);
                    }
                    crossterm::event::MouseButton::Right => {}
                    _ => {}
                }
            }
        }
        _ => {}
    };
}

pub fn handle_key_event_or_break(
    event: KeyEvent,
    app: &'static Mutex<Option<App>>,
    reset_sender: &Sender<ThreadControlEvent>,
    sender: &Sender<BottomEvent>, //termination_ctrl_cvar: Arc<Condvar>,
) -> bool {
    let current_widget_id = app
        .lock()
        .unwrap()
        .as_ref()
        .unwrap()
        .current_widget
        .widget_id;
    let mut app_lock = app.lock().unwrap();
    let app_mut = app_lock.as_mut().unwrap();
    let terminal_widget_state = app_mut
        .terminal_state
        .widget_states
        .get_mut(&current_widget_id);
    if let Some(terminal_widget_state) = terminal_widget_state {
        if !event.modifiers.contains(KeyModifiers::CONTROL) {
            match event.code {
                KeyCode::End => terminal_widget_state.offset = 0,
                KeyCode::PageUp => terminal_widget_state.offset += 1,
                KeyCode::PageDown if terminal_widget_state.offset > 0 => {
                    terminal_widget_state.offset -= 1
                }
                KeyCode::Esc => app_mut.is_expanded = false,
                _ if app_mut.is_expanded && !terminal_widget_state.is_working => {
                    match event.code {
                        KeyCode::Up
                            if {
                                terminal_widget_state.selected_input
                                    < terminal_widget_state.stdin.len() - 1
                            } =>
                        {
                            terminal_widget_state.selected_input += 1;
                        }
                        KeyCode::Down if terminal_widget_state.selected_input > 0 => {
                            terminal_widget_state.selected_input -= 1;
                        }
                        KeyCode::Left
                            if {
                                terminal_widget_state.input_offset
                                    < terminal_widget_state.current_input().len()
                            } =>
                        {
                            terminal_widget_state.input_offset += 1
                        }
                        KeyCode::Right if terminal_widget_state.input_offset > 0 => {
                            terminal_widget_state.input_offset -= 1
                        }
                        KeyCode::Enter if !terminal_widget_state.stdin.is_empty() => {
                            terminal_widget_state.is_working = true;
                            terminal_widget_state.input_offset = 0;
                            drop(app_lock);
                            {
                                let mut t = UnsafeTerminalWidgetState {
                                    id: current_widget_id,
                                    app,
                                    sender,
                                };
                                thread::spawn(move || {
                                    let command = t.stdin();
                                    let mut output = Command::new("bash")
                                        .args(["-c", &command])
                                        .stdin(Stdio::null())
                                        .stdout(Stdio::piped())
                                        .stderr(Stdio::piped())
                                        .spawn()
                                        .unwrap();
                                    while output.try_wait().unwrap().is_none() {
                                        let mut buf = [0];
                                        if output.stdout.as_mut().unwrap().read(&mut buf).unwrap()
                                            > 0
                                        {
                                            t.append_output(&buf);
                                        }
                                    }
                                    let mut end = Vec::new();
                                    output.stdout.unwrap().read_to_end(&mut end).unwrap();
                                    output.stderr.unwrap().read_to_end(&mut end).unwrap();
                                    t.append_output(&end);
                                    t.limit_output();
                                    t.finish();
                                });
                            }
                        }
                        KeyCode::Backspace => {
                            let index = terminal_widget_state.selected_input;
                            let Some(input) = terminal_widget_state.stdin.get_mut(index) else {
                                return false;
                            };
                            if let Some(offset) = {
                                if input.is_empty() {
                                    None
                                } else if input.len() > terminal_widget_state.input_offset {
                                    Some(input.len() - terminal_widget_state.input_offset - 1)
                                } else {
                                    None
                                }
                            } {
                                input.remove(offset);
                            }
                        }
                        KeyCode::Char(c) if c.is_ascii() => {
                            let index = terminal_widget_state.selected_input;
                            if let Some(stdin) = terminal_widget_state.stdin.get_mut(index) {
                                stdin.insert(stdin.len() - terminal_widget_state.input_offset, c);
                            }
                        }
                        KeyCode::Delete => {
                            let index = terminal_widget_state.selected_input;
                            let Some(input) = terminal_widget_state.stdin.get_mut(index) else {
                                return false;
                            };
                            if let Some(offset) = {
                                if terminal_widget_state.input_offset == 0 {
                                    None
                                } else if input.len() >= terminal_widget_state.input_offset {
                                    Some(input.len() - terminal_widget_state.input_offset)
                                } else {
                                    None
                                }
                            } {
                                input.remove(offset);
                                terminal_widget_state.input_offset -= 1;
                            }
                        }
                        KeyCode::F(9) => {
                            terminal_widget_state.stdout.clear();
                            terminal_widget_state.offset = 0;
                        }
                        KeyCode::F(10) => {
                            /*return handle_key_event_or_break(
                                KeyEvent::new(KeyCode::Char('~'), event.modifiers),
                                app,
                                reset_sender,
                                sender,
                                termination_ctrl_cvar,
                            )*/
                        }
                        _ => {}
                    }
                    return false;
                }
                _ => {}
            }
        }
    }
    // debug!("KeyEvent: {:?}", event);

    if event.modifiers.is_empty() {
        // Required catch for searching - otherwise you couldn't search with q.
        if event.code == KeyCode::Char('q') && !app_mut.is_in_search_widget() {
            return true;
        }
        match event.code {
            KeyCode::End => app_mut.skip_to_last(),
            KeyCode::Home => app_mut.skip_to_first(),
            KeyCode::Up => app_mut.on_up_key(),
            KeyCode::Down => app_mut.on_down_key(),
            KeyCode::Left => app_mut.on_left_key(),
            KeyCode::Right => app_mut.on_right_key(),
            /*KeyCode::Char('r') => {
                termination_ctrl_cvar.notify_all();
                return false;
            }*/
            KeyCode::Char(caught_char) => app_mut.on_char_key(caught_char),
            KeyCode::Esc => app_mut.on_esc(),
            KeyCode::Enter => app_mut.on_enter(),
            KeyCode::Tab => app_mut.on_tab(),
            KeyCode::Backspace => app_mut.on_backspace(),
            KeyCode::Delete => app_mut.on_delete(),
            KeyCode::F(1) => app_mut.toggle_ignore_case(),
            KeyCode::F(2) => app_mut.toggle_search_whole_word(),
            KeyCode::F(3) => app_mut.toggle_search_regex(),
            KeyCode::F(5) => app_mut.toggle_tree_mode(),
            KeyCode::F(6) => app_mut.toggle_sort_menu(),
            KeyCode::F(9) => app_mut.start_killing_process(),
            KeyCode::PageDown => app_mut.on_page_down(),
            KeyCode::PageUp => app_mut.on_page_up(),
            _ => {}
        }
    } else {
        // Otherwise, track the modifier as well...
        if let KeyModifiers::ALT = event.modifiers {
            match event.code {
                KeyCode::Char('c') | KeyCode::Char('C') => app_mut.toggle_ignore_case(),
                KeyCode::Char('w') | KeyCode::Char('W') => app_mut.toggle_search_whole_word(),
                KeyCode::Char('r') | KeyCode::Char('R') => app_mut.toggle_search_regex(),
                // KeyCode::Char('b') | KeyCode::Char('B') => todo!(),
                // KeyCode::Char('f') | KeyCode::Char('F') => todo!(),
                KeyCode::Char('h') => app_mut.on_left_key(),
                KeyCode::Char('l') => app_mut.on_right_key(),
                _ => {}
            }
        } else if let KeyModifiers::CONTROL = event.modifiers {
            if event.code == KeyCode::Char('c') {
                return true;
            }

            match event.code {
                KeyCode::Char('f') => app_mut.on_slash(),
                KeyCode::Left => app_mut.move_widget_selection(&WidgetDirection::Left),
                KeyCode::Right => app_mut.move_widget_selection(&WidgetDirection::Right),
                KeyCode::Up => app_mut.move_widget_selection(&WidgetDirection::Up),
                KeyCode::Down => app_mut.move_widget_selection(&WidgetDirection::Down),
                KeyCode::Char('r') => {
                    if reset_sender.send(ThreadControlEvent::Reset).is_ok() {
                        app_mut.reset();
                    }
                }
                KeyCode::Char('a') => app_mut.skip_cursor_beginning(),
                KeyCode::Char('e') => app_mut.skip_cursor_end(),
                KeyCode::Char('u') if app_mut.is_in_search_widget() => app_mut.clear_search(),
                KeyCode::Char('w') => app_mut.clear_previous_word(),
                KeyCode::Char('h') => app_mut.on_backspace(),
                KeyCode::Char('d') => app_mut.scroll_half_page_down(),
                KeyCode::Char('u') => app_mut.scroll_half_page_up(),
                // KeyCode::Char('j') => {}, // Move down
                // KeyCode::Char('k') => {}, // Move up
                // KeyCode::Char('h') => {}, // Move right
                // KeyCode::Char('l') => {}, // Move left
                // Can't do now, CTRL+BACKSPACE doesn't work and graphemes
                // are hard to iter while truncating last (eloquently).
                // KeyCode::Backspace => app_mut.skip_word_backspace(),
                _ => {}
            }
        } else if let KeyModifiers::SHIFT = event.modifiers {
            match event.code {
                KeyCode::Left => app_mut.move_widget_selection(&WidgetDirection::Left),
                KeyCode::Right => app_mut.move_widget_selection(&WidgetDirection::Right),
                KeyCode::Up => app_mut.move_widget_selection(&WidgetDirection::Up),
                KeyCode::Down => app_mut.move_widget_selection(&WidgetDirection::Down),
                KeyCode::Char(caught_char) => app_mut.on_char_key(caught_char),
                _ => {}
            }
        }
    }
    false
}

pub fn read_config(config_location: Option<&String>) -> error::Result<Option<PathBuf>> {
    let config_path = if let Some(conf_loc) = config_location {
        Some(PathBuf::from(conf_loc.as_str()))
    } else if cfg!(target_os = "windows") {
        if let Some(home_path) = dirs::config_dir() {
            let mut path = home_path;
            path.push(DEFAULT_CONFIG_FILE_PATH);
            Some(path)
        } else {
            None
        }
    } else if let Some(home_path) = dirs::home_dir() {
        let mut path = home_path;
        path.push(".config/");
        path.push(DEFAULT_CONFIG_FILE_PATH);
        if path.exists() {
            // If it already exists, use the old one.
            Some(path)
        } else {
            // If it does not, use the new one!
            if let Some(config_path) = dirs::config_dir() {
                let mut path = config_path;
                path.push(DEFAULT_CONFIG_FILE_PATH);
                Some(path)
            } else {
                None
            }
        }
    } else {
        None
    };

    Ok(config_path)
}

pub fn create_or_get_config(config_path: &Option<PathBuf>) -> error::Result<Config> {
    if let Some(path) = config_path {
        if let Ok(config_string) = fs::read_to_string(path) {
            // We found a config file!
            Ok(toml_edit::de::from_str(config_string.as_str())?)
        } else {
            // Config file DNE...
            if let Some(parent_path) = path.parent() {
                fs::create_dir_all(parent_path)?;
            }
            // fs::File::create(path)?.write_all(CONFIG_TOP_HEAD.as_bytes())?;
            fs::File::create(path)?.write_all(CONFIG_TEXT.as_bytes())?;
            Ok(Config::default())
        }
    } else {
        // Don't write, the config path was somehow None...
        Ok(Config::default())
    }
}

pub fn try_drawing(
    terminal: &mut tui::terminal::Terminal<tui::backend::CrosstermBackend<std::io::Stdout>>,
    app: &mut App, painter: &mut canvas::Painter,
) -> error::Result<()> {
    if let Err(err) = painter.draw_data(terminal, app) {
        cleanup_terminal(terminal)?;
        return Err(err);
    }

    Ok(())
}

pub fn cleanup_terminal(
    terminal: &mut tui::terminal::Terminal<tui::backend::CrosstermBackend<std::io::Stdout>>,
) -> error::Result<()> {
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        DisableBracketedPaste,
        DisableMouseCapture,
        LeaveAlternateScreen
    )?;
    terminal.show_cursor()?;

    Ok(())
}

/// Check and report to the user if the current environment is not a terminal.
pub fn check_if_terminal() {
    use crossterm::tty::IsTty;

    if !stdout().is_tty() {
        eprintln!(
            "Warning: bottom is not being output to a terminal. Things might not work properly."
        );
        eprintln!("If you're stuck, press 'q' or 'Ctrl-c' to quit the program.");
        stderr().flush().unwrap();
        thread::sleep(Duration::from_secs(1));
    }
}

pub fn update_data(app: &mut App) {
    let data_source = match &app.frozen_state {
        FrozenState::NotFrozen => &app.data_collection,
        FrozenState::Frozen(data) => data,
    };

    for proc in app.proc_state.widget_states.values_mut() {
        if proc.force_update_data {
            proc.ingest_data(data_source);
            proc.force_update_data = false;
        }
    }

    // FIXME: Make this CPU force update less terrible.
    if app.cpu_state.force_update.is_some() {
        app.converted_data.ingest_cpu_data(data_source);
        app.converted_data.load_avg_data = data_source.load_avg_harvest;

        app.cpu_state.force_update = None;
    }

    // FIXME: This is a bit of a temp hack to move data over.
    {
        let data = &app.converted_data.cpu_data;
        for cpu in app.cpu_state.widget_states.values_mut() {
            cpu.update_table(data);
        }
    }
    {
        let data = &app.converted_data.temp_data;
        for temp in app.temp_state.widget_states.values_mut() {
            if temp.force_update_data {
                temp.ingest_data(data);
                temp.force_update_data = false;
            }
        }
    }
    {
        let data = &app.converted_data.disk_data;
        for disk in app.disk_state.widget_states.values_mut() {
            if disk.force_update_data {
                disk.ingest_data(data);
                disk.force_update_data = false;
            }
        }
    }
    {
        for connections in app.connections_state.widget_states.values_mut() {
            connections.ingest_data(&app.converted_data.connections_data)
        }
    }

    // TODO: [OPT] Prefer reassignment over new vectors?
    if app.mem_state.force_update.is_some() {
        app.converted_data.mem_data = data_source.memory_harvest.clone();
        app.converted_data.swap_data = data_source.swap_harvest.clone();
        #[cfg(feature = "zfs")]
        {
            app.converted_data.arc_data = convert_arc_data_points(data_source);
        }

        #[cfg(feature = "gpu")]
        {
            app.converted_data.gpu_data = convert_gpu_data(data_source);
        }
        app.mem_state.force_update = None;
    }

    if app.net_state.force_update.is_some() {
        let (rx, tx) = get_rx_tx_data_points(
            data_source,
            &app.app_config_fields.network_scale_type,
            &app.app_config_fields.network_unit_type,
            app.app_config_fields.network_use_binary_prefix,
        );
        app.converted_data.network_data_rx = rx;
        app.converted_data.network_data_tx = tx;
        app.net_state.force_update = None;
    }
}

pub fn create_input_thread(
    sender: Sender<BottomEvent>, termination_ctrl_lock: Arc<Mutex<bool>>,
) -> JoinHandle<()> {
    thread::spawn(move || {
        let mut mouse_timer = Instant::now();

        loop {
            if let Ok(is_terminated) = termination_ctrl_lock.try_lock() {
                // We don't block.
                if *is_terminated {
                    drop(is_terminated);
                    break;
                }
            }
            if let Ok(poll) = poll(Duration::from_millis(20)) {
                if poll {
                    if let Ok(event) = read() {
                        // FIXME: Handle all other event cases.
                        match event {
                            // TODO: Might want to debounce this in the future, or take into account the actual resize
                            // values. Maybe we want to keep the current implementation in case the resize event might
                            // not fire... not sure.
                            Event::Resize(_, _) => {
                                if sender.send(BottomEvent::Resize).is_err() {
                                    break;
                                }
                            }
                            Event::Paste(paste) => {
                                if sender.send(BottomEvent::PasteEvent(paste)).is_err() {
                                    break;
                                }
                            }
                            Event::Key(key) => {
                                if sender.send(BottomEvent::KeyInput(key)).is_err() {
                                    break;
                                }
                            }
                            Event::Mouse(mouse) => match mouse.kind {
                                MouseEventKind::Moved | MouseEventKind::Drag(..) => {}
                                MouseEventKind::ScrollDown | MouseEventKind::ScrollUp => {
                                    if Instant::now().duration_since(mouse_timer).as_millis() >= 20
                                    {
                                        if sender.send(BottomEvent::MouseInput(mouse)).is_err() {
                                            break;
                                        }
                                        mouse_timer = Instant::now();
                                    }
                                }
                                _ => {
                                    if sender.send(BottomEvent::MouseInput(mouse)).is_err() {
                                        break;
                                    }
                                }
                            },
                            _ => (),
                        }
                    }
                }
            }
        }
    })
}

pub fn create_collection_thread(
    sender: Sender<BottomEvent>, control_receiver: Receiver<ThreadControlEvent>,
    termination_ctrl_lock: Arc<Mutex<bool>>, termination_ctrl_cvar: Arc<Condvar>,
    app_config_fields: &app::AppConfigFields, filters: app::DataFilters,
    used_widget_set: UsedWidgets,
) -> JoinHandle<()> {
    let temp_type = app_config_fields.temperature_type;
    let use_current_cpu_total = app_config_fields.use_current_cpu_total;
    let unnormalized_cpu = app_config_fields.unnormalized_cpu;
    let show_average_cpu = app_config_fields.show_average_cpu;
    let update_rate_in_milliseconds = app_config_fields.update_rate_in_milliseconds;

    thread::spawn(move || {
        let mut data_state = data_harvester::DataCollector::new(filters);

        data_state.set_data_collection(used_widget_set);
        data_state.set_temperature_type(temp_type);
        data_state.set_use_current_cpu_total(use_current_cpu_total);
        data_state.set_unnormalized_cpu(unnormalized_cpu);
        data_state.set_show_average_cpu(show_average_cpu);

        data_state.init();

        loop {
            // Check once at the very top...
            if let Ok(is_terminated) = termination_ctrl_lock.try_lock() {
                // We don't block here.
                if *is_terminated {
                    drop(is_terminated);
                    break;
                }
            }

            let mut update_time = update_rate_in_milliseconds;
            if let Ok(message) = control_receiver.try_recv() {
                // trace!("Received message in collection thread: {:?}", message);
                match message {
                    ThreadControlEvent::Reset => {
                        data_state.data.cleanup();
                    }
                    ThreadControlEvent::UpdateConfig(app_config_fields) => {
                        data_state.set_temperature_type(app_config_fields.temperature_type);
                        data_state
                            .set_use_current_cpu_total(app_config_fields.use_current_cpu_total);
                        data_state.set_unnormalized_cpu(unnormalized_cpu);
                        data_state.set_show_average_cpu(app_config_fields.show_average_cpu);
                    }
                    ThreadControlEvent::UpdateUsedWidgets(used_widget_set) => {
                        data_state.set_data_collection(*used_widget_set);
                    }
                    ThreadControlEvent::UpdateUpdateTime(new_time) => {
                        update_time = new_time;
                    }
                }
            }

            // TODO: [OPT] this feels like it might not be totally optimal. Hm.
            futures::executor::block_on(data_state.update_data());

            // Yet another check to bail if needed...
            if let Ok(is_terminated) = termination_ctrl_lock.try_lock() {
                // We don't block here.
                if *is_terminated {
                    drop(is_terminated);
                    break;
                }
            }

            let event = BottomEvent::Update(Box::from(data_state.data));
            data_state.data = data_harvester::Data::default();
            if sender.send(event).is_err() {
                break;
            }

            if let Ok((is_terminated, _wait_timeout_result)) = termination_ctrl_cvar.wait_timeout(
                termination_ctrl_lock.lock().unwrap(),
                Duration::from_millis(update_time),
            ) {
                if *is_terminated {
                    drop(is_terminated);
                    break;
                }
            }
        }
    })
}
