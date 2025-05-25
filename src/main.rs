use komorebi_client::Notification;
use komorebi_client::NotificationEvent;
use komorebi_client::SocketMessage;
use komorebi_client::State;
use komorebi_client::WindowManagerEvent;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::BufRead;
use std::io::BufReader;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::mpsc;
use std::thread;
use std::{io::stdin, process::Command};
use windows::{Win32::System::Com::*, Win32::UI::Shell::*, core::*};

#[derive(Serialize, Deserialize, Debug, Clone)]
enum WallpaperType {
    Windows,
    WallpaperEngine,
}
#[derive(Serialize, Deserialize, Debug, Clone)]
struct Wallpaper {
    path: String,
    kind: Option<WallpaperType>,
}
#[derive(Serialize, Deserialize, Debug)]
struct Workspace {
    index: usize,
    wallpapers: Vec<Wallpaper>,
    interval: Option<usize>,
}
#[derive(Serialize, Deserialize, Debug)]
struct Monitor {
    workspaces: Option<Vec<Workspace>>,
    wallpapers: Option<Vec<Wallpaper>>,
    interval: Option<usize>,
    interval: Option<usize>,
    enable: Option<bool>,
}
#[derive(Serialize, Deserialize, Debug)]
struct Config {
    monitors: Vec<Monitor>,
    wallpapers: Option<Vec<Wallpaper>>,
    we_path: Option<String>,
    interval: Option<usize>,
}

#[derive(Clone, Debug)]
struct Timer {
    interval: usize,
    elapsed: usize,
}

impl Timer {
    fn new(interval: usize) -> Timer {
        return Timer {
            interval: interval,
            elapsed: 0,
        };
    }
}

#[derive(Clone, Debug)]
struct WorkspaceState {
    wallpaper_idx: usize,
    timer: Timer,
}

#[derive(Clone, Debug)]
struct MonitorState {
    workspaces: Vec<WorkspaceState>,
    wallpaper_idx: usize,
    timer: Timer,
}

#[derive(Clone, Debug)]
struct PaperState {
    active_workspaces: Vec<usize>,
    monitors: Vec<MonitorState>,
    timer: Timer,
}

impl PaperState {
    fn new() -> PaperState {
        PaperState {
            active_workspaces: Vec::new(),
            monitors: Vec::new(),
            timer: Timer {
                interval: 0,
                elapsed: 0,
            },
        }
    }
}

#[derive(Debug)]
enum Event {
    SocketEvent { notification: Notification },
    TimerEvent { monitor: usize, workspace: usize },
}

const NAME: &str = "komopaper.sock";

fn main() -> anyhow::Result<()> {
    let socket = komorebi_client::subscribe(NAME)?;
    let json_data = fs::read_to_string("./config.json").expect("Failed to read config.json");
    let config: Config = serde_json::from_str(&json_data).expect("Failed to deserialize JSON");

    let mut paper_state = PaperState::new();

    let state_data = komorebi_client::send_query(&SocketMessage::State).unwrap();
    let state: State = serde_json::from_str(&state_data).expect("Failed to get state");
    paper_state.we_path = config.we_path;
    for (monitor_index, monitor) in state.monitors.elements().iter().enumerate() {
        paper_state
            .active_workspaces
            .push(monitor.focused_workspace_idx());
        set_wallpaper(&config, index, monitor.focused_workspace_idx());
        let mut workspace_states: Vec<WorkspaceState> = Vec::new();
        match &config.monitors[index].workspaces {
            Some(workspaces) => {
                for workspace in workspaces {
                    let interval = workspace.interval.unwrap_or(0);
                    let workspace_state = WorkspaceState {
                        wallpaper_idx: 0,
                        timer: Timer::new(interval),
                    };
                    workspace_states.push(workspace_state);
                }
            }
            None => {}
        }
        let interval = config.monitors[index].interval.unwrap_or(0);
        let monitor_state = MonitorState {
            workspaces: workspace_states,
            wallpaper_idx: 0,
            timer: Timer::new(interval),
        };
        paper_state.monitors.push(monitor_state);
    }
    println!("{:#?}", paper_state);

    let (tx_timer, rx) = mpsc::channel();

    let tx_socket = tx_timer.clone();

    thread::spawn(move || {
        for incoming in socket.incoming() {
            match incoming {
                Ok(data) => {
                    let reader = match data.try_clone() {
                        Ok(cloned_data) => BufReader::new(cloned_data),
                        Err(error) => {
                            println!("Failed to clone data: {error}");
                            continue;
                        }
                    };

                    for line in reader.lines().flatten() {
                        let notification: Notification = match serde_json::from_str(&line) {
                            Ok(notification) => notification,
                            Err(error) => {
                                println!("discarding malformed komorebi notification: {error}");
                                continue;
                            }
                        };
                        if let Err(send_error) = tx_socket.send(Event::SocketEvent { notification })
                        {
                            println!("failed to send notification: {send_error}");
                        }
                    }
                }
                Err(error) => {
                    println!("{error}");
                }
            }
        }
    });
    let timer_state = paper_state.clone();
    thread::spawn(move || loop {});

    for event in rx {
        match event {
            Event::SocketEvent { notification } => {
                let focused_monitor_idx = notification.state.monitors.focused_idx();
                let focused_workspace_idx = notification
                    .state
                    .monitors
                    .focused()
                    .unwrap()
                    .focused_workspace_idx();
                println!("{:#?}", paper_state.active_workspaces);
                if paper_state.active_workspaces[focused_monitor_idx] != focused_workspace_idx {
                    paper_state.active_workspaces[focused_monitor_idx] = focused_workspace_idx;
                    set_wallpaper(&config, focused_monitor_idx, focused_workspace_idx);
                }
            }
            Event::TimerEvent { monitor, workspace } => {
                println!("{monitor}, {workspace}");
            }
        }
    }

    Ok(())
}

