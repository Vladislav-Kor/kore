//! Окно пульта: шапка с нагрузкой, вкладки и настройки.
//! Всё, что меняется мышкой, сразу записывается в файл настроек.
//! Своё приложение может добавить вкладки и плашки шапки через [`Extra`].

use std::io;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use eframe::egui::{
    self, pos2, vec2, Align, Color32, Frame, Id, Key, Layout, LayerId, Margin, Order, RichText,
    ScrollArea, Slider, TextEdit, TextureHandle, Ui,
};

use crate::comic::{self, bold, button, caption, chip, panel, title, Chip};
use crate::guard::{GuardConf, GuardSetup};
use crate::live::{self, Live};
use crate::ram::{self, RamView, Stick};

pub const NAME: &str = "KORE";
/// Слова расшифровки и отметка, даёт ли слово букву в KORE. K — Корчагин.
const ACRONYM: [(&str, bool); 5] = [
    ("Korchagin", true),
    ("Overload", true),
    ("&", false),
    ("Resource", true),
    ("Engine", true),
];
const HEADER_HEIGHT: f32 = 128.0;
/// Кольца шапки: радиус и шаг между центрами.
const RING_RADIUS: f32 = 50.0;
const RING_STEP: f32 = 112.0;
const PILL_WIDTH: f32 = 196.0;
/// Плашки стоят в три строки, остальные не помещаются.
const PILL_ROWS: usize = 3;
const BAM_TIME: Duration = Duration::from_millis(1100);
const GPU_LIMIT: f32 = 70.0;
/// Выше этого Windows начинает сбрасывать память на диск, и ПК тормозит.
const RAM_LIMIT: f32 = 85.0;
/// Доля выделенной памяти от предела, после которой новым программам может не хватить памяти.
const COMMIT_LIMIT: f32 = 90.0;
/// Сколько значения шкал догоняют новый замер, секунды.
const SMOOTH_TIME: f32 = 0.6;
/// Шкалы анимируем только в активном окне: в фоне хватит кадра в секунду.
const FOCUSED_FRAME: Duration = Duration::from_millis(66);
const IDLE_FRAME: Duration = Duration::from_secs(1);

/// Что пульт знает о своём ПК.
pub struct Settings {
    pub guard: GuardSetup,
    /// Выше этого предела облачко сторожа предупреждает текстом `advice`.
    pub advised_limit: u32,
    pub advice: String,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            guard: GuardSetup::default(),
            advised_limit: 85,
            advice: "Выше 85 % сторож вмешивается, когда ПК уже тормозит.".to_string(),
        }
    }
}

/// Плашка состояния в шапке.
#[derive(Debug, Clone, PartialEq)]
pub struct Pill {
    pub label: &'static str,
    pub value: String,
    pub color: Color32,
}

/// Отклик на действие: вспышка с коротким словом или облачко с проблемой.
#[derive(Debug, Default)]
pub struct Feedback {
    bam: Option<(&'static str, Instant)>,
    problem: Option<String>,
}

impl Feedback {
    pub fn done(&mut self, result: io::Result<()>, word: &'static str) {
        match result {
            Ok(()) => {
                self.problem = None;
                self.bam = Some((word, Instant::now()));
            }
            Err(e) => self.problem = Some(format!("Не вышло: {e}")),
        }
    }

    pub fn problem(&mut self, text: impl Into<String>) {
        self.problem = Some(text.into());
    }
}

/// Что видит дополнительная вкладка.
pub struct View<'a> {
    pub live: &'a Live,
    pub halftone: &'a TextureHandle,
    pub feedback: &'a mut Feedback,
}

/// Дополнительные вкладки и плашки шапки поверх общего пульта.
pub trait Extra {
    /// Названия вкладок: встают после общих.
    fn tabs(&self) -> Vec<&'static str> {
        Vec::new()
    }

    /// Нарисовать вкладку номер `index` из [`Extra::tabs`].
    fn show(&mut self, _index: usize, _ui: &mut Ui, _view: View) {}

    /// Плашки шапки: общие можно оставить, заменить или дописать. Помещаются первые три.
    fn pills(&self, base: Vec<Pill>) -> Vec<Pill> {
        base
    }
}

/// Пульт без дополнительных вкладок.
pub struct NoExtra;

impl Extra for NoExtra {}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Tab {
    Guard,
    Monitor,
    Memory,
    Extra(usize),
}

