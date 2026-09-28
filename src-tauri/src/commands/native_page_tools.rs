use serde::{Deserialize, Serialize};

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeToolRect {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativePageToolsPacket {
    session: String,
    revision: u64,
    owner: String,
    query: String,
    edit: u64,
    placeholder: String,
    create_label: String,
    create_disabled_reason: String,
    show_add: bool,
    visible: bool,
    viewport_width: f64,
    search: NativeToolRect,
    add: NativeToolRect,
}

#[tauri::command]
pub async fn init_native_page_tools(
    window: tauri::WebviewWindow,
    session: String,
) -> Result<bool, String> {
    #[cfg(target_os = "macos")]
    {
        macos::init(window, session).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, session);
        Ok(false)
    }
}

#[tauri::command]
pub async fn update_native_page_tools(
    window: tauri::WebviewWindow,
    packet: NativePageToolsPacket,
) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        macos::update(window, packet).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, packet);
        Ok(())
    }
}

#[tauri::command]
pub async fn destroy_native_page_tools(
    window: tauri::WebviewWindow,
    session: String,
) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        macos::destroy(window, session).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, session);
        Ok(())
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use super::NativePageToolsPacket;
    use std::{
        ffi::{c_char, c_void, CStr, CString},
        sync::OnceLock,
    };
    use tauri::{Emitter, Manager};

    static APP: OnceLock<tauri::AppHandle> = OnceLock::new();
    extern "C" {
        fn ss_page_tools_init(
            view: *mut c_void,
            session: *const c_char,
            callback: extern "C" fn(*const c_char),
        );
        fn ss_page_tools_update(view: *mut c_void, json: *const c_char);
        fn ss_page_tools_destroy(view: *mut c_void, session: *const c_char);
    }

    extern "C" fn on_event(json: *const c_char) {
        if json.is_null() {
            return;
        }
        // Swift passes a NUL-terminated UTF-8 string valid for this call only.
        let bytes = unsafe { CStr::from_ptr(json) }.to_bytes();
        if let (Some(app), Ok(event)) = (
            APP.get(),
            serde_json::from_slice::<serde_json::Value>(bytes),
        ) {
            let _ = app.emit_to("main", "native-page-tools", event);
        }
    }

    async fn on_main_thread<T: Send + 'static>(
        window: tauri::WebviewWindow,
        action: impl FnOnce(*mut c_void) -> T + Send + 'static,
    ) -> Result<T, String> {
        if window.label() != "main" {
            return Err("Native page tools require the main window".into());
        }
        let (sender, receiver) = tokio::sync::oneshot::channel();
        let target = window.clone();
        window
            .run_on_main_thread(move || {
                let result = target.ns_view().map(action).map_err(|e| e.to_string());
                let _ = sender.send(result);
            })
            .map_err(|e| e.to_string())?;
        receiver.await.map_err(|e| e.to_string())?
    }

    pub async fn init(window: tauri::WebviewWindow, session: String) -> Result<bool, String> {
        APP.get_or_init(|| window.app_handle().clone());
        let session = CString::new(session).map_err(|e| e.to_string())?;
        on_main_thread(window, move |view| {
            // NSHostingView construction and all subsequent AppKit calls stay on the main thread.
            unsafe {
                ss_page_tools_init(view, session.as_ptr(), on_event);
            }
            true
        })
        .await
    }

    pub async fn update(
        window: tauri::WebviewWindow,
        packet: NativePageToolsPacket,
    ) -> Result<(), String> {
        if !packet.viewport_width.is_finite() || packet.viewport_width <= 0.0 {
            return Err("Invalid native toolbar viewport".into());
        }
        for rect in [&packet.search, &packet.add] {
            if ![rect.x, rect.y, rect.width, rect.height]
                .iter()
                .all(|v| v.is_finite())
                || rect.width < 0.0
                || rect.height < 0.0
            {
                return Err("Invalid native toolbar bounds".into());
            }
        }
        let json = CString::new(serde_json::to_string(&packet).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        on_main_thread(window, move |view| unsafe {
            ss_page_tools_update(view, json.as_ptr());
        })
        .await
    }

    pub async fn destroy(window: tauri::WebviewWindow, session: String) -> Result<(), String> {
        let session = CString::new(session).map_err(|e| e.to_string())?;
        on_main_thread(window, move |view| unsafe {
            ss_page_tools_destroy(view, session.as_ptr());
        })
        .await
    }
}
