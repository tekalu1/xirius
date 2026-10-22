#[test]
fn palette_values_run_through_clicks_in_a_parked_window() {
    let dir_tmp = tempfile::Builder::new()
        .prefix("xirius-aiarc-shots-")
        .tempdir()
        .unwrap();
    let dir = dir_tmp.path().join("aiarc-shots");
    std::fs::create_dir_all(&dir).unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_xirius-ui"))
        .args(["--park", "--silent", "--script", "aiarc", "--shot"])
        .arg(&dir)
        .env("XIRIUS_CONFIG_DIR", dir.join("prefs"))
        .env("XIRIUS_CRASH_DIR", dir.join("crashes"))
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stderr}");
    for stage in 0..11 {
        assert!(
            stderr.contains(&format!("aiarc-check stage={stage} ok=true")),
            "{stderr}"
        );
    }
    let pictures: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .path()
                .extension()
                .is_some_and(|extension| extension == "bmp")
        })
        .collect();
    assert_eq!(pictures.len(), 11, "{stderr}");
    for picture in pictures {
        let bytes = std::fs::read(picture.path()).unwrap();
        assert_eq!(&bytes[..2], b"BM");
        let offset = u32::from_le_bytes(bytes[10..14].try_into().unwrap()) as usize;
        let mut colors = std::collections::BTreeSet::new();
        for pixel in bytes[offset..].as_chunks::<4>().0 {
            colors.insert([pixel[0], pixel[1], pixel[2]]);
        }
        assert!(colors.len() > 50, "the window has rendered content");
    }
    std::fs::remove_dir_all(dir).unwrap();
}
