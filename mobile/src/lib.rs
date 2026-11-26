extern crate gpui_mobile;

use std::sync::atomic::{AtomicU64, Ordering};

pub use vkarma_core::live_md;
pub mod theme;
pub mod ui;
pub mod vault;

static RESUME_EPOCH: AtomicU64 = AtomicU64::new(0);

pub fn android_resume_epoch() -> u64 {
    RESUME_EPOCH.load(Ordering::SeqCst)
}

#[cfg(target_os = "android")]
pub fn has_all_files_access() -> bool {
    use gpui_mobile::android::jni as gpui_jni;
    match gpui_jni::with_env(|env| {
        let activity = gpui_jni::activity(env)?;
        env.call_method(
            &activity,
            jni::jni_str!("gpuiHasAllFilesAccess"),
            jni::jni_sig!("()Z"),
            &[],
        )
        .and_then(|v| v.z())
        .map_err(|e| {
            env.exception_clear();
            e.to_string()
        })
    }) {
        Ok(val) => val,
        Err(err) => {
            log::warn!("VKarma Mobile: has_all_files_access failed: {err}");
            false
        }
    }
}

#[cfg(not(target_os = "android"))]
pub fn has_all_files_access() -> bool {
    true
}

#[cfg(target_os = "android")]
pub fn request_all_files_access() {
    use gpui_mobile::android::jni as gpui_jni;
    if let Err(err) = gpui_jni::with_env(|env| {
        let activity = gpui_jni::activity(env)?;
        env.call_method(
            &activity,
            jni::jni_str!("gpuiRequestAllFilesAccess"),
            jni::jni_sig!("()V"),
            &[],
        )
        .map(drop)
        .map_err(|e| {
            env.exception_clear();
            e.to_string()
        })
    }) {
        log::warn!("VKarma Mobile: request_all_files_access failed: {err}");
    }
}

#[cfg(not(target_os = "android"))]
pub fn request_all_files_access() {}

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub extern "C" fn Java_dev_gpui_mobile_GpuiActivity_nativeOnResume(
    _env: *mut std::ffi::c_void,
    _class: *mut std::ffi::c_void,
) {
    RESUME_EPOCH.fetch_add(1, Ordering::SeqCst);
}

