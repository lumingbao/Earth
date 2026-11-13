#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod clipboard;
mod hotkey;
mod model;
mod platform;
mod search;
mod storage;
mod transforms;
mod tray;
mod ui;

use app::EarthApp;
use std::sync::Arc;

fn main() -> eframe::Result<()> {
    let icon_rgba = tray::build_earth_tray_rgba(64, false);
    let icon = Arc::new(egui::IconData {
        rgba: icon_rgba,
        width: 64,
        height: 64,
    });

    let viewport = egui::ViewportBuilder::default()
        .with_title("Earth")
        .with_decorations(false)
        .with_transparent(true)
        .with_has_shadow(true)
        .with_always_on_top()
        .with_taskbar(false)
        .with_resizable(true)
        .with_inner_size([470.0, 550.0])
        .with_min_inner_size([400.0, 400.0])
        .with_icon(icon);

    #[allow(unused_mut)]
    let mut native_options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    #[cfg(target_os = "macos")]
    {
        use winit::platform::macos::{ActivationPolicy, EventLoopBuilderExtMacOS};
        native_options.event_loop_builder = Some(Box::new(|builder| {
            builder.with_activation_policy(ActivationPolicy::Accessory);
        }));
    }

    eframe::run_native(
        "Earth",
        native_options,
        Box::new(|cc| Ok(Box::new(EarthApp::new(cc)))),
    )
}
