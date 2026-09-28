use tauri::{Manager, Theme};

/// 让原生标题栏跟随应用主题。传 "system" 时交回给操作系统。
#[tauri::command]
pub fn set_window_theme(app: tauri::AppHandle, theme: String) -> Result<(), String> {
    let resolved = match theme.as_str() {
        "light" => Some(Theme::Light),
        "dark" => Some(Theme::Dark),
        "system" => None,
        other => return Err(format!("未知主题: {other}")),
    };
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "主窗口不存在".to_string())?;
    window
        .set_theme(resolved)
        .map_err(|e| format!("设置窗口主题失败: {e}"))
}

/// 启动期错误。前端首屏拉取，非空则渲染恢复界面而不是主界面。
#[tauri::command]
pub fn get_init_error() -> Option<String> {
    crate::init_status::get()
}

/// 应用版本，用于设置页展示
#[tauri::command]
pub fn get_app_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

pub struct WindowMaterial(pub &'static str);

#[tauri::command]
pub fn get_window_material(material: tauri::State<'_, WindowMaterial>) -> &'static str {
    material.0
}

/// Called on the main thread before showing the window. The frontend tints the
/// translucent chrome separately and keeps the reading surface opaque.
pub fn window_material(_window: &tauri::WebviewWindow) -> &'static str {
    #[cfg(target_os = "macos")]
    {
        use window_vibrancy::{
            apply_liquid_glass, apply_vibrancy, LiquidGlassOptions, NSVisualEffectMaterial,
        };
        let options = LiquidGlassOptions::default().opaque(false);
        match apply_liquid_glass(_window, options) {
            Ok(()) => return "liquid-glass",
            Err(error) => log::info!("Liquid Glass unavailable, using sidebar material: {error}"),
        }
        if apply_vibrancy(_window, NSVisualEffectMaterial::Sidebar, None, None).is_ok() {
            return "vibrancy";
        }
    }
    "solid"
}
