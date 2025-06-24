use eframe::egui;
use vox_gui::client::app::VoxApp;

fn main() -> Result<(), eframe::Error> {
    env_logger::init();
    
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Vox Remote Desktop Client")
            .with_inner_size([1024.0, 768.0])
            .with_min_inner_size([800.0, 600.0])
            .with_icon(egui::IconData::default()), // Prevent icon loading crash
        ..Default::default()
    };
    
    eframe::run_native(
        "Vox Remote Desktop",
        options,
        Box::new(|cc| Ok(Box::new(VoxApp::new(cc)))),
    )
}