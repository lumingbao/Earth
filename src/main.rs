#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod clipboard;
mod hotkey;
mod i18n;
mod instance;
mod model;
mod platform;
mod search;
mod storage;
mod transforms;
mod tray;
mod ui;
mod updater;

use app::EarthApp;
use std::sync::Arc;

fn main() -> eframe::Result<()> {
    let (storage, instance_lock) = match storage::Storage::open_primary() {
        Ok(Some(primary)) => primary,
        Ok(None) => return Ok(()),
        Err(message) => {
            eprintln!("Earth: {message}");
            rfd::MessageDialog::new()
                .set_title("Earth 无法启动")
                .set_description(message)
                .set_level(rfd::MessageLevel::Error)
                .show();
            return Ok(());
        }
    };
    let start_hidden = platform::should_start_hidden_from_args();
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
        .with_taskbar(false)
        .with_resizable(true)
        .with_visible(!start_hidden)
        .with_active(!start_hidden)
        .with_inner_size([424.0, 520.0])
        .with_min_inner_size([380.0, 380.0])
        .with_icon(icon);

    #[cfg(target_os = "windows")]
    let viewport = viewport.with_transparent(false);

    #[allow(unused_mut)]
    let mut native_options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    #[cfg(target_os = "windows")]
    {
        use winit::platform::windows::EventLoopBuilderExtWindows as _;
        // On Windows, wgpu DX12 HWND swapchains only support CompositeAlphaMode::Opaque, which
        // fills transparent corners and empty workbench animation areas with solid black.
        // Switch to Glow (WGL + DwmEnableBlurBehindWindow), keeping `viewport.transparent` false
        // for GL pixel-format matching (avoiding driver-strict WGL_TRANSPARENT_ARB rejections while
        // still requesting 8 alpha bits) and enabling `transparent` in `window_builder` for HWND creation.
        native_options.renderer = eframe::Renderer::Glow;
        native_options.window_builder = Some(Box::new(|builder| builder.with_transparent(true)));
        native_options.event_loop_builder = Some(Box::new(|builder| {
            builder.with_msg_hook(platform::handle_windows_msg_hook);
        }));
    }

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
        Box::new(move |cc| {
            Ok(Box::new(EarthApp::new(
                cc,
                start_hidden,
                storage,
                instance_lock,
            )))
        }),
    )
}
