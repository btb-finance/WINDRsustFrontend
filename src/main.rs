mod app;
mod blockchain;
mod components;
mod config;
mod state;
mod wallet;

use leptos::*;

fn main() {
    // Install panic hook first — prints human-readable Rust panics in the browser
    // console instead of "RuntimeError: unreachable". Critical on low-end hardware.
    console_error_panic_hook::set_once();

    console_log::init_with_level(log::Level::Debug)
        .expect("Failed to initialise console_log");

    log::info!("Wind Swap WASM booting…");

    // mount_to_body is the stable-Rust Leptos 0.6 entry-point.
    mount_to_body(|| view! { <app::App/> });
}
