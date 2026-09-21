use std::path::{Path, PathBuf};

/// `report.pdf` → `report (1).pdf` … until the name is free.
pub fn unique_path(dir: &Path, filename: &str) -> PathBuf {
    unique_path_where(dir, filename, |p| !p.exists())
}

/// Like `unique_path`, but a name is only taken when its `.part` sibling is
/// free as well, so two downloads of `report.pdf` cannot share a temp file.
pub fn unique_download_path(dir: &Path, filename: &str) -> PathBuf {
    unique_path_where(dir, filename, |p| !p.exists() && !part_path(p).exists())
}

pub fn part_path(final_path: &Path) -> PathBuf {
    let mut s = final_path.as_os_str().to_owned();
    s.push(".part");
    PathBuf::from(s)
}

fn unique_path_where(dir: &Path, filename: &str, free: impl Fn(&Path) -> bool) -> PathBuf {
    let candidate = dir.join(filename);
    if free(&candidate) {
        return candidate;
    }
    let p = Path::new(filename);
    let stem = p
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "file".into());
    let ext = p
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    for i in 1.. {
        let candidate = dir.join(format!("{stem} ({i}){ext}"));
        if free(&candidate) {
            return candidate;
        }
    }
    unreachable!()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn download_path_avoids_names_with_a_partial_in_flight() {
        let dir = std::env::temp_dir().join(format!("tc-util-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();

        let first = unique_download_path(&dir, "report.pdf");
        assert_eq!(first, dir.join("report.pdf"));
        std::fs::write(part_path(&first), b"partial").unwrap();

        let second = unique_download_path(&dir, "report.pdf");
        assert_eq!(second, dir.join("report (1).pdf"));
        assert_eq!(part_path(&second), dir.join("report (1).pdf.part"));

        std::fs::remove_dir_all(&dir).unwrap();
    }
}
