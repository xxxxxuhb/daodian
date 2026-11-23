use super::*;

use base64::Engine;

#[test]
fn ctrl_alt_a_matches_the_configured_shortcut() {
    let shortcut: Shortcut = "Ctrl+1".parse().expect("快捷键");
    assert!(same_shortcut("ctrl+1", &shortcut));
    assert!(!same_shortcut("Alt+Space", &shortcut));
}

#[test]
fn bmp_stores_rows_bottom_up() {
    let bytes = bmp_file(1, 1, &[0, 0, 255, 255]).expect("bmp");
    assert_eq!(&bytes[0..2], b"BM");
    assert_eq!(u32::from_le_bytes(bytes[2..6].try_into().unwrap()), 58);
    assert_eq!(u32::from_le_bytes(bytes[10..14].try_into().unwrap()), 54);
    assert_eq!(i32::from_le_bytes(bytes[18..22].try_into().unwrap()), 1);
    assert_eq!(i32::from_le_bytes(bytes[22..26].try_into().unwrap()), 1);
    assert_eq!(u16::from_le_bytes(bytes[28..30].try_into().unwrap()), 32);
    assert_eq!(&bytes[54..], &[0, 0, 255, 255]);

    let rows = bmp_file(1, 2, &[0, 0, 255, 255, 255, 0, 0, 255]).expect("bmp");
    assert_eq!(&rows[54..58], &[255, 0, 0, 255]);
    assert_eq!(&rows[58..62], &[0, 0, 255, 255]);
}

#[test]
fn bmp_rejects_a_mismatched_buffer() {
    assert!(bmp_file(2, 1, &[0, 0, 255, 255]).is_err());
    assert!(bmp_file(0, 1, &[]).is_err());
}

#[test]
fn css_selection_maps_onto_the_monitor() {
    assert_eq!(css_to_physical(10.0, 1280.0, 1920), 15);
    assert_eq!(css_to_physical(1280.0, 1280.0, 1920), 1920);
    assert_eq!(clamp_grab(1920, 1080, 15, 0, 1920, 100).unwrap(), (15, 0, 1905, 100));
    assert!(clamp_grab(100, 100, 0, 0, 2, 2).is_err());
}

#[test]
fn crop_bgra_keeps_the_chosen_pixel() {
    let mut pixels = vec![0u8; 4 * 4 * 4];
    pixels[(1 * 4 + 1) * 4] = 7;
    let cropped = crop_bgra(4, 4, &pixels, 1, 1, 3, 3).unwrap();
    assert_eq!(cropped.len(), 36);
    assert_eq!(cropped[0], 7);
}

#[test]
fn png_roundtrip_keeps_a_red_pixel() {
    let image = image::RgbaImage::from_pixel(1, 1, image::Rgba([255, 0, 0, 255]));
    let mut bytes = Vec::new();
    image::DynamicImage::ImageRgba8(image)
        .write_to(
            &mut std::io::Cursor::new(&mut bytes),
            image::ImageFormat::Png,
        )
        .expect("编码");
    let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
    let decoded = decode_png(&encoded).expect("解码");
    assert_eq!(decoded.get_pixel(0, 0).0, [255, 0, 0, 255]);
}

#[test]
fn screenshot_name_includes_the_stamp() {
    assert_eq!(
        format_screenshot_name("20261123-144512"),
        "到点截图-20261123-144512.png"
    );
}

#[test]
fn duplicate_png_names_gain_a_suffix() {
    let dir = std::env::temp_dir().join(format!("daodian-shot-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("临时目录");
    let first = dir.join("到点截图-20261123-144512.png");
    fs::write(&first, b"a").expect("写入");
    assert_eq!(
        unique_png(first.clone()),
        dir.join("到点截图-20261123-144512-2.png")
    );
    fs::write(dir.join("到点截图-20261123-144512-2.png"), b"b").expect("写入");
    assert_eq!(
        unique_png(first),
        dir.join("到点截图-20261123-144512-3.png")
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn active_screen_follows_the_cursor() {
    let rects = [
        CaptureRect {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        },
        CaptureRect {
            x: 1920,
            y: 0,
            width: 2560,
            height: 1440,
        },
        CaptureRect {
            x: -1920,
            y: -200,
            width: 1920,
            height: 1080,
        },
    ];
    assert_eq!(screen_at(&rects, Some((10, 10))), 0);
    assert_eq!(screen_at(&rects, Some((2000, 10))), 1);
    assert_eq!(screen_at(&rects, Some((-10, 10))), 2);
    assert_eq!(screen_at(&rects, Some((10_000, 10))), 0);
    assert_eq!(screen_at(&rects, None), 0);
}

fn temp_shot_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "daodian-shot-{name}-{}-{}",
        std::process::id(),
        TMP_SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("临时目录");
    dir
}

fn leftover_temps(dir: &Path) -> Vec<PathBuf> {
    fs::read_dir(dir)
        .expect("读取目录")
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.contains(".tmp"))
        })
        .collect()
}

