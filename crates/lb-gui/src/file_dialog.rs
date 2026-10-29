//! Desktop dialogs; Android operations explain the experimental limitation.
#[cfg(not(target_os = "android"))]
pub use rfd::FileDialog;

#[cfg(target_os = "android")]
pub use mobile::FileDialog;

#[cfg(target_os = "android")]
mod mobile {
    use std::path::{Path, PathBuf};

    pub struct FileDialog;
    impl FileDialog {
        pub fn new() -> Self {
            Self
        }
        pub fn set_title(self, _: impl Into<String>) -> Self {
            self
        }
        pub fn set_file_name(self, _: impl Into<String>) -> Self {
            self
        }
        pub fn set_directory(self, _: impl AsRef<Path>) -> Self {
            self
        }
        pub fn add_filter(self, _: impl Into<String>, _: &[impl ToString]) -> Self {
            self
        }
        fn unavailable() {
            crate::bridge::with(|c| c.toast("Выбор и сохранение файлов пока недоступны на Android. Используйте настольный клиент.", true));
        }
        pub fn pick_file(self) -> Option<PathBuf> {
            Self::unavailable();
            None
        }
        pub fn pick_folder(self) -> Option<PathBuf> {
            Self::unavailable();
            None
        }
        pub fn save_file(self) -> Option<PathBuf> {
            Self::unavailable();
            None
        }
        pub fn pick_files(self) -> Option<Vec<PathBuf>> {
            Self::unavailable();
            None
        }
    }
}