/// Minimize the Android task cleanly when Back is pressed at the root screen.
#[cfg(target_os = "android")]
pub fn minimize_android_app() {
    use gpui_mobile::android::jni as gpui_jni;
    if let Err(err) = gpui_jni::with_env(|env| {
        let activity = gpui_jni::activity(env)?;
        env.call_method(
            &activity,
            jni::jni_str!("gpuiMoveTaskToBack"),
            jni::jni_sig!("()V"),
            &[],
        )
        .map(drop)
        .map_err(|e| {
            env.exception_clear();
            e.to_string()
        })
    }) {
        log::warn!("VKarma Mobile: minimize_android_app failed: {err}");
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AndroidImeMode {
    Prose = 0,
    Title = 1,
    Search = 2,
    Plain = 3,
}

pub fn ime_surrounding_context(text: &str, cursor_byte: usize) -> (String, String) {
    let mut cur = cursor_byte.min(text.len());
    while cur > 0 && !text.is_char_boundary(cur) {
        cur -= 1;
    }
    let before_slice = &text[..cur];
    let before_start = before_slice
        .char_indices()
        .rev()
        .nth(159)
        .map(|(ix, _)| ix)
        .unwrap_or(0);
    let after_slice = &text[cur..];
    let after_end = after_slice
        .char_indices()
        .nth(64)
        .map(|(ix, _)| ix)
        .unwrap_or(after_slice.len());
    (
        before_slice[before_start..].to_string(),
        after_slice[..after_end].to_string(),
    )
}

#[cfg(target_os = "android")]
pub fn configure_android_ime(
    mode: AndroidImeMode,
    before: &str,
    after: &str,
    restart_if_open: bool,
) {
    use gpui_mobile::android::jni as gpui_jni;
    use jni::objects::JValue;
    if let Err(err) = gpui_jni::with_env(|env| {
        let activity = gpui_jni::activity(env)?;
        let j_before = env.new_string(before).map_err(|e| e.to_string())?;
        let j_after = env.new_string(after).map_err(|e| e.to_string())?;
        env.call_method(
            &activity,
            jni::jni_str!("gpuiConfigureIme"),
            jni::jni_sig!("(ILjava/lang/String;Ljava/lang/String;Z)V"),
            &[
                JValue::Int(mode as i32),
                JValue::Object(&j_before),
                JValue::Object(&j_after),
                JValue::Bool(restart_if_open),
            ],
        )
        .map(drop)
        .map_err(|e| {
            env.exception_clear();
            e.to_string()
        })
    }) {
        log::warn!("VKarma Mobile: configure_android_ime failed: {err}");
    }
}

#[cfg(not(target_os = "android"))]
pub fn configure_android_ime(
    _mode: AndroidImeMode,
    _before: &str,
    _after: &str,
    _restart_if_open: bool,
) {
}

#[cfg(target_os = "android")]
pub fn sync_android_ime_context(before: &str, after: &str) {
    use gpui_mobile::android::jni as gpui_jni;
    use jni::objects::JValue;
    if let Err(err) = gpui_jni::with_env(|env| {
        let activity = gpui_jni::activity(env)?;
        let j_before = env.new_string(before).map_err(|e| e.to_string())?;
        let j_after = env.new_string(after).map_err(|e| e.to_string())?;
        env.call_method(
            &activity,
            jni::jni_str!("gpuiSyncImeContext"),
            jni::jni_sig!("(Ljava/lang/String;Ljava/lang/String;)V"),
            &[JValue::Object(&j_before), JValue::Object(&j_after)],
        )
        .map(drop)
        .map_err(|e| {
            env.exception_clear();
            e.to_string()
        })
    }) {
        log::warn!("VKarma Mobile: sync_android_ime_context failed: {err}");
    }
}

#[cfg(not(target_os = "android"))]
pub fn sync_android_ime_context(_before: &str, _after: &str) {}

#[cfg(not(target_os = "android"))]
pub fn minimize_android_app() {}

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub extern "C" fn Java_dev_gpui_mobile_GpuiActivity_nativeOnBackPressed(
    _env: *mut std::ffi::c_void,
    _class: *mut std::ffi::c_void,
) {
    use gpui_mobile::android::jni as gpui_jni;
    log::info!("VKarma Mobile: nativeOnBackPressed received");
    if let Some(platform) = gpui_jni::platform() {
        let dispatcher = platform.foreground_executor();
        dispatcher.dispatch_on_main_thread(move || {
            if let Some(platform) = gpui_jni::platform()
                && let Some(win) = platform.primary_window()
            {
                win.handle_key_event(gpui_mobile::android::AndroidKeyEvent {
                    key_code: gpui_mobile::android::AKEYCODE_BACK,
                    action: 0,
                    meta_state: 0,
                    unicode_char: 0,
                });
            }
        });
    }
}

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub fn android_main(app: android_activity::AndroidApp) {
    use gpui::{App, AppContext as _, WindowOptions};
    use gpui_mobile::android::jni;

    let log_level = if cfg!(debug_assertions) {
        log::LevelFilter::Info
    } else {
        log::LevelFilter::Warn
    };
    android_logger::init_once(
        android_logger::Config::default()
            .with_max_level(log_level)
            .with_tag("vkarma-mobile"),
    );

    jni::install_panic_hook();
    log::info!("VKarma Mobile android_main starting");

    if jni::platform().is_some() {
        log::warn!(
            "VKarma Mobile: android_main re-entered after PLATFORM initialization; exiting cleanly for fresh process"
        );
        std::process::exit(0);
    }

    let _platform = jni::init_platform(&app);
    let Some(shared) = jni::shared_platform() else {
        log::error!("VKarma Mobile: shared_platform() returned None");
        return;
    };

    gpui::Application::with_platform(shared.into_rc())
        .with_assets(gpui_kit::assets::Assets)
        .run(|cx: &mut App| {
        log::info!("VKarma Mobile GPUI Application::run callback");
        gpui_kit::init(cx);
        theme::apply_mobile_theme(cx);
        gpui_mobile::set_system_chrome(&gpui_mobile::SystemChromeStyle {
            status_bar_color: Some(theme::kd::BG_DEEP),
            status_bar_style: gpui_mobile::StatusBarContentStyle::Light,
            navigation_bar_color: Some(theme::kd::BG_DEEP),
        });

        if let Err(err) = cx.open_window(
            WindowOptions {
                window_bounds: None,
                ..Default::default()
            },
            |window, cx| {
                let root_view = cx.new(|cx| ui::VKarmaMobileApp::new(window, cx));
                cx.new(|cx| gpui_kit::component::Root::new(root_view, window, cx))
            },
        ) {
            log::error!("VKarma Mobile open_window failed: {err:#}");
            return;
        }

        cx.activate(true);
        log::info!("VKarma Mobile window active");
    });

    log::info!("VKarma Mobile android_main event loop finished");
}

#[cfg(test)]
mod tests {
    use super::ime_surrounding_context;

    #[test]
    fn ime_surrounding_context_slices_utf8_safely() {
        assert_eq!(
            ime_surrounding_context("Hello мир", 6),
            ("Hello ".to_string(), "мир".to_string())
        );
        assert_eq!(
            ime_surrounding_context("Hello мир", 7),
            ("Hello ".to_string(), "мир".to_string())
        );
        let prefix = "а".repeat(200);
        let suffix = "😀".repeat(80);
        let combined = format!("{prefix}{suffix}");
        let (b, a) = ime_surrounding_context(&combined, prefix.len());
        assert_eq!(b.chars().count(), 160);
        assert_eq!(a.chars().count(), 64);
    }
}
