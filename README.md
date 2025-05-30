# Komopaper
Komopaper is a wallpaper switcher for Komorebi that allows you to set custom wallpapers per workspace. It supports both native windows wallpapers and Wallpaper Engine. See the provided config.json for how to configure. Make sure to have as many elements in the monitors array as you have on your desktop. It's perfectly ok to leave a monitors element empty, Komopaper will ignore it then.

# Showcase and basic config
[![IMAGE ALT TEXT](http://img.youtube.com/vi/VG502xo58Bk/0.jpg)](https://www.youtube.com/watch?v=VG502xo58Bk)

# How-to
To run the program download the executable and put it in any folder alongside your config.json.

I will improve the documentation, but for now you can follow the types for the config to get a feel for the things you have available to be configured or take a look at the example config.

Interval and wallpapers work in order of specificity: Workspace > Monitor > Global

If an interval is set for a workspace that will be used for that workspace, otherwise it will look for the setting in the monitor and lastly if not found it will look at the global setting.

*IMPORTANT:* For Wallpaper Engine to work you have to set the we_path in your config.

``` Rust
struct Config {
    monitors: Vec<Monitor>,
    wallpapers: Option<Vec<Wallpaper>>,
    we_path: Option<String>,
    interval: Option<usize>,
}

struct Monitor {
    workspaces: Option<Vec<Workspace>>,
    wallpapers: Option<Vec<Wallpaper>>,
    interval: Option<usize>,
    enable: Option<bool>,
}

struct Workspace {
    index: usize,
    wallpapers: Option<Vec<Wallpaper>>,
    interval: Option<usize>,
}

struct Wallpaper {
    path: String,
    kind: Option<WallpaperType>,
}

enum WallpaperType {
    Windows,
    WallpaperEngine,
}
```
