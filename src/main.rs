use komorebi_client::Notification;
use komorebi_client::SocketMessage;
use komorebi_client::State;
use komorebi_client::UnixListener;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::BufRead;
use std::io::BufReader;
use std::process::Command;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};
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
    wallpapers: Option<Vec<Wallpaper>>,
    interval: Option<usize>,
}
#[derive(Serialize, Deserialize, Debug)]
struct Monitor {
    workspaces: Option<Vec<Workspace>>,
    wallpapers: Option<Vec<Wallpaper>>,
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
    interval: Duration,
    next: Instant,
}

impl Timer {
    fn new(interval: usize) -> Timer {
        Timer {
            interval: Duration::from_secs(interval.try_into().unwrap()),
            next: Instant::now() + Duration::from_secs(interval.try_into().unwrap()),
        }
    }

    fn check_and_reset(&mut self) -> bool {
        let now = Instant::now();
        if now >= self.next {
            self.next = now + self.interval;
            true
        } else {
            false
        }
    }
}

#[derive(Clone, Debug)]
struct WorkspaceState {
    wallpaper_idx: usize,
    wallpapers: Vec<Wallpaper>,
    timer: Option<Timer>,
}
impl WorkspaceState {
    fn new() -> Self {
        Self {
            wallpaper_idx: 0,
            wallpapers: Vec::new(),
            timer: None,
        }
    }
}

#[derive(Clone, Debug)]
struct MonitorState {
    workspaces: Vec<WorkspaceState>,
}
impl MonitorState {
    fn new() -> Self {
        Self {
            workspaces: Vec::new(),
        }
    }
}

#[derive(Clone, Debug)]
struct PaperState {
    active_workspaces: Vec<usize>,
    monitors: Vec<MonitorState>,
    we_path: Option<String>,
}

impl PaperState {
    fn new() -> PaperState {
        PaperState {
            active_workspaces: Vec::new(),
            monitors: Vec::new(),
            we_path: None,
        }
    }
}

#[derive(Debug)]
enum Event {
    SocketEvent {
        notification: Notification,
    },
    TimerEvent {
        monitor_idx: usize,
        workspace_idx: usize,
    },
}

const NAME: &str = "komopaper.sock";

fn main() -> anyhow::Result<()> {
    let socket = komorebi_client::subscribe(NAME)?;
    let json_data = fs::read_to_string("./config.json").expect("Failed to read config.json");
    let config: Config = serde_json::from_str(&json_data).expect("Failed to deserialize JSON");

    let state_data = komorebi_client::send_query(&SocketMessage::State).unwrap();
    let state: State = serde_json::from_str(&state_data).expect("Failed to get state");
    let mut paper_state = initialize_paper_state(&config, &state);

    let (tx_timer, rx) = mpsc::channel();

    let tx_socket = tx_timer.clone();
    spawn_socket_thread(socket, tx_socket);

    let timer_state = paper_state.clone();
    spawn_timer_thread(timer_state, tx_timer);

    for event in rx {
        match event {
            Event::SocketEvent { notification } => {
                handle_socket_event(&mut paper_state, notification)
            }
            Event::TimerEvent {
                monitor_idx,
                workspace_idx,
            } => handle_timer_event(&mut paper_state, monitor_idx, workspace_idx),
        }
        println!("{:#?}", paper_state.active_workspaces);
    }

    Ok(())
}

fn spawn_timer_thread(mut timer_state: PaperState, tx_timer: mpsc::Sender<Event>) {
    thread::spawn(move || {
        loop {
            for (monitor_index, monitor) in timer_state.monitors.iter_mut().enumerate() {
                for (workspace_index, workspace) in monitor.workspaces.iter_mut().enumerate() {
                    if let Some(ref mut timer) = workspace.timer {
                        if timer.check_and_reset() {
                            if let Err(send_error) = tx_timer.send(Event::TimerEvent {
                                monitor_idx: monitor_index,
                                workspace_idx: workspace_index,
                            }) {
                                println!("failed to send notification: {send_error}");
                            }
                        }
                    }
                }
            }
            thread::sleep(Duration::from_secs(1));
        }
    });
}