pub struct App {
    tab: Tab,
    live: Arc<Mutex<Live>>,
    halftone: TextureHandle,
    /// Планки не меняются, пока ПК включён: читаем один раз.
    sticks: Vec<Stick>,
    settings: Settings,
    conf: GuardConf,
    extra: Box<dyn Extra>,
    new_exempt: String,
    new_path: String,
    feedback: Feedback,
}

impl App {
    pub fn new(ctx: &egui::Context, settings: Settings, extra: Box<dyn Extra>) -> App {
        App {
            tab: Tab::Guard,
            live: live::spawn(ctx.clone(), settings.guard.clone()),
            halftone: comic::halftone_texture(ctx),
            sticks: ram::sticks(),
            conf: settings.guard.load(),
            settings,
            extra,
            new_exempt: String::new(),
            new_path: String::new(),
            feedback: Feedback::default(),
        }
    }

    fn save_guard(&mut self) {
        let result = self.settings.guard.save(&self.conf);
        self.feedback.done(result, "ЕСТЬ!");
    }

    fn header(&self, ui: &mut Ui, live: &Live) {
        let rect = ui.max_rect();
        let time = ui.input(|i| i.time) as f32;
        let cpu = ui.ctx().animate_value_with_time(Id::new("ring-cpu"), live.cpu, SMOOTH_TIME);
        let gpu = live.gpu.map(|v| ui.ctx().animate_value_with_time(Id::new("ring-gpu"), v, SMOOTH_TIME));
        let memory = (live.ram.total_mb > 0).then(|| ui.ctx().animate_value_with_time(Id::new("ring-ram"), live.ram.load, SMOOTH_TIME));
        let painter = ui.painter();
        comic::header_backdrop(painter, &self.halftone, rect, time);
        let y = rect.center().y - 2.0;
        comic::emblem(painter, pos2(rect.left() + 64.0, y), 42.0, time);
        let text = rect.left() + 130.0;
        comic::lettering(painter, pos2(text, rect.top() + 14.0), NAME, title(58.0), comic::GOLD, comic::INK, 3.0, 0.0);
        let words_end = comic::acronym(painter, pos2(text + 2.0, rect.top() + 86.0), &ACRONYM);
        let rings = [
            ("CPU", Some(cpu), self.conf.limit as f32),
            ("GPU", gpu, GPU_LIMIT),
            ("RAM", memory, RAM_LIMIT),
        ];
        let first_ring = rect.right() - 64.0 - RING_STEP * (rings.len() - 1) as f32;
        for (i, (name, value, limit)) in rings.into_iter().enumerate() {
            comic::ring_gauge(painter, pos2(first_ring + RING_STEP * i as f32, y), RING_RADIUS, name, value, limit, time);
        }
        // Плашки встают между расшифровкой и кольцами, если им хватает места.
        let left = first_ring - RING_RADIUS - 22.0 - PILL_WIDTH;
        if left > words_end + 24.0 {
            let pills = self.extra.pills(self.base_pills(live));
            for (row, pill) in pills.iter().take(PILL_ROWS).enumerate() {
                let rect = egui::Rect::from_min_size(pos2(left, rect.top() + 20.0 + row as f32 * 30.0), vec2(PILL_WIDTH, 25.0));
                comic::status_pill(painter, rect, pill.label, &pill.value, pill.color);
            }
        }
    }

    /// Общие плашки: сторож, выделенная память и сколько процессов под потолком.
    fn base_pills(&self, live: &Live) -> Vec<Pill> {
        let (guard, guard_color) = match live.guard.as_ref() {
            _ if !self.conf.enabled => ("ВЫКЛ".to_string(), comic::RED),
            Some(status) if status.session != 0 => (format!("ВКЛ · {} %", status.limit), comic::CYAN),
            _ => ("НЕТ СВЯЗИ".to_string(), comic::GOLD),
        };
        let memory = &live.ram;
        let commit = memory.commit_mb as f32 / memory.commit_limit_mb.max(1) as f32 * 100.0;
        let capped = live.guard.as_ref().map_or("—".to_string(), |status| status.capped.len().to_string());
        vec![
            Pill { label: "СТОРОЖ", value: guard, color: guard_color },
            Pill {
                label: "ВЫДЕЛЕНО",
                value: format!("{} / {}", gigabytes(memory.commit_mb), size(memory.commit_limit_mb)),
                color: comic::load_color(commit, COMMIT_LIMIT),
            },
            Pill { label: "ПОД ПОТОЛКОМ", value: capped, color: comic::CYAN },
        ]
    }

