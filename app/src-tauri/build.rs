fn main() {
    // Filled by scripts/fetch-python-runtime.sh (run by `tauri build`); empty in dev, where box scripts use `python3`.
    std::fs::create_dir_all("python-runtime").unwrap();
    // Filled by scripts/third-party-notices.py (run by `tauri build`); empty in dev.
    std::fs::create_dir_all("licenses").unwrap();
    tauri_build::build()
}