fn spawn_socket_thread(socket: UnixListener, tx_socket: mpsc::Sender<Event>) {
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
}

fn handle_timer_event(paper_state: &mut PaperState, monitor_idx: usize, workspace_idx: usize) {
    let workspace = &mut paper_state.monitors[monitor_idx].workspaces[workspace_idx];
    if workspace.wallpapers.len() == 0 {
        return;
    }
    let wallpapers_len = workspace.wallpapers.len();

    let old_wallpaper_idx = workspace.wallpaper_idx;
    workspace.wallpaper_idx = (workspace.wallpaper_idx + 1) % wallpapers_len;
    println!("{:#?}", workspace.wallpaper_idx);
    if paper_state.active_workspaces[monitor_idx] == workspace_idx
        && old_wallpaper_idx != workspace.wallpaper_idx
    {
        set_wallpaper(&paper_state, monitor_idx, workspace_idx);
    }
}

fn handle_socket_event(paper_state: &mut PaperState, notification: Notification) {
    let focused_monitor_idx = notification.state.monitors.focused_idx();
    let focused_workspace_idx = notification
        .state
        .monitors
        .focused()
        .unwrap()
        .focused_workspace_idx();
    if paper_state.active_workspaces[focused_monitor_idx] != focused_workspace_idx {
        paper_state.active_workspaces[focused_monitor_idx] = focused_workspace_idx;
        set_wallpaper(&paper_state, focused_monitor_idx, focused_workspace_idx);
    }
}

fn initialize_paper_state(config: &Config, state: &State) -> PaperState {
    let mut paper_state = PaperState::new();
    paper_state.we_path = config.we_path.clone();
    for (monitor_index, monitor) in state.monitors.elements().iter().enumerate() {
        paper_state
            .active_workspaces
            .push(monitor.focused_workspace_idx());
        let monitor_state = MonitorState::new();
        paper_state.monitors.push(monitor_state);
        for (workspace_index, _workspace) in monitor.workspaces.elements().iter().enumerate() {
            let mut workspace_state = WorkspaceState::new();
            let timer;
            match &config.monitors[monitor_index].workspaces {
                Some(workspaces) => {
                    let workspace_config = &workspaces[workspace_index];
                    let interval = workspace_config
                        .interval
                        .or(config.monitors[monitor_index].interval)
                        .or(config.interval);
                    timer = interval.map(|interval| Timer::new(interval));
                    if let Some(workspace_wallpapers) = &workspace_config.wallpapers {
                        workspace_state.wallpapers = workspace_wallpapers.clone();
                    } else if let Some(monitor_wallpapers) =
                        &config.monitors[monitor_index].wallpapers
                    {
                        workspace_state.wallpapers = monitor_wallpapers.clone();
                    } else if let Some(global_wallpapers) = &config.wallpapers {
                        workspace_state.wallpapers = global_wallpapers.clone();
                    }
                }
                None => {
                    let interval = config.monitors[monitor_index].interval.or(config.interval);
                    timer = interval.map(|interval| Timer::new(interval));
                    if let Some(monitor_wallpapers) = &config.monitors[monitor_index].wallpapers {
                        workspace_state.wallpapers = monitor_wallpapers.clone();
                    } else if let Some(global_wallpapers) = &config.wallpapers {
                        workspace_state.wallpapers = global_wallpapers.clone();
                    }
                }
            }
            workspace_state.timer = timer;
            paper_state.monitors[monitor_index]
                .workspaces
                .push(workspace_state);
        }
    }
    return paper_state;
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
                    close_we_wallpaper(we_path.clone(), monitor_index.try_into().unwrap());
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
