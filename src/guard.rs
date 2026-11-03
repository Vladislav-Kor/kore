//! Сторож CPU: guard.conf правим мы, guard.status пишет сторож, журнал только читаем.

use std::fs;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::PathBuf;
use std::time::Duration;

use crate::admin;

/// Статус старше этого значит, что сторож молчит.
const STATUS_FRESH: Duration = Duration::from_secs(10);

/// Системные процессы, зашитые в kore-guard.ps1: файл настроек их не снимает.
pub const SYSTEM_LOCKED: [&str; 5] = ["svchost", "dwm", "explorer", "MsMpEng", "System"];

const HEADER: &str = "# Настройки сторожа CPU. Правит пульт KORE, сторож перечитывает файл на ходу.\n\
                      # Системные процессы защищены в коде сторожа: убрать их отсюда нельзя.\n";

/// Где лежит сторож и кого он не режет никогда.
#[derive(Debug, Clone)]
pub struct GuardSetup {
    pub dir: PathBuf,
    /// Защищены в коде сторожа. Пульт показывает их с замком и не даёт убрать.
    pub locked: Vec<String>,
}

impl Default for GuardSetup {
    fn default() -> Self {
        GuardSetup { dir: default_dir(), locked: SYSTEM_LOCKED.map(String::from).to_vec() }
    }
}

/// Папка guard рядом с exe. В debug-сборке без такой папки берём её из исходников.
fn default_dir() -> PathBuf {
    let beside = std::env::current_exe().ok().and_then(|exe| exe.parent().map(|dir| dir.join("guard")));
    #[cfg(debug_assertions)]
    if !beside.as_ref().is_some_and(|dir| dir.exists()) {
        return std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("guard");
    }
    beside.unwrap_or_else(|| PathBuf::from("guard"))
}

impl GuardSetup {
    fn path(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }

    pub fn is_locked(&self, name: &str) -> bool {
        self.locked.iter().any(|l| l.eq_ignore_ascii_case(name))
    }

    pub fn load(&self) -> GuardConf {
        fs::read_to_string(self.path("guard.conf"))
            .map(|text| GuardConf::parse(&text))
            .unwrap_or_default()
    }

    /// Через временный файл: сторож не прочитает половину записи.
    pub fn save(&self, conf: &GuardConf) -> io::Result<()> {
        let tmp = self.path("guard.conf.tmp");
        fs::write(&tmp, conf.render())?;
        fs::rename(&tmp, self.path("guard.conf"))
    }

    /// Свежий статус работающего сторожа; None, если сторож молчит.
    pub fn status(&self) -> Option<GuardStatus> {
        let file = self.path("guard.status");
        let age = fs::metadata(&file).ok()?.modified().ok()?.elapsed().ok()?;
        if age > STATUS_FRESH {
            return None;
        }
        GuardStatus::parse(&fs::read_to_string(file).ok()?)
    }

    /// Последние строки журнала. Журнал большой, поэтому читаем только его хвост.
    pub fn log_tail(&self, count: usize) -> Vec<String> {
        let read = || -> io::Result<String> {
            let mut file = fs::File::open(self.path("guard.log"))?;
            let len = file.metadata()?.len();
            file.seek(SeekFrom::Start(len.saturating_sub(6000)))?;
            let mut bytes = Vec::new();
            file.read_to_end(&mut bytes)?;
            Ok(String::from_utf8_lossy(&bytes).into_owned())
        };
        let text = read().unwrap_or_default();
        let lines: Vec<&str> = text.lines().skip(1).collect();
        let start = lines.len().saturating_sub(count);
        lines[start..].iter().map(|l| l.to_string()).collect()
    }

