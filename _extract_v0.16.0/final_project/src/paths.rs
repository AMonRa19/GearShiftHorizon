use std::path::PathBuf;

/// Runtime data is kept beside the executable in a dedicated `data` folder.
pub fn data_dir() -> PathBuf {
    let base = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    let dir = base.join("data");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

pub fn data_file(name: &str) -> PathBuf {
    data_dir().join(name)
}
