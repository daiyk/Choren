fn main() {
    // Filled by scripts/fetch-python-runtime.sh (run by `tauri build`); empty in dev, where box scripts use `python3`.
    std::fs::create_dir_all("python-runtime").unwrap();
    tauri_build::build()
}
