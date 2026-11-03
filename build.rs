//! Встраивает иконку приложения в .exe на Windows.
//! Если ресурсный компилятор недоступен — не фейлим сборку, только предупреждаем.

fn main() {
    #[cfg(windows)]
    {
        let icon = "assets/kore.ico";
        println!("cargo:rerun-if-changed={icon}");
        let mut res = winresource::WindowsResource::new();
        res.set_icon(icon);
        if let Err(e) = res.compile() {
            println!("cargo:warning=иконка не встроена в exe: {e}");
        }
    }
}
