//! Оперативная память: занятость, выделенная память, кто её держит и какие стоят планки.
//! Только чтение. Процессы и планки читаем напрямую у ядра и прошивки, мимо WMI: на части ПК он зависает.

use std::collections::{BTreeSet, HashMap};

const MB: u64 = 1024 * 1024;
const SYSTEM_PROCESS_INFORMATION: u32 = 5;
const STATUS_INFO_LENGTH_MISMATCH: i32 = 0xC000_0004_u32 as i32;
/// Смещения полей SYSTEM_PROCESS_INFORMATION в 64-битной Windows.
const NEXT_ENTRY: usize = 0x00;
const IMAGE_NAME_LENGTH: usize = 0x38;
const IMAGE_NAME_BUFFER: usize = 0x40;
const PROCESS_ID: usize = 0x50;
const WORKING_SET: usize = 0x90;
/// PagefileUsage: частная выделенная память процесса.
const PRIVATE_BYTES: usize = 0xB8;
/// Частоты DDR, по которым узнаём скорость в названии модели планки.
const DDR_SPEEDS: [u32; 24] = [
    1333, 1600, 1866, 2133, 2400, 2666, 2933, 3000, 3200, 3600, 4000, 4266, 4400, 4800, 5200, 5600, 6000, 6200,
    6400, 6800, 7200, 7600, 8000, 8400,
];
/// Подпись таблицы SMBIOS для GetSystemFirmwareTable: 'RSMB'.
const RSMB: u32 = u32::from_be_bytes(*b"RSMB");
/// Перед таблицей RawSMBIOSData идут версия и длина: 8 байт.
const RAW_HEADER: usize = 8;
const MEMORY_DEVICE: u8 = 17;
const END_OF_TABLE: u8 = 127;

#[repr(C)]
struct MemoryStatusEx {
    length: u32,
    load: u32,
    total_phys: u64,
    avail_phys: u64,
    total_page_file: u64,
    avail_page_file: u64,
    total_virtual: u64,
    avail_virtual: u64,
    avail_extended_virtual: u64,
}

#[link(name = "kernel32")]
extern "system" {
    fn GlobalMemoryStatusEx(status: *mut MemoryStatusEx) -> i32;
    fn GetSystemFirmwareTable(provider: u32, id: u32, buffer: *mut u8, size: u32) -> u32;
}

