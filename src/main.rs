//! KORE: сторож CPU, графики и оперативная память. Настройки сторожа лежат в папке guard рядом с exe.

// GUI-приложение: не открываем консольное окно в Windows (ни в debug, ни в release).
#![windows_subsystem = "windows"]

fn main() -> eframe::Result<()> {
    kore::run(kore::Settings::default(), Box::new(kore::NoExtra))
}
