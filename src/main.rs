#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod api;
mod app;
mod assets;
mod cloud;
mod gallery;
mod icons;
mod install;
mod settings;
mod sources;
mod system;
mod updater;
mod thumb_cache;
mod worker;

fn main() -> eframe::Result<()> {
    let argv: Vec<_> = std::env::args().collect();
    let exe = std::env::current_exe().unwrap_or_default();
    let setup = exe
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.eq_ignore_ascii_case("WaifuGallerySetup.exe"));
    if setup || argv.iter().any(|a| a == "--install") {
        if let Err(e) = install::install() {
            rfd::MessageDialog::new()
                .set_title("Установка Waifu Gallery")
                .set_description(e)
                .set_level(rfd::MessageLevel::Error)
                .show();
        }
        return Ok(());
    }
    if argv.iter().any(|a| a == "--uninstall") {
        if rfd::MessageDialog::new()
            .set_title("Удаление Waifu Gallery")
            .set_description("Удалить приложение? Картинки и настройки останутся на компьютере.")
            .set_buttons(rfd::MessageButtons::YesNo)
            .show()
            == rfd::MessageDialogResult::Yes
        {
            if let Err(e) = install::uninstall() {
                rfd::MessageDialog::new().set_description(e).show();
            }
        }
        return Ok(());
    }
    let mut args = argv.into_iter().skip(1);
    let mut base = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(std::path::Path::to_path_buf))
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    if exe.parent().is_some_and(|p| p.join(".installed").is_file()) {
        if let Some(data) = install::data_directory() {
            base = data;
        }
        #[cfg(windows)]
        if let Ok(key) = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER)
            .open_subkey_with_flags(
                "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\WaifuGallery.Desktop",
                winreg::enums::KEY_SET_VALUE,
            )
        {
            let _ = key.set_value("DisplayVersion", &env!("CARGO_PKG_VERSION"));
        }
    }
    let mut smoke = false;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--data-dir" => {
                if let Some(dir) = args.next() {
                    base = dir.into();
                }
            }
            "--smoke-test" => smoke = true,
            _ => {}
        }
    }
    if let Err(error) = std::fs::create_dir_all(&base) {
        rfd::MessageDialog::new()
            .set_title("Waifu Gallery")
            .set_description(format!("Не удалось открыть папку данных: {error}"))
            .set_level(rfd::MessageLevel::Error)
            .show();
        return Ok(());
    }
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 880.0])
            .with_min_inner_size([1040.0, 760.0])
            .with_title("Waifu Gallery")
            .with_icon({
                let image = image::load_from_memory(include_bytes!("../assets/icon.png"))
                    .expect("embedded app icon")
                    .into_rgba8();
                eframe::egui::IconData {
                    width: image.width(),
                    height: image.height(),
                    rgba: image.into_raw(),
                }
            }),
        ..Default::default()
    };
    eframe::run_native(
        "Waifu Gallery",
        options,
        Box::new(move |cc| Ok(Box::new(app::GalleryApp::new(cc, base, smoke)))),
    )
}
