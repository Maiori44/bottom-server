#![warn(rust_2018_idioms)]
#![allow(clippy::uninlined_format_args)]
#![allow(non_upper_case_globals)]
#[allow(unused_imports)]
#[cfg(feature = "log")]
#[macro_use]
extern crate log;

use std::{
    io::stdout,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Condvar, Mutex,
    },
    thread,
    time::Duration,
};

use anyhow::{Context, Result};
use bottom::{
    app::App,
    canvas::{self, canvas_styling::CanvasColours},
    constants::*,
    data_conversion::*,
    options::*,
    *,
};
use crossterm::{
    event::{EnableBracketedPaste, EnableMouseCapture},
    execute,
    terminal::{enable_raw_mode, EnterAlternateScreen},
};
use tui::{backend::CrosstermBackend, Terminal};

static app: Mutex<Option<App>> = Mutex::new(None);

fn main() -> Result<()> {
    let matches = clap::get_matches();
    #[cfg(all(feature = "fern", debug_assertions))]
    {
        utils::logging::init_logger(log::LevelFilter::Debug, std::ffi::OsStr::new("debug.log"))?;
    }

    // Read from config file.
    let config_path = read_config(matches.get_one::<String>("config_location"))
        .context("Unable to access the given config file location.")?;
    let mut config: Config = create_or_get_config(&config_path)
        .context("Unable to properly parse or create the config file.")?;

    // Get widget layout separately
    let (widget_layout, default_widget_id, default_widget_type_option) =
        get_widget_layout(&matches, &config)
            .context("Found an issue while trying to build the widget layout.")?;

    // FIXME: Should move this into build app or config
    let colours = {
        let colour_scheme = get_color_scheme(&matches, &config)?;
        CanvasColours::new(colour_scheme, &config)?
    };

    // Create "app" struct, which will control most of the program and store settings/state
    let raw_app = build_app(
        &matches,
        &mut config,
        &widget_layout,
        default_widget_id,
        &default_widget_type_option,
        &colours,
    )?;

    *app.lock().unwrap() = Some(raw_app);

    // Create painter and set colours.
    let mut painter = canvas::Painter::init(widget_layout, colours)?;

    // Check if the current environment is in a terminal.
    check_if_terminal();

    // Create termination mutex and cvar
    #[allow(clippy::mutex_atomic)]
    let thread_termination_lock = Arc::new(Mutex::new(false));
    let thread_termination_cvar = Arc::new(Condvar::new());

    // Set up input handling
    let (sender, receiver) = mpsc::channel();
    let _input_thread = create_input_thread(sender.clone(), thread_termination_lock.clone());

    // Cleaning loop
    let _cleaning_thread = {
        let lock = thread_termination_lock.clone();
        let cvar = thread_termination_cvar.clone();
        let cleaning_sender = sender.clone();
        let offset_wait_time = app
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .app_config_fields
            .retention_ms
            + 60000;
        thread::spawn(move || {
            loop {
                let result = cvar.wait_timeout(
                    lock.lock().unwrap(),
                    Duration::from_millis(offset_wait_time),
                );
                if let Ok(result) = result {
                    if *(result.0) {
                        break;
                    }
                }
                if cleaning_sender.send(BottomEvent::Clean).is_err() {
                    // debug!("Failed to send cleaning sender...");
                    break;
                }
            }
        })
    };

    // Event loop
    let (collection_thread_ctrl_sender, collection_thread_ctrl_receiver) = mpsc::channel();
    let _collection_thread = {
        let app_lock = app.lock().unwrap();
        create_collection_thread(
            sender.clone(),
            collection_thread_ctrl_receiver,
            thread_termination_lock.clone(),
            thread_termination_cvar.clone(),
            &app_lock.as_ref().unwrap().app_config_fields,
            app_lock.as_ref().unwrap().filters.clone(),
            app_lock.as_ref().unwrap().used_widgets.clone(),
        )
    };

    // Set up up tui and crossterm
    let mut stdout_val = stdout();
    execute!(
        stdout_val,
        EnterAlternateScreen,
        EnableMouseCapture,
        EnableBracketedPaste
    )?;
    enable_raw_mode()?;

    let mut terminal = Terminal::new(CrosstermBackend::new(stdout_val))?;
    terminal.clear()?;
    terminal.hide_cursor()?;

    #[cfg(target_os = "freebsd")]
    let _stderr_fd = {
        // A really ugly band-aid to suppress stderr warnings on FreeBSD due to sysinfo.
        use std::fs::OpenOptions;

        use filedescriptor::{FileDescriptor, StdioDescriptor};

        let path = OpenOptions::new().write(true).open("/dev/null")?;
        FileDescriptor::redirect_stdio(&path, StdioDescriptor::Stderr)?
    };

    // Set termination hook
    let is_terminated = Arc::new(AtomicBool::new(false));
    let ist_clone = is_terminated.clone();
    ctrlc::set_handler(move || {
        ist_clone.store(true, Ordering::SeqCst);
    })?;
    let mut first_run = true;

    while !is_terminated.load(Ordering::SeqCst) {
        // TODO: Would be good to instead use a mix of is_terminated check + recv. Probably use a termination event instead.
        if let Ok(recv) = receiver.recv_timeout(Duration::from_millis(TICK_RATE_IN_MILLISECONDS)) {
            match recv {
                BottomEvent::Resize => {
                    try_drawing(
                        &mut terminal,
                        app.lock().unwrap().as_mut().unwrap(),
                        &mut painter,
                    )?; // FIXME: This is bugged with frozen?
                }
                BottomEvent::KeyInput(event) => {
                    if handle_key_event_or_break(
                        event,
                        &app,
                        &collection_thread_ctrl_sender,
                        &sender,
                        thread_termination_cvar.clone(),
                    ) {
                        break;
                    }
                    update_data(app.lock().unwrap().as_mut().unwrap());
                    try_drawing(
                        &mut terminal,
                        app.lock().unwrap().as_mut().unwrap(),
                        &mut painter,
                    )?;
                }
                BottomEvent::MouseInput(event) => {
                    handle_mouse_event(event, app.lock().unwrap().as_mut().unwrap());
                    update_data(app.lock().unwrap().as_mut().unwrap());
                    try_drawing(
                        &mut terminal,
                        app.lock().unwrap().as_mut().unwrap(),
                        &mut painter,
                    )?;
                }
                BottomEvent::PasteEvent(paste) => {
                    app.lock().unwrap().as_mut().unwrap().handle_paste(paste);
                    update_data(&mut app.lock().unwrap().as_mut().unwrap());
                    try_drawing(
                        &mut terminal,
                        app.lock().unwrap().as_mut().unwrap(),
                        &mut painter,
                    )?;
                }
                BottomEvent::Update(data) => {
                    app.lock()
                        .unwrap()
                        .as_mut()
                        .unwrap()
                        .data_collection
                        .eat_data(data);

                    // This thing is required as otherwise, some widgets can't draw correctly w/o
                    // some data (or they need to be re-drawn).
                    if first_run {
                        first_run = false;
                        app.lock().unwrap().as_mut().unwrap().is_force_redraw = true;
                    }

                    if !app
                        .lock()
                        .unwrap()
                        .as_mut()
                        .unwrap()
                        .frozen_state
                        .is_frozen()
                    {
                        // Convert all data into tui-compliant components
                        let data_collection = app
                            .lock()
                            .unwrap()
                            .as_ref()
                            .unwrap()
                            .data_collection
                            .clone();
                        // Network
                        if app.lock().unwrap().as_mut().unwrap().used_widgets.use_net {
                            let network_data = {
                                let app_lock = app.lock().unwrap();
                                convert_network_data_points(
                                    &app_lock.as_ref().unwrap().data_collection,
                                    app_lock.as_ref().unwrap().app_config_fields.use_basic_mode
                                        || app_lock
                                            .as_ref()
                                            .unwrap()
                                            .app_config_fields
                                            .use_old_network_legend,
                                    &app_lock
                                        .as_ref()
                                        .unwrap()
                                        .app_config_fields
                                        .network_scale_type,
                                    &app_lock
                                        .as_ref()
                                        .unwrap()
                                        .app_config_fields
                                        .network_unit_type,
                                    app_lock
                                        .as_ref()
                                        .unwrap()
                                        .app_config_fields
                                        .network_use_binary_prefix,
                                )
                            };
                            app.lock()
                                .unwrap()
                                .as_mut()
                                .unwrap()
                                .converted_data
                                .network_data_rx = network_data.rx;
                            app.lock()
                                .unwrap()
                                .as_mut()
                                .unwrap()
                                .converted_data
                                .network_data_tx = network_data.tx;
                            app.lock()
                                .unwrap()
                                .as_mut()
                                .unwrap()
                                .converted_data
                                .rx_display = network_data.rx_display;
                            app.lock()
                                .unwrap()
                                .as_mut()
                                .unwrap()
                                .converted_data
                                .tx_display = network_data.tx_display;
                            if let Some(total_rx_display) = network_data.total_rx_display {
                                app.lock()
                                    .unwrap()
                                    .as_mut()
                                    .unwrap()
                                    .converted_data
                                    .total_rx_display = total_rx_display;
                            }
                            if let Some(total_tx_display) = network_data.total_tx_display {
                                app.lock()
                                    .unwrap()
                                    .as_mut()
                                    .unwrap()
                                    .converted_data
                                    .total_tx_display = total_tx_display;
                            }
                        }

                        // Disk
                        if app.lock().unwrap().as_mut().unwrap().used_widgets.use_disk {
                            app.lock()
                                .unwrap()
                                .as_mut()
                                .unwrap()
                                .converted_data
                                .ingest_disk_data(&data_collection);

                            for disk in app
                                .lock()
                                .unwrap()
                                .as_mut()
                                .unwrap()
                                .disk_state
                                .widget_states
                                .values_mut()
                            {
                                disk.force_data_update();
                            }
                        }

                        // Temperatures
                        if app.lock().unwrap().as_mut().unwrap().used_widgets.use_temp {
                            {
                                let mut app_lock = app.lock().unwrap();
                                let temperature_type = app_lock
                                    .as_ref()
                                    .unwrap()
                                    .app_config_fields
                                    .temperature_type;
                                app_lock
                                    .as_mut()
                                    .unwrap()
                                    .converted_data
                                    .ingest_temp_data(&data_collection, temperature_type);
                            }

                            for temp in app
                                .lock()
                                .unwrap()
                                .as_mut()
                                .unwrap()
                                .temp_state
                                .widget_states
                                .values_mut()
                            {
                                temp.force_data_update();
                            }
                        }

                        if !app
                            .lock()
                            .unwrap()
                            .as_mut()
                            .unwrap()
                            .connections_state
                            .widget_states
                            .is_empty()
                        {
                            app.lock()
                                .unwrap()
                                .as_mut()
                                .unwrap()
                                .converted_data
                                .ingest_connections_data();
                        }

                        // Memory
                        if app.lock().unwrap().as_mut().unwrap().used_widgets.use_mem {
                            let memory_harvest = app
                                .lock()
                                .unwrap()
                                .as_mut()
                                .unwrap()
                                .data_collection
                                .memory_harvest
                                .clone();
                            app.lock()
                                .unwrap()
                                .as_mut()
                                .unwrap()
                                .converted_data
                                .mem_data = memory_harvest;
                            let swap_harvest = app
                                .lock()
                                .unwrap()
                                .as_mut()
                                .unwrap()
                                .data_collection
                                .swap_harvest
                                .clone();
                            app.lock()
                                .unwrap()
                                .as_mut()
                                .unwrap()
                                .converted_data
                                .swap_data = swap_harvest;

                            let (memory_labels, swap_labels) = convert_mem_labels(
                                &app.lock().unwrap().as_mut().unwrap().data_collection,
                            );

                            app.lock()
                                .unwrap()
                                .as_mut()
                                .unwrap()
                                .converted_data
                                .mem_labels = memory_labels;
                            app.lock()
                                .unwrap()
                                .as_mut()
                                .unwrap()
                                .converted_data
                                .swap_labels = swap_labels;
                        }

                        // CPU
                        if app.lock().unwrap().as_mut().unwrap().used_widgets.use_cpu {
                            app.lock()
                                .unwrap()
                                .as_mut()
                                .unwrap()
                                .converted_data
                                .ingest_cpu_data(&data_collection);
                            let load_avg_harvest = app
                                .lock()
                                .unwrap()
                                .as_ref()
                                .unwrap()
                                .data_collection
                                .load_avg_harvest;
                            app.lock()
                                .unwrap()
                                .as_mut()
                                .unwrap()
                                .converted_data
                                .load_avg_data = load_avg_harvest;
                        }

                        // Processes
                        if app.lock().unwrap().as_mut().unwrap().used_widgets.use_proc {
                            for proc in app
                                .lock()
                                .unwrap()
                                .as_mut()
                                .unwrap()
                                .proc_state
                                .widget_states
                                .values_mut()
                            {
                                proc.force_data_update();
                            }
                        }

                        // Battery
                        #[cfg(feature = "battery")]
                        {
                            if app
                                .lock()
                                .unwrap()
                                .as_mut()
                                .unwrap()
                                .used_widgets
                                .use_battery
                            {
                                app.lock()
                                    .unwrap()
                                    .as_mut()
                                    .unwrap()
                                    .converted_data
                                    .battery_data = convert_battery_harvest(&data_collection);
                            }
                        }

                        update_data(app.lock().unwrap().as_mut().unwrap());
                        try_drawing(
                            &mut terminal,
                            app.lock().unwrap().as_mut().unwrap(),
                            &mut painter,
                        )?;
                    }
                }
                BottomEvent::Clean => {
                    let retention_ms = app
                        .lock()
                        .unwrap()
                        .as_mut()
                        .unwrap()
                        .app_config_fields
                        .retention_ms;
                    app.lock()
                        .unwrap()
                        .as_mut()
                        .unwrap()
                        .data_collection
                        .clean_data(retention_ms);
                }
            }
        }
    }

    // I think doing it in this order is safe...

    *thread_termination_lock.lock().unwrap() = true;

    thread_termination_cvar.notify_all();

    cleanup_terminal(&mut terminal)?;

    Ok(())
}
