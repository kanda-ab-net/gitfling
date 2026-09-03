// Windowsでリリース時にコンソールを出さない
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    gitfling_lib::run()
}
