//! Живые показатели: фоновый поток опрашивает ПК, окно только читает снимок.

use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use eframe::egui;

use crate::guard::{GuardSetup, GuardStatus};
use crate::ram::{self, ProcessRam, RamView};
use crate::system::{self, CpuMeter};

const TICK: Duration = Duration::from_secs(2);
/// nvidia-smi и список процессов дороже остального: опрашиваем каждый четвёртый такт.
const SLOW_EVERY: u32 = 4;
const LOG_LINES: usize = 6;
/// Строк в списке «кто держит память».
const TOP_RAM: usize = 8;
/// Точек на графике: при такте 2 с это 4 минуты.
pub const HISTORY: usize = 120;

#[derive(Debug, Clone, Default)]
pub struct Live {
    pub cpu: f32,
    pub gpu: Option<f32>,
    pub cpu_history: Vec<f32>,
    pub gpu_history: Vec<f32>,
    pub ram: RamView,
    pub ram_history: Vec<f32>,
    pub top_ram: Vec<ProcessRam>,
    pub processes: usize,
    pub guard: Option<GuardStatus>,
    pub log: Vec<String>,
}

fn push_point(history: &mut Vec<f32>, value: f32) {
    if history.len() == HISTORY {
        history.remove(0);
    }
    history.push(value);
}

pub fn spawn(ctx: egui::Context, guard: GuardSetup) -> Arc<Mutex<Live>> {
    let shared = Arc::new(Mutex::new(Live::default()));
    let out = shared.clone();
    thread::spawn(move || {
        let mut meter = CpuMeter::default();
        meter.read();
        let mut tick = 0u32;
        loop {
            thread::sleep(TICK);
            let cpu = meter.read();
            let memory = ram::read();
            let slow = (tick % SLOW_EVERY == 0).then(|| {
                let processes = ram::processes();
                (system::gpu_load(), ram::top_processes(&processes, TOP_RAM), processes.len())
            });
            let status = guard.status();
            let log = guard.log_tail(LOG_LINES);
            if let Ok(mut live) = out.lock() {
                live.cpu = cpu;
                live.ram = memory;
                live.guard = status;
                live.log = log;
                if let Some((gpu, top_ram, processes)) = slow {
                    live.gpu = gpu;
                    live.top_ram = top_ram;
                    live.processes = processes;
                }
                let (gpu, load) = (live.gpu.unwrap_or(0.0), live.ram.load);
                push_point(&mut live.cpu_history, cpu);
                push_point(&mut live.gpu_history, gpu);
                push_point(&mut live.ram_history, load);
            }
            ctx.request_repaint();
            tick = tick.wrapping_add(1);
        }
    });
    shared
}