    /// install-guard.ps1 сам снимает потолки старого сторожа и запускает нового.
    pub fn install(&self) -> io::Result<()> {
        admin::run_elevated(&self.path("install-guard.ps1").to_string_lossy(), &[])
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct GuardConf {
    pub enabled: bool,
    /// Предел общей загрузки CPU, %.
    pub limit: u32,
    /// Ниже этого потолка процесс не режем, % всего CPU.
    pub min_cap: u32,
    pub protect_foreground: bool,
    /// Имена процессов без .exe.
    pub exempt: Vec<String>,
    /// Части пути: процессы из этих папок не трогаем (игры).
    pub paths: Vec<String>,
}

impl Default for GuardConf {
    fn default() -> Self {
        GuardConf {
            enabled: true,
            limit: 75,
            min_cap: 8,
            protect_foreground: true,
            exempt: ["brave", "chrome", "msedge", "firefox", "opera", "vivaldi", "browser", "yandex"]
                .map(String::from)
                .to_vec(),
            paths: [r"\steamapps\", r"\Riot Games\", r"\Epic Games\", r"\GOG Games\"].map(String::from).to_vec(),
        }
    }
}

fn split_list(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect()
}

impl GuardConf {
    /// Неизвестные ключи и нечитаемые числа пропускаем: остаётся значение по умолчанию.
    pub fn parse(text: &str) -> GuardConf {
        let mut conf = GuardConf::default();
        for line in text.lines() {
            let line = line.trim();
            if line.starts_with('#') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else { continue };
            let value = value.trim();
            match key.trim() {
                "enabled" => conf.enabled = value == "1",
                "protect_foreground" => conf.protect_foreground = value == "1",
                "limit" => conf.limit = value.parse().unwrap_or(conf.limit),
                "min_cap" => conf.min_cap = value.parse().unwrap_or(conf.min_cap),
                "exempt" => conf.exempt = split_list(value),
                "protected_paths" => conf.paths = split_list(value),
                _ => {}
            }
        }
        conf
    }

    pub fn render(&self) -> String {
        format!(
            "{HEADER}enabled={}\nlimit={}\nmin_cap={}\nprotect_foreground={}\nexempt={}\nprotected_paths={}\n",
            self.enabled as u8,
            self.limit,
            self.min_cap,
            self.protect_foreground as u8,
            self.exempt.join(","),
            self.paths.join(","),
        )
    }

    pub fn add_exempt(&mut self, raw: &str, setup: &GuardSetup) -> Result<(), String> {
        let trimmed = raw.trim();
        let name = trimmed
            .strip_suffix(".exe")
            .or_else(|| trimmed.strip_suffix(".EXE"))
            .unwrap_or(trimmed);
        if name.is_empty() || name.contains([',', '=', '\n', '\r', '\\', '/']) {
            return Err("Нужно имя процесса, например Telegram".into());
        }
        if setup.is_locked(name) {
            return Err(format!("{name} и так защищён навсегда"));
        }
        if self.exempt.iter().any(|e| e.eq_ignore_ascii_case(name)) {
            return Err(format!("{name} уже в списке"));
        }
        self.exempt.push(name.to_string());
        Ok(())
    }

    pub fn add_path(&mut self, raw: &str) -> Result<(), String> {
        let part = raw.trim();
        if part.len() < 3 || part.contains([',', '=', '\n', '\r']) {
            return Err(r"Нужна часть пути, например \Games\".into());
        }
        if self.paths.iter().any(|p| p.eq_ignore_ascii_case(part)) {
            return Err("Такая папка уже в списке".into());
        }
        self.paths.push(part.to_string());
        Ok(())
    }

    /// Защищённые навсегда имена не убираются никогда.
    pub fn remove_exempt(&mut self, name: &str, setup: &GuardSetup) -> bool {
        if setup.is_locked(name) {
            return false;
        }
        let before = self.exempt.len();
        self.exempt.retain(|e| e != name);
        self.exempt.len() != before
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct GuardStatus {
    pub enabled: bool,
    pub limit: u32,
    pub total: u32,
    pub peak: u32,
    /// 0 — сторож сидит в служебной сессии и не видит активное окно.
    pub session: u32,
    pub foreground: String,
    /// Имя процесса и потолок в % всего CPU.
    pub capped: Vec<(String, u32)>,
}

impl GuardStatus {
    /// None для файла без отметки версии: такой файл писал не этот сторож.
    pub fn parse(text: &str) -> Option<GuardStatus> {
        let mut status = GuardStatus {
            enabled: false,
            limit: 0,
            total: 0,
            peak: 0,
            session: 0,
            foreground: String::new(),
            capped: Vec::new(),
        };
        let mut versioned = false;
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else { continue };
            match key {
                "version" => versioned = true,
                "enabled" => status.enabled = value == "1",
                "limit" => status.limit = value.parse().unwrap_or(0),
                "total" => status.total = value.parse().unwrap_or(0),
                "peak" => status.peak = value.parse().unwrap_or(0),
                "session" => status.session = value.parse().unwrap_or(0),
                "foreground" => status.foreground = value.to_string(),
                "capped" => {
                    status.capped = value
                        .split(';')
                        .filter_map(|item| {
                            let (name, cap) = item.rsplit_once(' ')?;
                            Some((name.to_string(), cap.parse().ok()?))
                        })
                        .collect()
                }
                _ => {}
            }
        }
        versioned.then_some(status)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_then_parse_keeps_everything() {
        let conf = GuardConf {
            enabled: false,
            limit: 80,
            min_cap: 12,
            protect_foreground: false,
            exempt: vec!["brave".into(), "Telegram".into()],
            paths: vec![r"\Games\".into()],
        };
        assert_eq!(GuardConf::parse(&conf.render()), conf);
    }

    #[test]
    fn parse_keeps_defaults_for_broken_numbers() {
        let conf = GuardConf::parse("limit=abc\nmin_cap=\nstrange line\n");
        assert_eq!(conf.limit, 75);
        assert_eq!(conf.min_cap, 8);
    }

    #[test]
    fn locked_names_can_never_be_removed_or_listed_as_ordinary() {
        let setup = GuardSetup { dir: PathBuf::new(), locked: vec!["svchost".into(), "special".into()] };
        let mut conf = GuardConf::default();
        conf.exempt.push("special".into());
        assert!(!conf.remove_exempt("special", &setup));
        assert!(!conf.remove_exempt("SPECIAL", &setup));
        assert!(conf.exempt.iter().any(|e| e == "special"));
        assert!(conf.add_exempt("special.exe", &setup).is_err());
        assert!(conf.add_exempt("svchost", &setup).is_err());
        assert!(conf.remove_exempt("brave", &setup));
    }

    #[test]
    fn add_exempt_cleans_the_name_and_refuses_garbage() {
        let setup = GuardSetup::default();
        let mut conf = GuardConf::default();
        conf.add_exempt(" Telegram.exe ", &setup).unwrap();
        assert!(conf.exempt.iter().any(|e| e == "Telegram"));
        assert!(conf.add_exempt("telegram", &setup).is_err());
        assert!(conf.add_exempt("a,b", &setup).is_err());
        assert!(conf.add_exempt("a=b", &setup).is_err());
        assert!(conf.add_exempt("  ", &setup).is_err());
    }

    #[test]
    fn add_path_refuses_separators_and_duplicates() {
        let mut conf = GuardConf::default();
        conf.add_path(r"\My Games\").unwrap();
        assert!(conf.add_path(r"\my games\").is_err());
        assert!(conf.add_path("a,b,c").is_err());
    }

    #[test]
    fn status_parses_capped_names_with_spaces() {
        let text = "time=x\nversion=2\nenabled=1\nlimit=75\ntotal=44\npeak=100\nsession=1\n\
                    foreground=brave\ncapped=Docker Desktop 8;python 18\n";
        let status = GuardStatus::parse(text).unwrap();
        assert_eq!(status.total, 44);
        assert_eq!(status.foreground, "brave");
        assert_eq!(
            status.capped,
            vec![("Docker Desktop".to_string(), 8), ("python".to_string(), 18)]
        );
    }

    #[test]
    fn status_without_version_is_refused() {
        assert_eq!(GuardStatus::parse("total=44\n"), None);
    }

    #[test]
    fn save_then_load_through_the_folder() {
        let dir = std::env::temp_dir().join(format!("kore-guard-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let setup = GuardSetup { dir: dir.clone(), locked: Vec::new() };
        let mut conf = GuardConf::default();
        conf.limit = 66;
        setup.save(&conf).unwrap();
        assert_eq!(setup.load(), conf);
        fs::remove_dir_all(dir).unwrap();
    }
}
