use egui::{Color32, Context, Stroke, Visuals};

// Colors from index.html
pub const COLOR_PRIMARY: Color32 = Color32::from_rgb(0x3b, 0x82, 0xf6); // #3b82f6
pub const COLOR_BG: Color32 = Color32::from_rgb(0x00, 0x00, 0x00); // #000000
pub const COLOR_CARD_BG: Color32 = Color32::from_rgb(0x11, 0x18, 0x27); // #111827
pub const COLOR_TEXT: Color32 = Color32::from_rgb(0xf9, 0xfa, 0xfb); // #f9fafb
pub const COLOR_BUTTON: Color32 = Color32::from_rgb(0x37, 0x41, 0x51); // #374151
pub const COLOR_DATA: Color32 = Color32::from_rgb(0x10, 0xb9, 0x81); // #10b981
pub const COLOR_LABEL: Color32 = Color32::from_rgb(0x6b, 0x72, 0x80); // #6b7280 (Gray 500)
pub const COLOR_FOOTER_BG: Color32 = Color32::from_rgba_premultiplied(0, 0, 0, 100); // rgba(0,0,0,0.4) approx

// Brand Colors for Ombre Title
pub const COLOR_TITLE_START: Color32 = Color32::from_rgb(0x3b, 0x82, 0xf6); // #3b82f6 (Blue)
pub const COLOR_TITLE_END: Color32 = Color32::from_rgb(0x10, 0xb9, 0x81); // #10b981 (Green)

pub fn configure_style(ctx: &Context) {
    let mut visuals = Visuals::dark();

    // Backgrounds
    visuals.panel_fill = COLOR_BG;
    visuals.window_fill = COLOR_CARD_BG;
    visuals.window_stroke = Stroke::new(1.0, Color32::from_gray(60));

    // Text
    visuals.override_text_color = Some(COLOR_TEXT);
    visuals.hyperlink_color = COLOR_PRIMARY;

    // Interactive Widgets (Buttons, Inputs)
    visuals.widgets.noninteractive.bg_fill = COLOR_CARD_BG;
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, COLOR_TEXT);

    // Inactive (Default state)
    visuals.widgets.inactive.bg_fill = COLOR_BUTTON;
    visuals.widgets.inactive.weak_bg_fill = COLOR_BUTTON;
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, COLOR_TEXT);
    visuals.widgets.inactive.rounding = egui::Rounding::same(4.0);

    // Hovered
    visuals.widgets.hovered.bg_fill = Color32::from_rgb(0x4b, 0x55, 0x63); // Lighter gray
    visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(0x4b, 0x55, 0x63);
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, Color32::WHITE);
    visuals.widgets.hovered.rounding = egui::Rounding::same(4.0);

    // Active (Clicked)
    visuals.widgets.active.bg_fill = COLOR_PRIMARY;
    visuals.widgets.active.weak_bg_fill = COLOR_PRIMARY;
    visuals.widgets.active.fg_stroke = Stroke::new(1.0, Color32::WHITE);
    visuals.widgets.active.rounding = egui::Rounding::same(4.0);

    // Selection
    visuals.selection.bg_fill = COLOR_PRIMARY;
    visuals.selection.stroke = Stroke::new(1.0, Color32::WHITE);

    ctx.set_visuals(visuals);
}