    fn tabs(&mut self, ui: &mut Ui) {
        let mut tabs = vec![(Tab::Guard, "СТОРОЖ CPU"), (Tab::Monitor, "МОНИТОР"), (Tab::Memory, "ПАМЯТЬ")];
        tabs.extend(self.extra.tabs().into_iter().enumerate().map(|(i, name)| (Tab::Extra(i), name)));
        ui.horizontal_wrapped(|ui| {
            for (tab, name) in tabs {
                let fill = if self.tab == tab { comic::RED } else { comic::CARD };
                if button(ui, name, fill).clicked() {
                    self.tab = tab;
                }
            }
        });
    }

    fn tab_guard(&mut self, ui: &mut Ui, live: &Live) {
        let status = live.guard.as_ref();
        if status.is_none_or(|s| s.session == 0) {
            panel(ui, comic::CARD, |ui| {
                ui.horizontal(|ui| {
                    if button(ui, "УСТАНОВИТЬ СТОРОЖА", comic::RED).clicked() {
                        let result = self.settings.guard.install();
                        self.feedback.done(result, "ЖМИ «ДА»!");
                    }
                    ui.label(
                        RichText::new("Сторож не запущен. Установка спросит права администратора, настройки ниже заработают сразу.")
                            .font(bold(15.0)),
                    );
                });
            });
            ui.add_space(8.0);
        }
        ui.columns(2, |columns| {
            self.guard_controls(&mut columns[0], live);
            self.guard_lists(&mut columns[1], live);
        });
    }

    fn guard_controls(&mut self, ui: &mut Ui, live: &Live) {
        let texture = self.halftone.clone();
        let mut changed = false;
        panel(ui, comic::CARD, |ui| {
            caption(ui, "ПРЕДЕЛ НАГРУЗКИ");
            comic::bubble(ui, &self.guard_mood(live), comic::BUBBLE);
            comic::meter(ui, &texture, "CPU", live.cpu, self.conf.limit as f32);
            setting(ui, "Сторож работает", |ui| {
                changed |= comic::toggle(ui, &mut self.conf.enabled).changed();
            });
            setting(ui, "Не трогать активное окно", |ui| {
                changed |= comic::toggle(ui, &mut self.conf.protect_foreground).changed();
            });
            ui.label(RichText::new("Предел общей загрузки CPU").font(bold(15.0)));
            let r = ui.add(Slider::new(&mut self.conf.limit, 50..=95).suffix(" %"));
            changed |= r.drag_stopped() || (r.changed() && !r.dragged());
            ui.label(RichText::new("Ниже этого потолка фон не режем").font(bold(15.0)));
            let r = ui.add(Slider::new(&mut self.conf.min_cap, 3..=30).suffix(" %"));
            changed |= r.drag_stopped() || (r.changed() && !r.dragged());
        });
        if changed {
            self.save_guard();
        }
        ui.add_space(8.0);
        panel(ui, comic::CARD, |ui| {
            caption(ui, "ЖУРНАЛ");
            if live.log.is_empty() {
                ui.label(RichText::new("Журнал пуст: сторож ещё не запускался.").font(bold(14.0)));
            }
            for line in &live.log {
                ui.label(RichText::new(line).font(bold(12.5)));
            }
        });
    }

