use egui::{Context, Style, Visuals, Color32, Rounding, Stroke, FontId, FontFamily, TextStyle, epaint::Shadow, Margin};
use std::collections::BTreeMap;

pub fn apply_custom_theme(ctx: &Context) {
    let mut style = Style::default();
    
    // Define custom colors
    let _bg_dark = Color32::from_rgb(20, 23, 30);
    let bg_panel = Color32::from_rgb(28, 32, 40);
    let bg_window = Color32::from_rgb(35, 40, 50);
    let accent = Color32::from_rgb(88, 166, 255);
    let accent_hover = Color32::from_rgb(108, 186, 255);
    let text_primary = Color32::from_rgb(220, 225, 230);
    let text_secondary = Color32::from_rgb(150, 160, 170);
    let error = Color32::from_rgb(255, 88, 88);
    let _success = Color32::from_rgb(88, 255, 166);
    
    // Configure visuals
    style.visuals = Visuals {
        dark_mode: true,
        override_text_color: Some(text_primary),
        
        // Window styling
        window_rounding: Rounding::same(8.0),
        window_shadow: Shadow {
            offset: egui::Vec2::new(0.0, 8.0),
            blur: 16.0,
            spread: 0.0,
            color: Color32::from_black_alpha(60),
        },
        window_fill: bg_window,
        window_stroke: Stroke::new(1.0, Color32::from_gray(40)),
        
        // Panel styling
        panel_fill: bg_panel,
        
        // Widget visuals
        widgets: egui::style::Widgets {
            noninteractive: egui::style::WidgetVisuals {
                bg_fill: bg_panel,
                weak_bg_fill: bg_panel,
                bg_stroke: Stroke::new(1.0, Color32::from_gray(40)),
                fg_stroke: Stroke::new(1.0, text_secondary),
                rounding: Rounding::same(4.0),
                expansion: 0.0,
            },
            inactive: egui::style::WidgetVisuals {
                bg_fill: Color32::from_rgb(40, 45, 55),
                weak_bg_fill: Color32::from_rgb(40, 45, 55),
                bg_stroke: Stroke::new(1.0, Color32::from_gray(50)),
                fg_stroke: Stroke::new(1.0, text_primary),
                rounding: Rounding::same(4.0),
                expansion: 0.0,
            },
            hovered: egui::style::WidgetVisuals {
                bg_fill: Color32::from_rgb(50, 55, 65),
                weak_bg_fill: Color32::from_rgb(50, 55, 65),
                bg_stroke: Stroke::new(1.0, accent),
                fg_stroke: Stroke::new(1.5, text_primary),
                rounding: Rounding::same(4.0),
                expansion: 1.0,
            },
            active: egui::style::WidgetVisuals {
                bg_fill: accent,
                weak_bg_fill: accent,
                bg_stroke: Stroke::new(1.0, accent_hover),
                fg_stroke: Stroke::new(2.0, Color32::WHITE),
                rounding: Rounding::same(4.0),
                expansion: 1.0,
            },
            open: egui::style::WidgetVisuals {
                bg_fill: bg_panel,
                weak_bg_fill: bg_panel,
                bg_stroke: Stroke::new(1.0, Color32::from_gray(60)),
                fg_stroke: Stroke::new(1.0, text_primary),
                rounding: Rounding::same(4.0),
                expansion: 0.0,
            },
        },
        
        // Selection colors
        selection: egui::style::Selection {
            bg_fill: accent.linear_multiply(0.4),
            stroke: Stroke::new(1.0, accent),
        },
        
        // Hyperlink color
        hyperlink_color: accent,
        
        // Code background
        code_bg_color: Color32::from_rgb(25, 30, 38),
        
        // Misc
        warn_fg_color: Color32::from_rgb(255, 200, 88),
        error_fg_color: error,
        
        ..Default::default()
    };
    
    // Configure spacing
    style.spacing.item_spacing = egui::vec2(8.0, 8.0);
    style.spacing.button_padding = egui::vec2(12.0, 8.0);
    style.spacing.menu_margin = Margin::same(8.0);
    style.spacing.indent = 20.0;
    
    // Configure fonts
    let fonts = egui::FontDefinitions::default();
    
    // You can load custom fonts here if needed
    // fonts.font_data.insert(
    //     "my_font".to_owned(),
    //     egui::FontData::from_static(include_bytes!("../assets/fonts/MyFont.ttf")),
    // );
    
    // Configure text styles
    let mut text_styles = BTreeMap::new();
    text_styles.insert(
        TextStyle::Small,
        FontId::new(12.0, FontFamily::Proportional),
    );
    text_styles.insert(
        TextStyle::Body,
        FontId::new(14.0, FontFamily::Proportional),
    );
    text_styles.insert(
        TextStyle::Button,
        FontId::new(14.0, FontFamily::Proportional),
    );
    text_styles.insert(
        TextStyle::Heading,
        FontId::new(20.0, FontFamily::Proportional),
    );
    text_styles.insert(
        TextStyle::Monospace,
        FontId::new(14.0, FontFamily::Monospace),
    );
    
    style.text_styles = text_styles;
    
    // Apply the style
    ctx.set_style(style);
    ctx.set_fonts(fonts);
}

// Custom button style
pub fn styled_button(ui: &mut egui::Ui, text: &str) -> egui::Response {
    let button = egui::Button::new(text)
        .fill(Color32::from_rgb(88, 166, 255))
        .rounding(Rounding::same(6.0));
    
    ui.add_sized([120.0, 40.0], button)
}

// Custom text input style
pub fn styled_text_input(ui: &mut egui::Ui, text: &mut String, hint: &str) -> egui::Response {
    ui.visuals_mut().override_text_color = Some(Color32::from_gray(200));
    
    let response = ui.add(
        egui::TextEdit::singleline(text)
            .desired_width(200.0)
            .hint_text(hint)
            .font(egui::TextStyle::Body)
    );
    
    // Add custom border on focus
    if response.has_focus() {
        ui.painter().rect_stroke(
            response.rect,
            Rounding::same(4.0),
            Stroke::new(2.0, Color32::from_rgb(88, 166, 255)),
        );
    }
    
    response
}