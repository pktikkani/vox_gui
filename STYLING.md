# Styling Guide for Vox Remote Desktop

This guide explains how the GUI is styled and how you can customize it further.

## Current Theme

The application uses a modern dark theme with:
- **Background Colors**: Dark grays (#141720, #1C2028, #232832)
- **Accent Color**: Blue (#58A6FF)
- **Success Color**: Green (#58FFA6)
- **Error Color**: Red (#FF5858)
- **Text Colors**: Light grays for contrast

## How Styling Works in egui

The styling is implemented in `src/client/theme.rs` and applied when the app starts. The main components that can be styled are:

### 1. **Visuals**
- Window appearance (rounding, shadows, fills)
- Widget states (normal, hovered, active, disabled)
- Selection colors
- Text colors

### 2. **Spacing**
- Item spacing
- Button padding
- Menu margins
- Indentation

### 3. **Fonts**
- Font families
- Text sizes for different styles (Body, Heading, Button, etc.)
- Custom font loading

## Customization Examples

### Changing Colors

```rust
// In theme.rs
let accent = Color32::from_rgb(88, 166, 255);  // Blue
// Change to green:
let accent = Color32::from_rgb(88, 255, 166);  // Green
```

### Changing Widget Rounding

```rust
style.visuals.window_rounding = Rounding::same(8.0);  // Current
style.visuals.window_rounding = Rounding::same(0.0);  // Square corners
style.visuals.window_rounding = Rounding::same(16.0); // More rounded
```

### Adding Custom Fonts

```rust
// Load a custom font
fonts.font_data.insert(
    "custom_font".to_owned(),
    egui::FontData::from_static(include_bytes!("../assets/fonts/CustomFont.ttf")),
);

// Use it in text styles
fonts.families.insert(
    FontFamily::Name("custom".into()),
    vec!["custom_font".to_owned()]
);
```

### Creating Custom Widgets

The theme.rs file includes examples of custom styled widgets:

```rust
pub fn styled_button(ui: &mut egui::Ui, text: &str) -> egui::Response {
    let button = egui::Button::new(text)
        .fill(Color32::from_rgb(88, 166, 255))
        .rounding(Rounding::same(6.0));
    
    ui.add_sized([120.0, 40.0], button)
}
```

## Advanced Styling

### Custom Frames

```rust
egui::Frame::none()
    .fill(Color32::from_rgb(35, 40, 50))
    .rounding(Rounding::same(12.0))
    .inner_margin(Margin::same(30.0))
    .shadow(Shadow {
        offset: Vec2::new(0.0, 4.0),
        blur: 8.0,
        spread: 0.0,
        color: Color32::from_black_alpha(40),
    })
    .show(ui, |ui| {
        // Content here
    });
```

### Gradient Effects

While egui doesn't directly support gradients, you can simulate them:

```rust
// Draw multiple rectangles with slightly different colors
for i in 0..10 {
    let color = Color32::from_rgb(
        88 + i * 2,
        166 + i * 2,
        255,
    );
    // Draw rectangle at different positions
}
```

### Animations

```rust
// Use ctx.animate_value_with_time() for smooth transitions
let expansion = ctx.animate_value_with_time(
    button_id,
    if hovered { 1.0 } else { 0.0 },
    0.2  // Animation duration
);
```

## Theme Variations

You can create multiple themes and switch between them:

```rust
pub fn apply_light_theme(ctx: &Context) {
    // Light theme colors
}

pub fn apply_high_contrast_theme(ctx: &Context) {
    // High contrast for accessibility
}
```

## Resources

- [egui Docs on Styling](https://docs.rs/egui/latest/egui/struct.Style.html)
- [egui Color Documentation](https://docs.rs/egui/latest/egui/struct.Color32.html)
- [Example from TinyPomodoro](https://github.com/a-liashenko/TinyPomodoro)

The current implementation provides a solid foundation for a professional-looking remote desktop application. Feel free to experiment with colors, spacing, and widget styles to match your preferences!