#[link(name = "ntdll")]
extern "system" {
    fn NtQuerySystemInformation(class: u32, buffer: *mut u8, length: u32, returned: *mut u32) -> i32;
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct RamView {
    /// Занято физической памяти, проценты.
    pub load: f32,
    pub used_mb: u64,
    pub total_mb: u64,
    pub available_mb: u64,
    /// Выделено, как в диспетчере задач: память плюс файл подкачки.
    pub commit_mb: u64,
    pub commit_limit_mb: u64,
}

pub fn read() -> RamView {
    let mut status = MemoryStatusEx {
        length: std::mem::size_of::<MemoryStatusEx>() as u32,
        load: 0,
        total_phys: 0,
        avail_phys: 0,
        total_page_file: 0,
        avail_page_file: 0,
        total_virtual: 0,
        avail_virtual: 0,
        avail_extended_virtual: 0,
    };
    // SAFETY: структура нужного размера, её длина записана в length, как требует вызов.
    if unsafe { GlobalMemoryStatusEx(&mut status) } == 0 || status.total_phys == 0 {
        return RamView::default();
    }
    let used = status.total_phys - status.avail_phys;
    RamView {
        load: used as f32 / status.total_phys as f32 * 100.0,
        used_mb: used / MB,
        total_mb: status.total_phys / MB,
        available_mb: status.avail_phys / MB,
        commit_mb: (status.total_page_file - status.avail_page_file) / MB,
        commit_limit_mb: status.total_page_file / MB,
    }
}

/// Память одного процесса.
#[derive(Debug, Clone, PartialEq)]
pub struct ProcessMemory {
    pub pid: u32,
    pub name: String,
    /// Рабочий набор: сколько страниц процесса сейчас в планках.
    pub working_set: u64,
    /// Частная выделенная память: сколько процесс забрал себе, вместе с подкачкой.
    pub private: u64,
}

/// Все процессы с их памятью. Права на процессы не нужны, системные тоже видны.
pub fn processes() -> Vec<ProcessMemory> {
    // u64 даёт буферу выравнивание 8, как у полей записи.
    let mut buffer = vec![0u64; 1 << 17];
    for _ in 0..4 {
        let bytes = (buffer.len() * 8) as u32;
        let mut returned = 0u32;
        // SAFETY: буфер живёт до конца вызова, его размер передан в bytes.
        let status = unsafe {
            NtQuerySystemInformation(SYSTEM_PROCESS_INFORMATION, buffer.as_mut_ptr().cast(), bytes, &mut returned)
        };
        if status == STATUS_INFO_LENGTH_MISMATCH {
            // Процессов могло прибавиться, пока буфер растёт: берём с запасом.
            buffer.resize((returned as usize / 8 + 1).max(buffer.len()) * 2, 0);
            continue;
        }
        if status != 0 {
            return Vec::new();
        }
        let length = (returned as usize).min(buffer.len() * 8);
        // SAFETY: те же байты буфера, только как u8; ссылка живёт, пока жив buffer.
        let raw = unsafe { std::slice::from_raw_parts(buffer.as_ptr().cast::<u8>(), length) };
        return parse_processes(raw);
    }
    Vec::new()
}

fn parse_processes(raw: &[u8]) -> Vec<ProcessMemory> {
    let base = raw.as_ptr() as usize;
    let mut list = Vec::new();
    let mut at = 0;
    while let (Some(next), Some(pid), Some(working_set), Some(private)) = (
        dword(raw, at + NEXT_ENTRY),
        qword(raw, at + PROCESS_ID),
        qword(raw, at + WORKING_SET),
        qword(raw, at + PRIVATE_BYTES),
    ) {
        // Имя лежит в том же буфере, запись хранит на него указатель.
        let length = word(raw, at + IMAGE_NAME_LENGTH).unwrap_or(0) as usize;
        let pointer = qword(raw, at + IMAGE_NAME_BUFFER).unwrap_or(0) as usize;
        let name = pointer
            .checked_sub(base)
            .and_then(|start| raw.get(start..start + length))
            .map(|bytes| String::from_utf16_lossy(&bytes.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect::<Vec<_>>()))
            .unwrap_or_else(|| "Idle".to_string());
        list.push(ProcessMemory { pid: pid as u32, name, working_set, private });
        if next == 0 {
            break;
        }
        at += next as usize;
    }
    list
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProcessRam {
    pub name: String,
    /// Сколько процессов с этим именем.
    pub count: usize,
    pub private_mb: u64,
    pub ram_mb: u64,
}

/// Кто забрал больше всего выделенной памяти, копии одного имени сложены.
pub fn top_processes(list: &[ProcessMemory], count: usize) -> Vec<ProcessRam> {
    let mut by_name: HashMap<&str, ProcessRam> = HashMap::new();
    for process in list {
        let name = process.name.strip_suffix(".exe").unwrap_or(&process.name);
        let entry = by_name.entry(name).or_insert_with(|| ProcessRam {
            name: name.to_string(),
            count: 0,
            private_mb: 0,
            ram_mb: 0,
        });
        entry.count += 1;
        entry.private_mb += process.private / MB;
        entry.ram_mb += process.working_set / MB;
    }
    let mut top: Vec<ProcessRam> = by_name.into_values().collect();
    top.sort_by(|a, b| b.private_mb.cmp(&a.private_mb).then_with(|| a.name.cmp(&b.name)));
    top.truncate(count);
    top
}

/// Планка памяти из SMBIOS, запись типа 17.
#[derive(Debug, Clone, PartialEq)]
pub struct Stick {
    pub slot: String,
    pub size_mb: u64,
    pub kind: &'static str,
    pub maker: String,
    pub part: String,
    /// Паспортная частота планки по прошивке, МТ/с. С профилем разгона прошивка часто пишет сюда его частоту.
    pub rated_mts: u32,
    /// Частота из названия модели: KS4800D5…, D5-4800.
    pub marked_mts: Option<u32>,
    /// Частота, на которой планка работает сейчас, МТ/с.
    pub running_mts: u32,
    pub millivolts: u16,
}

impl Stick {
    /// Частота по маркировке, а если в названии её нет, то паспорт из прошивки.
    pub fn rated(&self) -> u32 {
        self.marked_mts.unwrap_or(self.rated_mts)
    }

    /// Работает быстрее своей частоты: включён профиль разгона XMP или EXPO.
    pub fn overclocked(&self) -> bool {
        self.rated() > 0 && self.running_mts > self.rated()
    }
}

/// Планки разных производителей или моделей.
pub fn mixed(sticks: &[Stick]) -> bool {
    sticks.iter().map(|s| (s.maker.as_str(), s.part.as_str())).collect::<BTreeSet<_>>().len() > 1
}

/// Частота DDR в названии модели: ровно четыре цифры подряд из списка известных частот.
fn marked_speed(part: &str) -> Option<u32> {
    part.split(|c: char| !c.is_ascii_digit())
        .filter(|run| run.len() == 4)
        .filter_map(|run| run.parse().ok())
        .find(|speed| DDR_SPEEDS.contains(speed))
}

pub fn sticks() -> Vec<Stick> {
    // SAFETY: нулевой буфер допустим, вызов только сообщает нужный размер.
    let size = unsafe { GetSystemFirmwareTable(RSMB, 0, std::ptr::null_mut(), 0) };
    if size == 0 {
        return Vec::new();
    }
    let mut raw = vec![0u8; size as usize];
    // SAFETY: буфер живёт до конца вызова, его размер передан в size.
    let got = unsafe { GetSystemFirmwareTable(RSMB, 0, raw.as_mut_ptr(), size) } as usize;
    if got == 0 || got > raw.len() {
        return Vec::new();
    }
    raw.get(RAW_HEADER..got).map(parse_smbios).unwrap_or_default()
}

fn parse_smbios(table: &[u8]) -> Vec<Stick> {
    let mut sticks = Vec::new();
    let mut at = 0;
    while at + 4 <= table.len() {
        let kind = table[at];
        let length = table[at + 1] as usize;
        if length < 4 || at + length > table.len() {
            break;
        }
        // За форматированной частью записи идут строки, набор кончается двумя нулями.
        let mut end = at + length;
        while end + 1 < table.len() && !(table[end] == 0 && table[end + 1] == 0) {
            end += 1;
        }
        let strings: Vec<String> =
            table[at + length..end].split(|b| *b == 0).map(|s| String::from_utf8_lossy(s).trim().to_string()).collect();
        if kind == MEMORY_DEVICE {
            sticks.extend(stick(&table[at..at + length], &strings));
        }
        if kind == END_OF_TABLE {
            break;
        }
        at = end + 2;
    }
    sticks
}

fn word(record: &[u8], at: usize) -> Option<u16> {
    record.get(at..at + 2).map(|b| u16::from_le_bytes([b[0], b[1]]))
}

fn dword(record: &[u8], at: usize) -> Option<u32> {
    record.get(at..at + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

fn qword(record: &[u8], at: usize) -> Option<u64> {
    record.get(at..at + 8).map(|b| u64::from_le_bytes(b.try_into().unwrap_or_default()))
}

/// Строка записи: в поле её номер с единицы, 0 значит «нет строки».
fn text(record: &[u8], strings: &[String], at: usize) -> String {
    let index = record.get(at).map_or(0, |i| *i as usize);
    index.checked_sub(1).and_then(|i| strings.get(i)).cloned().unwrap_or_default()
}

/// Частота: 0xFFFF в коротком поле значит, что она лежит в длинном.
fn speed(record: &[u8], short: usize, long: usize) -> u32 {
    match word(record, short) {
        Some(0xFFFF) => dword(record, long).unwrap_or(0),
        Some(value) => u32::from(value),
        None => 0,
    }
}

fn memory_kind(code: u8) -> &'static str {
    match code {
        0x18 => "DDR3",
        0x1A => "DDR4",
        0x1D => "LPDDR3",
        0x1E => "LPDDR4",
        0x22 => "DDR5",
        0x23 => "LPDDR5",
        _ => "RAM",
    }
}

fn stick(record: &[u8], strings: &[String]) -> Option<Stick> {
    let size_mb = match word(record, 0x0C)? {
        // Пустой слот или размер неизвестен.
        0 | 0xFFFF => return None,
        0x7FFF => u64::from(dword(record, 0x1C)? & 0x7FFF_FFFF),
        kb if kb & 0x8000 != 0 => u64::from(kb & 0x7FFF) / 1024,
        mb => u64::from(mb),
    };
    let part = text(record, strings, 0x1A);
    Some(Stick {
        slot: text(record, strings, 0x10),
        size_mb,
        kind: memory_kind(record.get(0x12).copied().unwrap_or(0)),
        maker: text(record, strings, 0x17),
        marked_mts: marked_speed(&part),
        part,
        rated_mts: speed(record, 0x15, 0x54),
        running_mts: speed(record, 0x20, 0x58),
        millivolts: word(record, 0x26).unwrap_or(0),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Запись типа 17 длиной 0x28 со строками слота, производителя и модели.
    fn memory_record(size: u16, rated: u16, running: u16) -> Vec<u8> {
        let mut record = vec![0u8; 0x28];
        record[0] = MEMORY_DEVICE;
        record[1] = 0x28;
        record[0x0C..0x0E].copy_from_slice(&size.to_le_bytes());
        record[0x10] = 1;
        record[0x12] = 0x22;
        record[0x15..0x17].copy_from_slice(&rated.to_le_bytes());
        record[0x17] = 2;
        record[0x1A] = 3;
        record[0x20..0x22].copy_from_slice(&running.to_le_bytes());
        record[0x26..0x28].copy_from_slice(&1250u16.to_le_bytes());
        record.extend_from_slice(b"DIMMA2\0Kingston\0KF556C40-16 \0\0");
        record
    }

    #[test]
    fn reads_sticks_and_skips_empty_slots() {
        let mut table = vec![0u8, 4, 0, 0, 0, 0];
        table.extend(memory_record(16384, 4800, 5600));
        table.extend(memory_record(0, 0, 0));
        table.extend([END_OF_TABLE, 4, 0, 0, 0, 0]);
        let sticks = parse_smbios(&table);
        assert_eq!(sticks.len(), 1);
        let stick = &sticks[0];
        assert_eq!(stick.slot, "DIMMA2");
        assert_eq!(stick.maker, "Kingston");
        assert_eq!(stick.part, "KF556C40-16");
        assert_eq!((stick.size_mb, stick.kind), (16384, "DDR5"));
        assert_eq!((stick.rated_mts, stick.running_mts, stick.millivolts), (4800, 5600, 1250));
        assert!(stick.overclocked());
    }

    #[test]
    fn reads_extended_size_and_speed() {
        let mut record = memory_record(0x7FFF, 0xFFFF, 4800);
        record[1] = 0x5C;
        let strings = record.split_off(0x28);
        record.resize(0x5C, 0);
        record[0x1C..0x20].copy_from_slice(&65536u32.to_le_bytes());
        record[0x54..0x58].copy_from_slice(&8000u32.to_le_bytes());
        record.extend(strings);
        let sticks = parse_smbios(&record);
        assert_eq!(sticks[0].size_mb, 65536);
        assert_eq!(sticks[0].rated_mts, 8000);
        assert!(!sticks[0].overclocked());
    }

    #[test]
    fn speed_comes_from_the_model_name() {
        assert_eq!(marked_speed("CL40-40-40 D5-4800"), Some(4800));
        assert_eq!(marked_speed("KS4800D5P11016G"), Some(4800));
        assert_eq!(marked_speed("F5-6000J3040G32G"), Some(6000));
        assert_eq!(marked_speed("KF556C40-16"), None);
        assert_eq!(marked_speed("11016"), None);
    }

    #[test]
    fn marking_beats_firmware_speed() {
        let mut record = memory_record(16384, 5600, 5600);
        record.truncate(0x28);
        record.extend(b"DIMMA2\0Netac\0D5-4800\0\0");
        let sticks = parse_smbios(&record);
        assert_eq!((sticks[0].rated(), sticks[0].marked_mts), (4800, Some(4800)));
        assert!(sticks[0].overclocked());
        let mut other = sticks[0].clone();
        other.maker = "KingSpec".to_string();
        assert!(!mixed(&[sticks[0].clone(), sticks[0].clone()]));
        assert!(mixed(&[sticks[0].clone(), other]));
    }

    fn put(raw: &mut [u8], at: usize, bytes: &[u8]) {
        raw[at..at + bytes.len()].copy_from_slice(bytes);
    }

    #[test]
    fn reads_process_records() {
        let mut raw = vec![0u8; 0x300];
        let base = raw.as_ptr() as usize;
        let name: Vec<u8> = "python.exe".encode_utf16().flat_map(u16::to_le_bytes).collect();
        put(&mut raw, 0x200, &name);
        // Первая запись — Idle без имени, вторая — python.exe.
        put(&mut raw, NEXT_ENTRY, &0x100u32.to_le_bytes());
        let second = 0x100;
        put(&mut raw, second + IMAGE_NAME_LENGTH, &(name.len() as u16).to_le_bytes());
        put(&mut raw, second + IMAGE_NAME_BUFFER, &((base + 0x200) as u64).to_le_bytes());
        put(&mut raw, second + PROCESS_ID, &4242u64.to_le_bytes());
        put(&mut raw, second + WORKING_SET, &(100 * MB).to_le_bytes());
        put(&mut raw, second + PRIVATE_BYTES, &(300 * MB).to_le_bytes());
        let list = parse_processes(&raw);
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].name, "Idle");
        assert_eq!((list[1].pid, list[1].name.as_str()), (4242, "python.exe"));
        assert_eq!((list[1].working_set, list[1].private), (100 * MB, 300 * MB));
    }

    #[test]
    fn top_sums_copies_and_sorts_by_private_memory() {
        let process = |name: &str, private: u64, working_set: u64| ProcessMemory {
            pid: 1,
            name: name.to_string(),
            working_set: working_set * MB,
            private: private * MB,
        };
        let list = [process("python.exe", 900, 100), process("python.exe", 600, 50), process("telegram.exe", 1000, 900)];
        let top = top_processes(&list, 5);
        assert_eq!((top[0].name.as_str(), top[0].count, top[0].private_mb, top[0].ram_mb), ("python", 2, 1500, 150));
        assert_eq!(top[1].name, "telegram");
        assert_eq!(top_processes(&list, 1).len(), 1);
    }

    #[test]
    fn broken_table_gives_nothing() {
        assert!(parse_smbios(&[MEMORY_DEVICE, 2, 0]).is_empty());
        assert!(parse_smbios(&[MEMORY_DEVICE, 0x40, 0, 0, 0]).is_empty());
    }

    #[test]
    fn reads_this_pc() {
        let view = read();
        assert!(view.total_mb > 0 && view.used_mb <= view.total_mb);
        assert!(view.commit_limit_mb >= view.total_mb / 2);
        let list = processes();
        let me = list.iter().find(|p| p.pid == std::process::id()).expect("свой процесс есть в списке");
        assert!(me.name.to_lowercase().starts_with("kore") && me.working_set > 0);
    }
}
