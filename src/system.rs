//! Windows: внешние программы без окна консоли, загрузка CPU и GPU.

use std::os::windows::process::CommandExt;
use std::process::Command;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[link(name = "kernel32")]
extern "system" {
    fn GetSystemTimes(idle: *mut u64, kernel: *mut u64, user: *mut u64) -> i32;
}

/// Как `Command::new`, но без чёрного окна консоли: nvidia-smi зовём каждые 8 секунд.
pub fn hidden_command(program: &str) -> Command {
    let mut command = Command::new(program);
    command.creation_flags(CREATE_NO_WINDOW);
    command
}

/// Наибольшая загрузка среди видеокарт NVIDIA, %. None — карты NVIDIA нет или nvidia-smi не ответил.
pub fn gpu_load() -> Option<f32> {
    let output = hidden_command("nvidia-smi")
        .args(["--query-gpu=utilization.gpu", "--format=csv,noheader,nounits"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.trim().parse::<f32>().ok())
        .reduce(f32::max)
}

/// Загрузка всего CPU между двумя замерами, по счётчикам времени Windows.
#[derive(Debug, Default)]
pub struct CpuMeter {
    /// Простой и всё время с прошлого замера, в сотнях наносекунд.
    last: Option<(u64, u64)>,
}

impl CpuMeter {
    /// Загрузка с прошлого вызова, %. Первый вызов только запоминает счётчики и даёт 0.
    pub fn read(&mut self) -> f32 {
        let (mut idle, mut kernel, mut user) = (0u64, 0u64, 0u64);
        // SAFETY: три u64 по размеру совпадают с FILETIME, вызов только пишет в них.
        if unsafe { GetSystemTimes(&mut idle, &mut kernel, &mut user) } == 0 {
            return 0.0;
        }
        // Время ядра уже включает простой.
        let total = kernel + user;
        let load = match self.last {
            Some((last_idle, last_total)) if total > last_total => {
                let spent = (total - last_total) as f32;
                let busy = spent - idle.saturating_sub(last_idle) as f32;
                (busy / spent * 100.0).clamp(0.0, 100.0)
            }
            _ => 0.0,
        };
        self.last = Some((idle, total));
        load
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_load_stays_in_range() {
        let mut meter = CpuMeter::default();
        assert_eq!(meter.read(), 0.0);
        std::thread::sleep(std::time::Duration::from_millis(200));
        let load = meter.read();
        assert!((0.0..=100.0).contains(&load));
    }
}