    fn guard_lists(&mut self, ui: &mut Ui, live: &Live) {
        let mut remove_exempt = None;
        let mut remove_path = None;
        let mut add_exempt = false;
        let mut add_path = false;
        panel(ui, comic::CARD, |ui| {
            caption(ui, "КОГО НЕ РЕЗАТЬ");
            ui.horizontal_wrapped(|ui| {
                for name in &self.settings.guard.locked {
                    chip(ui, name, Chip::Locked, "Защищено в коде сторожа, убрать нельзя");
                }
                for name in &self.conf.exempt {
                    if self.settings.guard.is_locked(name) {
                        continue;
                    }
                    if chip(ui, name, Chip::Removable, "") {
                        remove_exempt = Some(name.clone());
                    }
                }
            });
            ui.horizontal(|ui| {
                let field = ui.add(TextEdit::singleline(&mut self.new_exempt).hint_text("Имя процесса").desired_width(190.0));
                add_exempt = submitted(ui, &field);
                add_exempt |= button(ui, "ДОБАВИТЬ", comic::GOLD).clicked();
            });
            caption(ui, "ПАПКИ С ИГРАМИ");
            ui.horizontal_wrapped(|ui| {
                for part in &self.conf.paths {
                    if chip(ui, part, Chip::Removable, "Процессы из этой папки сторож не режет") {
                        remove_path = Some(part.clone());
                    }
                }
            });
            ui.horizontal(|ui| {
                let field = ui.add(TextEdit::singleline(&mut self.new_path).hint_text(r"\Games\").desired_width(190.0));
                add_path = submitted(ui, &field);
                add_path |= button(ui, "ДОБАВИТЬ", comic::GOLD).clicked();
            });
        });
        if let Some(name) = remove_exempt {
            self.conf.remove_exempt(&name, &self.settings.guard);
            self.save_guard();
        }
        if let Some(part) = remove_path {
            self.conf.paths.retain(|p| *p != part);
            self.save_guard();
        }
        if add_exempt && !self.new_exempt.trim().is_empty() {
            match self.conf.add_exempt(&self.new_exempt.clone(), &self.settings.guard) {
                Ok(()) => {
                    self.new_exempt.clear();
                    self.save_guard();
                }
                Err(e) => self.feedback.problem(e),
            }
        }
        if add_path && !self.new_path.trim().is_empty() {
            match self.conf.add_path(&self.new_path.clone()) {
                Ok(()) => {
                    self.new_path.clear();
                    self.save_guard();
                }
                Err(e) => self.feedback.problem(e),
            }
        }
        ui.add_space(8.0);
        panel(ui, comic::CARD, |ui| {
            caption(ui, "ПОД ПОТОЛКОМ");
            match live.guard.as_ref() {
                Some(status) if !status.capped.is_empty() => {
                    ui.horizontal_wrapped(|ui| {
                        for (name, cap) in &status.capped {
                            chip(ui, &format!("{name} {cap} %"), Chip::Alert, "");
                        }
                    });
                }
                Some(_) => {
                    ui.label(RichText::new("Никого. Весь фон на свободе.").font(bold(15.0)));
                }
                None => {
                    ui.label(RichText::new("Сторож молчит: он не запущен или выключен.").font(bold(15.0)));
                }
            }
        });
    }

    fn guard_mood(&self, live: &Live) -> String {
        let conf = &self.conf;
        if !conf.enabled {
            return "Сторож отдыхает. За нагрузкой CPU сейчас никто не следит!".to_string();
        }
        if conf.limit > self.settings.advised_limit {
            return format!("Предел {} %. {}", conf.limit, self.settings.advice);
        }
        match live.guard.as_ref() {
            None => "Настройки записаны. Сторож ещё не запущен.".to_string(),
            Some(s) if s.total + 3 > s.limit => {
                let free = if s.foreground.is_empty() { "браузер".to_string() } else { s.foreground.clone() };
                format!("Горячо, {} %! Режу фон, а {free} не трогаю.", s.total)
            }
            Some(_) => "Всё спокойно. Браузер и игра свободны, фон под присмотром.".to_string(),
        }
    }

    fn tab_monitor(&self, ui: &mut Ui, live: &Live) {
        let time = ui.input(|i| i.time) as f32;
        ui.columns(2, |columns| {
            panel(&mut columns[0], comic::CARD, |ui| {
                caption(ui, "ПРОЦЕССОР");
                comic::history(ui, "CPU", &live.cpu_history, self.conf.limit as f32, time);
                ui.label(RichText::new("Красная линия — предел сторожа.").font(bold(14.0)).color(comic::TEXT_DIM));
            });
            panel(&mut columns[1], comic::CARD, |ui| {
                caption(ui, "ВИДЕОКАРТА");
                comic::history(ui, "GPU", &live.gpu_history, GPU_LIMIT, time);
                let note = if live.gpu.is_some() {
                    "Красная линия — 70 %: выше видеокарта греется и шумит."
                } else {
                    "Загрузку GPU пульт берёт у nvidia-smi: нужна видеокарта NVIDIA."
                };
                ui.label(RichText::new(note).font(bold(14.0)).color(comic::TEXT_DIM));
            });
        });
        ui.add_space(8.0);
        panel(ui, comic::CARD, |ui| {
            caption(ui, "ПАМЯТЬ");
            comic::history(ui, "RAM", &live.ram_history, RAM_LIMIT, time);
            ui.label(RichText::new("Красная линия — 85 %: выше Windows сбрасывает память на диск.").font(bold(14.0)).color(comic::TEXT_DIM));
        });
    }

    fn tab_memory(&self, ui: &mut Ui, live: &Live) {
        let time = ui.input(|i| i.time) as f32;
        let memory = &live.ram;
        ui.columns(2, |columns| {
            panel(&mut columns[0], comic::CARD, |ui| {
                caption(ui, "ЗАНЯТО");
                comic::bubble(ui, &memory_mood(memory), comic::BUBBLE);
                comic::meter(ui, &self.halftone, "RAM", memory.load, RAM_LIMIT);
                ui.add_space(4.0);
                for (name, value) in [
                    ("Занято", format!("{} из {}", size(memory.used_mb), size(memory.total_mb))),
                    ("Свободно", size(memory.available_mb)),
                    ("Выделено с подкачкой", format!("{} из {}", size(memory.commit_mb), size(memory.commit_limit_mb))),
                ] {
                    setting(ui, name, |ui| {
                        ui.label(RichText::new(value).font(bold(15.0)).color(comic::GOLD));
                    });
                }
                comic::history(ui, "RAM", &live.ram_history, RAM_LIMIT, time);
            });
            panel(&mut columns[1], comic::CARD, |ui| {
                caption(ui, "КТО ДЕРЖИТ ПАМЯТЬ");
                if live.top_ram.is_empty() {
                    ui.label(RichText::new("Считаю процессы…").font(bold(15.0)));
                }
                let total = memory.total_mb.max(1) as f32;
                for process in &live.top_ram {
                    let name = if process.count > 1 {
                        format!("{} ×{}", process.name, process.count)
                    } else {
                        process.name.clone()
                    };
                    let value = format!("{} · в ОЗУ {}", size(process.private_mb), size(process.ram_mb));
                    let share = process.private_mb as f32 / total;
                    comic::bar_row(ui, &name, &value, share, comic::load_color(share * 100.0, RAM_LIMIT / 2.0));
                }
                ui.label(
                    RichText::new("Сначала выделенная память процесса вместе с подкачкой, потом сколько её сейчас в планках. Полоса — доля от всей ОЗУ.")
                        .font(bold(12.5))
                        .color(comic::TEXT_DIM),
                );
            });
        });
        ui.add_space(8.0);
        panel(ui, comic::CARD, |ui| {
            caption(ui, "ПЛАНКИ");
            if self.sticks.is_empty() {
                ui.label(RichText::new("Прошивка не отдала список планок.").font(bold(15.0)));
            }
            for stick in &self.sticks {
                let color = if stick.overclocked() { comic::GOLD } else { comic::CYAN };
                comic::bar_row(ui, &stick_name(stick), &stick_speed(stick), 1.0, color);
            }
            if let Some(fast) = self.sticks.iter().find(|s| s.overclocked()) {
                let mut warning = format!(
                    "Планки рассчитаны на {} МТ/с, а работают на {}: включён профиль разгона XMP или EXPO.",
                    fast.rated(),
                    fast.running_mts
                );
                if ram::mixed(&self.sticks) {
                    warning += " И планки разных моделей.";
                }
                warning += " Разгон памяти — частая причина синих экранов: при сбоях проверяй её первой.";
                comic::bubble(ui, &warning, comic::GOLD);
            } else if ram::mixed(&self.sticks) {
                comic::bubble(ui, "Планки разных моделей. На своей частоте это нормально, но разгонять такую пару рискованно.", comic::BUBBLE);
            }
        });
    }

    fn paint_bam(&mut self, ctx: &egui::Context) {
        let Some((word, since)) = self.feedback.bam else { return };
        if since.elapsed() > BAM_TIME {
            self.feedback.bam = None;
            return;
        }
        let painter = ctx.layer_painter(LayerId::new(Order::Foreground, Id::new("bam")));
        let center = ctx.screen_rect().right_bottom() - vec2(190.0, 110.0);
        comic::burst(&painter, center, vec2(170.0, 92.0), 13, comic::GOLD);
        painter.text(center, egui::Align2::CENTER_CENTER, word, title(34.0), comic::INK);
    }
}

/// Строка настройки: подпись слева, элемент справа.
pub fn setting(ui: &mut Ui, name: &str, add: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(name).font(bold(15.0)));
        ui.with_layout(Layout::right_to_left(Align::Center), add);
    });
}

