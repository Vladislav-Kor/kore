//! Запуск PowerShell-скрипта с правами администратора: Windows спросит пользователя через UAC.

use crate::system::hidden_command;

/// Скрипт и аргументы задаёт само приложение, не пользователь.
pub fn run_elevated(script: &str, args: &[&str]) -> std::io::Result<()> {
    let mut list = vec![
        "'-NoProfile'".to_string(),
        "'-ExecutionPolicy'".to_string(),
        "'Bypass'".to_string(),
        "'-File'".to_string(),
        format!("'\"{script}\"'"),
    ];
    list.extend(args.iter().map(|a| format!("'{a}'")));
    let command = format!(
        "Start-Process powershell.exe -Verb RunAs -ArgumentList {}",
        list.join(",")
    );
    hidden_command("powershell")
        .args(["-NoProfile", "-Command", &command])
        .spawn()
        .map(|_| ())
}
