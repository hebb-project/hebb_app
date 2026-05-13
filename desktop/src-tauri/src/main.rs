// Prevents an extra console window from opening on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    long_horizon_cortex_desktop_lib::run()
}