/// Поле ввода отправлено клавишей Enter.
pub fn submitted(ui: &Ui, field: &egui::Response) -> bool {
    field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter))
}

/// Объём по-русски: мегабайты до гигабайта, дальше гигабайты с одним знаком.
fn size(mb: u64) -> String {
    if mb < 1024 {
        format!("{mb} МБ")
    } else {
        format!("{:.1} ГБ", mb as f64 / 1024.0).replace('.', ",")
    }
}

/// Гигабайты без подписи: для плашки «37,0 / 65,8 ГБ».
fn gigabytes(mb: u64) -> String {
    format!("{:.1}", mb as f64 / 1024.0).replace('.', ",")
}

fn memory_mood(memory: &RamView) -> String {
    if memory.total_mb == 0 {
        return "Windows не отдала данные о памяти.".to_string();
    }
    let commit = memory.commit_mb as f32 / memory.commit_limit_mb.max(1) as f32 * 100.0;
    if commit >= COMMIT_LIMIT {
        format!(
            "Выделено {} из {}: это почти предел. Новые программы могут не открыться.",
            size(memory.commit_mb),
            size(memory.commit_limit_mb)
        )
    } else if memory.load >= RAM_LIMIT {
        format!("Память на исходе: {:.0} %. Windows сбрасывает её на диск, отсюда лаги.", memory.load)
    } else if memory.load >= RAM_LIMIT - 15.0 {
        format!("Память занята на {:.0} %. Запас есть, но небольшой.", memory.load)
    } else {
        format!("Память в порядке: свободно {}.", size(memory.available_mb))
    }
}

