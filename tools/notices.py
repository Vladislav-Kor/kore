# Собирает THIRD-PARTY-NOTICES.txt: лицензии всех crate, которые попадают в KORE.exe.
# Одинаковые тексты лицензий печатаются один раз со списком crate. Запуск из корня репозитория:
#   python tools/notices.py
import hashlib
import json
import pathlib
import subprocess

ROOT = pathlib.Path(__file__).resolve().parent.parent
TARGET = "x86_64-pc-windows-msvc"
PREFIXES = ("LICENSE", "LICENCE", "COPYING", "NOTICE", "COPYRIGHT")
# Папки, где crate держат лицензии встроенных файлов: шрифты egui лежат в fonts.
EXTRA_DIRS = ("fonts", "licenses", "LICENSES")


def cargo(*args):
    return subprocess.run(["cargo", *args], cwd=ROOT, capture_output=True, text=True, encoding="utf-8", check=True).stdout


def used_packages():
    """Имя и версия каждого crate из дерева обычных зависимостей под Windows."""
    used = set()
    for line in cargo("tree", "-e", "normal", "--target", TARGET, "--prefix", "none", "--format", "{p}").splitlines():
        parts = line.replace("(*)", "").split()
        if len(parts) >= 2:
            used.add((parts[0], parts[1].lstrip("v")))
    return used


def main():
    used = used_packages()
    meta = json.loads(cargo("metadata", "--format-version", "1", "--filter-platform", TARGET))
    texts = {}
    without_files = []
    for package in sorted(meta["packages"], key=lambda p: (p["name"], p["version"])):
        key = (package["name"], package["version"])
        if key not in used or package["name"] == "kore":
            continue
        label = f'{package["name"]} {package["version"]} ({package.get("license") or "license-file"})'
        folder = pathlib.Path(package["manifest_path"]).parent
        files = sorted(p for p in folder.iterdir() if p.is_file() and p.name.upper().startswith(PREFIXES))
        for extra in EXTRA_DIRS:
            if (folder / extra).is_dir():
                files += sorted(p for p in (folder / extra).iterdir() if p.suffix.lower() == ".txt")
        if not files:
            without_files.append(label)
        for file in files:
            text = file.read_text(encoding="utf-8", errors="replace").strip()
            digest = hashlib.sha256(text.encode()).hexdigest()
            texts.setdefault(digest, (text, []))[1].append(f"{label}: {file.relative_to(folder).as_posix()}")
    out = [
        "KORE uses the following third-party Rust crates. Their license texts follow.",
        "KORE использует перечисленные ниже сторонние библиотеки Rust. Ниже тексты их лицензий.",
        "",
    ]
    for text, owners in sorted(texts.values(), key=lambda item: item[1][0]):
        out.append("=" * 78)
        out.extend(owners)
        out.append("-" * 78)
        out.append(text)
        out.append("")
    if without_files:
        out.append("=" * 78)
        out.append("Crates without a license file in the package; the license is the SPDX expression shown.")
        out.append("The texts of these licenses (MIT, Apache-2.0, BSL-1.0, Unicode-3.0) are included above with other crates;")
        out.append("crates licensed as MIT OR Apache-2.0 are used under Apache-2.0. CC0-1.0 needs no notice.")
        out.extend(without_files)
    (ROOT / "THIRD-PARTY-NOTICES.txt").write_text("\n".join(out) + "\n", encoding="utf-8", newline="\n")
    print(len(used) - 1, "crates,", len(texts), "unique license texts,", len(without_files), "without files")


main()
