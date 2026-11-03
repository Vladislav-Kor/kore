//! KORE — Korchagin Overload & Resource Engine. K — Корчагин.
//! Пульт загрузки ПК для Windows в геройском стиле: сторож CPU, графики, оперативная память.
//! Своё приложение может добавить вкладки и плашки шапки через [`Extra`] и открыть окно через [`run`].

pub mod admin;
pub mod app;
pub mod comic;
pub mod guard;
pub mod live;
pub mod ram;
pub mod system;

use std::sync::Arc;

use eframe::egui;
use eframe::egui_wgpu::{WgpuConfiguration, WgpuSetup, WgpuSetupCreateNew};

pub use app::{Extra, Feedback, NoExtra, Pill, Settings, View, NAME};

const ICON_SIZE: u32 = 64;

/// Окну хватает мелких блоков видеопамяти и одного кадра в очереди:
/// по умолчанию wgpu берёт блоки по 256 МБ.
fn wgpu_options() -> WgpuConfiguration {
    let setup = WgpuSetupCreateNew::default();
    let device = setup.device_descriptor.clone();
    WgpuConfiguration {
        desired_maximum_frame_latency: Some(1),
        wgpu_setup: WgpuSetup::CreateNew(WgpuSetupCreateNew {
            device_descriptor: Arc::new(move |adapter| wgpu::DeviceDescriptor {
                memory_hints: wgpu::MemoryHints::MemoryUsage,
                ..device(adapter)
            }),
            ..setup
        }),
        ..Default::default()
    }
}

/// Открыть окно пульта. Возвращается, когда окно закрыли.
pub fn run(settings: Settings, extra: Box<dyn Extra>) -> eframe::Result<()> {
    // wgpu берёт список бэкендов из окружения: только Direct3D 12. OpenGL на части драйверов зависает.
    std::env::set_var("WGPU_BACKEND", "dx12");
    let icon = egui::IconData {
        rgba: include_bytes!("../assets/kore.rgba").to_vec(),
        width: ICON_SIZE,
        height: ICON_SIZE,
    };
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1120.0, 900.0])
            .with_min_inner_size([940.0, 640.0])
            .with_title(NAME)
            .with_icon(icon),
        renderer: eframe::Renderer::Wgpu,
        wgpu_options: wgpu_options(),
        ..Default::default()
    };
    eframe::run_native(
        NAME,
        native_options,
        Box::new(move |cc| {
            comic::apply(&cc.egui_ctx);
            Ok(Box::new(app::App::new(&cc.egui_ctx, settings, extra)))
        }),
    )
}