fn stick_name(stick: &Stick) -> String {
    let maker = [stick.maker.as_str(), stick.part.as_str()].iter().filter(|s| !s.is_empty()).copied().collect::<Vec<_>>().join(" ");
    let slot = if stick.slot.is_empty() { "Слот" } else { &stick.slot };
    format!("{slot} · {} {} · {maker}", size(stick.size_mb), stick.kind)
}

fn stick_speed(stick: &Stick) -> String {
    let mut line = format!("{} МТ/с", stick.running_mts);
    if stick.rated() > 0 && stick.rated() != stick.running_mts {
        let source = if stick.marked_mts.is_some() { "по маркировке" } else { "паспорт" };
        line += &format!(", {source} {}", stick.rated());
    }
    if stick.millivolts > 0 {
        line += &format!(" · {:.2} В", stick.millivolts as f32 / 1000.0).replace('.', ",");
    }
    line
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let live = self.live.lock().map(|l| l.clone()).unwrap_or_default();
        egui::TopBottomPanel::top("header")
            .exact_height(HEADER_HEIGHT)
            .frame(Frame::new().fill(comic::HEADER))
            .show(ctx, |ui| self.header(ui, &live));
        egui::TopBottomPanel::top("tabs")
            .frame(Frame::new().fill(comic::PAPER).inner_margin(Margin { left: 16, right: 16, top: 12, bottom: 6 }))
            .show(ctx, |ui| self.tabs(ui));
        egui::CentralPanel::default()
            .frame(Frame::new().fill(comic::PAPER).inner_margin(Margin::same(16)))
            .show(ctx, |ui| {
                let rect = ui.max_rect().expand(16.0);
                comic::halftone(ui.painter(), &self.halftone, rect, Color32::from_white_alpha(5));
                ScrollArea::vertical().id_salt(self.tab).auto_shrink([false, false]).show(ui, |ui| {
                    if let Some(problem) = self.feedback.problem.clone() {
                        comic::bubble(ui, &problem, comic::GOLD);
                    }
                    match self.tab {
                        Tab::Guard => self.tab_guard(ui, &live),
                        Tab::Monitor => self.tab_monitor(ui, &live),
                        Tab::Memory => self.tab_memory(ui, &live),
                        Tab::Extra(index) => {
                            let view = View { live: &live, halftone: &self.halftone, feedback: &mut self.feedback };
                            self.extra.show(index, ui, view);
                        }
                    }
                });
            });
        self.paint_bam(ctx);
        let focused = ctx.input(|i| i.focused);
        ctx.request_repaint_after(if focused { FOCUSED_FRAME } else { IDLE_FRAME });
    }
}