#[test]
fn ensure_png_only_fills_a_missing_extension() {
    let dir = PathBuf::from("图片");
    assert_eq!(ensure_png(dir.join("到点")), dir.join("到点.png"));
    assert_eq!(ensure_png(dir.join("到点.png")), dir.join("到点.png"));
    assert_eq!(ensure_png(dir.join("到点.PNG")), dir.join("到点.PNG"));
    assert_eq!(ensure_png(dir.join("到点.jpg")), dir.join("到点.jpg"));
}

#[test]
fn atomic_write_overwrites_the_chosen_path() {
    let dir = temp_shot_dir("overwrite");
    let path = dir.join("到点截图.png");
    fs::write(&path, b"old").expect("写入");
    write_bytes_atomic(&path, b"new-bytes").expect("覆盖");
    assert_eq!(fs::read(&path).expect("读回"), b"new-bytes");
    assert!(!dir.join("到点截图-2.png").exists());
    assert!(leftover_temps(&dir).is_empty());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn atomic_write_keeps_the_original_when_replace_fails() {
    let dir = temp_shot_dir("readonly");
    let path = dir.join("到点截图.png");
    fs::write(&path, b"old-bytes").expect("写入");
    let mut perms = fs::metadata(&path).expect("元数据").permissions();
    perms.set_readonly(true);
    fs::set_permissions(&path, perms).expect("只读");
    let result = write_bytes_atomic(&path, b"new-bytes");
    assert!(result.is_err(), "{result:?}");
    assert_eq!(fs::read(&path).expect("读回"), b"old-bytes");
    assert!(leftover_temps(&dir).is_empty());
    let mut perms = fs::metadata(&path).expect("元数据").permissions();
    perms.set_readonly(false);
    fs::set_permissions(&path, perms).expect("解除只读");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn last_directory_opens_before_the_default() {
    let root = temp_shot_dir("dirs");
    let last = root.join("last");
    fs::create_dir_all(&last).expect("上次目录");
    let pictures = root.join("pictures");
    let shots = root.join("shots");
    let start =
        resolve_save_start(last.to_str().expect("路径"), "", &pictures, &shots).expect("目录");
    assert_eq!(start.directory, last);
    assert!(start.warning.is_empty());
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn missing_last_directory_uses_the_default_folder() {
    let root = temp_shot_dir("default-dir");
    let pictures = root.join("pictures");
    let shots = root.join("shots");
    let start = resolve_save_start(
        root.join("gone").to_str().expect("路径"),
        "",
        &pictures,
        &shots,
    )
    .expect("目录");
    assert_eq!(start.directory, pictures.join("到点截图"));
    assert!(start.directory.is_dir());
    assert!(start.warning.is_empty());
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn configured_directory_is_created_for_the_dialog() {
    let root = temp_shot_dir("configured");
    let chosen = root.join("chosen");
    let start = resolve_save_start(
        "",
        chosen.to_str().expect("路径"),
        &root.join("pictures"),
        &root.join("shots"),
    )
    .expect("目录");
    assert_eq!(start.directory, chosen);
    assert!(chosen.is_dir());
    assert!(start.warning.is_empty());
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn unwritable_save_dir_falls_back_and_explains() {
    let root = temp_shot_dir("blocked");
    let pictures = root.join("pictures");
    fs::create_dir_all(&pictures).expect("图片目录");
    fs::write(pictures.join("到点截图"), b"not-a-dir").expect("挡住默认目录");
    let shots = root.join("shots");
    let start = resolve_save_start("", "", &pictures, &shots).expect("回退");
    assert_eq!(start.directory, shots);
    assert!(start.warning.contains("不可写"));
    assert!(shots.is_dir());
    let _ = fs::remove_dir_all(&root);
}
