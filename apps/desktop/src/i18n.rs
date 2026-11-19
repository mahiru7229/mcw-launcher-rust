use crate::theme::ThemeMode;
use mcw_storage::{atomic_write_text, LauncherPaths};
use serde::{Deserialize, Serialize};
use std::fs;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    En,
    Vi,
}

impl Language {
    pub fn code(self) -> &'static str {
        match self {
            Self::En => "en-US",
            Self::Vi => "vi-VN",
        }
    }

    pub fn from_code(code: &str) -> Self {
        if code.trim().to_ascii_lowercase().starts_with("vi") {
            Self::Vi
        } else {
            Self::En
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LauncherPrefs {
    #[serde(default = "default_theme")]
    pub theme: String,
    #[serde(default = "default_language")]
    pub language: String,
}

fn default_theme() -> String {
    "dark".to_string()
}

fn default_language() -> String {
    "en-US".to_string()
}

impl Default for LauncherPrefs {
    fn default() -> Self {
        Self {
            theme: default_theme(),
            language: default_language(),
        }
    }
}

impl LauncherPrefs {
    pub fn load(paths: &LauncherPaths) -> Self {
        let path = paths.config_root.join("launcher_settings.json");
        fs::read_to_string(path)
            .ok()
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, paths: &LauncherPaths) {
        let path = paths.config_root.join("launcher_settings.json");
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = atomic_write_text(&path, &json);
        }
    }

    pub fn theme_mode(&self) -> ThemeMode {
        if self.theme.eq_ignore_ascii_case("light") {
            ThemeMode::Light
        } else {
            ThemeMode::Dark
        }
    }

    pub fn language_enum(&self) -> Language {
        Language::from_code(&self.language)
    }
}

/// Concise EN/VI dictionary matching MCW Launcher's standard Vietnamese terminology.
pub fn tr(lang: Language, key: &'static str) -> &'static str {
    match lang {
        Language::En => key,
        Language::Vi => match key {
            "Launcher Settings" => "Cài đặt Launcher",
            "Appearance" => "Giao diện",
            "Language" => "Ngôn ngữ",
            "Select the launcher color theme (Default: Dark)." => {
                "Chọn giao diện màu cho Launcher (Mặc định: Tối)."
            }
            "Select the interface language for MCW Launcher." => {
                "Chọn ngôn ngữ hiển thị cho MCW Launcher."
            }
            "Dark (Default)" => "Tối (Mặc định)",
            "Light" => "Sáng",
            "Sign in / Add Account" => "Đăng nhập / Thêm tài khoản",
            "+ New Instance / Modpack" => "+ Tạo Instance / Modpack",
            "No instances yet" => "Chưa có Instance nào",
            "Create an instance to get started." => "Tạo một Instance mới để bắt đầu.",
            "Launch" => "Khởi chạy",
            "Stop Game" => "Dừng Game",
            "Preparing..." => "Đang chuẩn bị...",
            "Overview" => "Tổng quan",
            "Mods" => "Mods",
            "Packs" => "Gói tài nguyên",
            "Worlds" => "Thế giới",
            "Screenshots" => "Ảnh chụp",
            "Backups" => "Sao lưu",
            "Logs" => "Nhật ký",
            "Settings" => "Cài đặt",
            "Open .minecraft" => "Mở .minecraft",
            "Rename" => "Đổi tên",
            "Repair Files" => "Sửa chữa tệp",
            "Delete" => "Xóa",
            "Resource Packs" => "Gói tài nguyên",
            "Shader Packs" => "Gói Shader",
            "Open Folder" => "Mở thư mục",
            "Refresh" => "Làm mới",
            "Search" => "Tìm kiếm",
            "Install" => "Cài đặt",
            "Installing..." => "Đang cài...",
            "Remove" => "Gỡ bỏ",
            "Close" => "Đóng",
            "Cancel" => "Hủy",
            "Save Settings" => "Lưu cài đặt",
            "Quick Play" => "Chơi nhanh",
            "Duplicate" => "Nhân bản",
            "Upload to mclo.gs" => "Tải lên mclo.gs",
            "Open Manual Downloads" => "Mở tải thủ công",
            "Scan Downloads Now" => "Quét thư mục Tải xuống",
            "INCOMPLETE MODPACK" => "MODPACK CHƯA HOÀN TẤT",
            "ACTIVE INSTALLS" => "ĐANG CÀI ĐẶT",
            "Export .mrpack" => "Xuất .mrpack",
            _ => key,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_en_and_vi_translations() {
        assert_eq!(tr(Language::En, "Launch"), "Launch");
        assert_eq!(tr(Language::Vi, "Launch"), "Khởi chạy");
        assert_eq!(tr(Language::Vi, "Launcher Settings"), "Cài đặt Launcher");
        assert_eq!(Language::from_code("vi-VN"), Language::Vi);
        assert_eq!(Language::from_code("en-US"), Language::En);
    }
}
