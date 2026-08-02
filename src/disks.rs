use std::path::PathBuf;

use sysinfo::Disks;

pub struct DiskEntry {
    pub mount_point: PathBuf,
    pub label: String,
    pub size_text: String,
    pub removable: bool,
}

pub fn list_disks() -> Vec<DiskEntry> {
    Disks::new_with_refreshed_list()
        .iter()
        .map(|disk| DiskEntry {
            mount_point: disk.mount_point().to_path_buf(),
            label: disk.name().to_string_lossy().into_owned(),
            size_text: format_size(disk.total_space()),
            removable: disk.is_removable(),
        })
        .collect()
}

fn format_size(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
    let mut size = bytes as f64;
    let mut unit = 0;
    while size >= 1024.0 && unit < UNITS.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} {}", UNITS[unit])
    } else {
        format!("{size:.0} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_gigabytes() {
        assert_eq!(format_size(931 * 1024 * 1024 * 1024), "931 GB");
    }

    #[test]
    fn formats_bytes() {
        assert_eq!(format_size(500), "500 B");
    }
}
