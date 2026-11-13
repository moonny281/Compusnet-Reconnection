// Prevents an extra Windows console window.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    campus_net_reconnector_lib::run()
}
