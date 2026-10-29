// On Windows a release build is a window program: without this it would open a console window as well.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

// The app itself is in lib.rs (a phone starts it through the library).
fn main() {
    pi_oven_desktop_lib::run()
}