fn set_wallpaper(paper_state: &PaperState, monitor_index: usize, workspace_index: usize) {
    let monitor = &paper_state.monitors[monitor_index];
    let workspace = &monitor.workspaces[workspace_index];
    let wallpapers = &workspace.wallpapers;
    let wallpaper_index = workspace.wallpaper_idx;
    if wallpapers.len() > 0 {
        let wallpaper = &wallpapers[wallpaper_index];
        match wallpaper.kind {
            Some(WallpaperType::Windows) | None => {
                if let Some(we_path) = &paper_state.we_path {
                    close_we_wallpaper(
                        paper_state.we_path.clone().unwrap(),
                        monitor_index.try_into().unwrap(),
                    );
                }
                set_win_wallpaper(wallpaper, monitor_index.try_into().unwrap());
            }
            Some(WallpaperType::WallpaperEngine) => {
                if let Some(we_path) = &paper_state.we_path {
                    set_we_wallpaper(
                        we_path.clone(),
                        wallpaper,
                        monitor_index.try_into().unwrap(),
                    );
                } else {
                    println!("No wallpaper engine path set");
                }
            }
        }
    }
}

fn set_win_wallpaper(wallpaper: &Wallpaper, index: u32) {
    let wp_hstring = HSTRING::from(wallpaper.path.clone());
    unsafe {
        CoInitializeEx(None, COINIT_APARTMENTTHREADED).unwrap();
        // IDesktopWallpaper::SetWallpaper(&self, mon1_id, wp1);
        let wallpaper: IDesktopWallpaper =
            CoCreateInstance(&DesktopWallpaper, None, CLSCTX_ALL).unwrap();

        match wallpaper.GetMonitorDevicePathAt(index) {
            Ok(id) => match wallpaper.SetWallpaper(id, &wp_hstring) {
                Ok(_) => {}
                Err(err) => {
                    println!("{:#?}", err)
                }
            },
            Err(err) => {
                println!("{:#?}", err)
            }
        }
    }
}

fn set_we_wallpaper(we_path: String, wallpaper: &Wallpaper, index: u32) {
    let mut we = Command::new(we_path);
    we.arg("-control")
        .arg("openWallpaper")
        .arg("-file")
        .arg(wallpaper.path.clone())
        .arg("-monitor")
        .arg(index.to_string())
        .spawn()
        .expect("Failed to spawn the process");
}

fn close_we_wallpaper(we_path: String, index: u32) {
    let mut we = Command::new(we_path);
    we.arg("-control")
        .arg("closeWallpaper")
        .arg("-monitor")
        .arg(index.to_string())
        .spawn()
        .expect("Failed to spawn the process");
}
