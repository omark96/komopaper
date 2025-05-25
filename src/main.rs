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
use std::{io::stdin, process::Command};
use windows::{Win32::System::Com::*, Win32::UI::Shell::*, core::*};

#[derive(Serialize, Deserialize, Debug)]
enum WallpaperType {
    Windows,
    WallpaperEngine,
}
#[derive(Serialize, Deserialize, Debug)]
struct Wallpaper {
    path: String,
    kind: Option<WallpaperType>,
}
#[derive(Serialize, Deserialize, Debug)]
struct Workspace {
    index: usize,
    wallpapers: Vec<Wallpaper>,
}
#[derive(Serialize, Deserialize, Debug)]
struct Monitor {
    workspaces: Option<Vec<Workspace>>,
    wallpapers: Option<Vec<Wallpaper>>,
    enable: Option<bool>,
}
#[derive(Serialize, Deserialize, Debug)]
struct Config {
    monitors: Vec<Monitor>,
    we_path: Option<String>,
}
#[derive(Debug)]
struct PaperState {
    active_workspaces: Vec<usize>,
}

impl PaperState {
    fn new() -> PaperState {
        PaperState {
            active_workspaces: Vec::new(),
        }
    }
}

const NAME: &str = "komopaper.sock";

fn main() -> anyhow::Result<()> {
    let socket = komorebi_client::subscribe(NAME)?;
    let json_data = fs::read_to_string("./config.json").expect("Failed to read config.json");
    let config: Config = serde_json::from_str(&json_data).expect("Failed to deserialize JSON");

    let mut paper_state = PaperState::new();

    let state_data = komorebi_client::send_query(&SocketMessage::State).unwrap();
    let state: State = serde_json::from_str(&state_data).expect("Failed to get state");
    for (index, monitor) in state.monitors.elements().iter().enumerate() {
        paper_state
            .active_workspaces
            .push(monitor.focused_workspace_idx());
        set_wallpaper(&config, index, monitor.focused_workspace_idx());
    }
    for incoming in socket.incoming() {
        match incoming {
            Ok(data) => {
                let reader = BufReader::new(data.try_clone()?);

                for line in reader.lines().flatten() {
                    let notification: Notification = match serde_json::from_str(&line) {
                        Ok(notification) => notification,
                        Err(error) => {
                            println!("discarding malformed komorebi notification: {error}");
                            continue;
                        }
                    };
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
            }
            Err(error) => {
                println!("{error}");
            }
        }
    }
    Ok(())
}

fn set_wallpaper(config: &Config, monitor_index: usize, workspace_index: usize) {
    if let Some(workspaces) = config.monitors[monitor_index].workspaces.as_ref() {
        match get_workspace_by_index(&workspaces, workspace_index) {
            Some(workspace) => {
                let wallpaper = &workspace.wallpapers[0];
                if let Some(kind) = wallpaper.kind.as_ref() {
                    match kind {
                        WallpaperType::Windows => {
                            close_we_wallpaper(
                                config.we_path.clone().unwrap(),
                                monitor_index.try_into().unwrap(),
                            );
                            set_win_wallpaper(wallpaper, monitor_index.try_into().unwrap());
                        }
                        WallpaperType::WallpaperEngine => {
                            set_we_wallpaper(
                                config.we_path.clone().unwrap(),
                                wallpaper,
                                monitor_index.try_into().unwrap(),
                            );
                        }
                    }
                }
            }
            None => {}
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

fn get_workspace_by_index(workspaces: &Vec<Workspace>, target_index: usize) -> Option<&Workspace> {
    workspaces
        .iter()
        .find(|workspace| workspace.index == target_index)
}
