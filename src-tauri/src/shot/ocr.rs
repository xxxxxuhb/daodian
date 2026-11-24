//! 选区取字。只用 Windows 自带的本地识别，文字不写日志、不联网。

use serde::Serialize;

const MAX_CHARS: usize = 10_000;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OcrOutcome {
    pub text: String,
    pub hint: String,
    pub open_settings: bool,
}

pub fn clip_text(text: &str) -> String {
    text.trim().chars().take(MAX_CHARS).collect()
}

/// 小图放大，压成灰度；暗底反色，方便识别白字。
pub(crate) fn prepare_rgba(image: image::RgbaImage) -> image::RgbaImage {
    let (width, height) = image.dimensions();
    let long = width.max(height);
    let image = if long > 0 && long * 2 <= 4096 {
        image::imageops::resize(
            &image,
            width.saturating_mul(2).max(1),
            height.saturating_mul(2).max(1),
            image::imageops::FilterType::Triangle,
        )
    } else {
        image
    };
    let mut sum = 0u64;
    let mut count = 0u64;
    for pixel in image.pixels() {
        sum += luma(pixel[0], pixel[1], pixel[2]);
        count += 1;
    }
    let dark = count > 0 && sum / count < 96;
    let mut image = image;
    for pixel in image.pixels_mut() {
        let mut value = luma(pixel[0], pixel[1], pixel[2]) as u8;
        if dark {
            value = 255 - value;
        }
        pixel[0] = value;
        pixel[1] = value;
        pixel[2] = value;
        pixel[3] = 255;
    }
    image
}

fn luma(red: u8, green: u8, blue: u8) -> u64 {
    (u64::from(red) * 54 + u64::from(green) * 183 + u64::from(blue) * 19) / 256
}

pub fn recognize_png(png: &str) -> Result<OcrOutcome, String> {
    let image = prepare_rgba(super::save::decode_png(png)?);
    #[cfg(windows)]
    {
        return recognize_windows(&image);
    }
    #[cfg(not(windows))]
    {
        let _ = image;
        Err("当前平台暂不支持取字".into())
    }
}

#[cfg(windows)]
fn recognize_windows(image: &image::RgbaImage) -> Result<OcrOutcome, String> {
    use image::ImageEncoder;

    let mut bytes = Vec::new();
    let encoder = image::codecs::png::PngEncoder::new(&mut bytes);
    encoder
        .write_image(
            image.as_raw(),
            image.width(),
            image.height(),
            image::ExtendedColorType::Rgba8,
        )
        .map_err(|err| format!("取字前处理失败: {err}"))?;
    let (engine, hint, open_settings) = ocr_engine()?;
    if open_settings && engine.is_none() {
        return Ok(OcrOutcome {
            text: String::new(),
            hint,
            open_settings: true,
        });
    }
    let Some(engine) = engine else {
        return Ok(OcrOutcome {
            text: String::new(),
            hint,
            open_settings,
        });
    };
    let bitmap = software_bitmap(&bytes)?;
    let result = engine
        .RecognizeAsync(&bitmap)
        .map_err(|err| format!("文字识别失败: {err}"))?
        .join()
        .map_err(|err| format!("文字识别失败: {err}"))?;
    let mut lines = Vec::new();
    if let Ok(found) = result.Lines() {
        for line in found {
            if let Ok(text) = line.Text() {
                lines.push(text.to_string());
            }
        }
    }
    let text = clip_text(&lines.join("\n"));
    let hint = if text.is_empty() && hint.is_empty() {
        "这块区域里没有识别到文字".into()
    } else {
        hint
    };
    Ok(OcrOutcome {
        text,
        hint,
        open_settings,
    })
}

#[cfg(windows)]
fn ocr_engine() -> Result<
    (
        Option<windows::Media::Ocr::OcrEngine>,
        String,
        bool,
    ),
    String,
> {
    ensure_apartment();
    let languages = windows::Media::Ocr::OcrEngine::AvailableRecognizerLanguages()
        .map_err(|err| format!("读不到文字识别语言: {err}"))?;
    let mut chinese = false;
    for language in languages {
        let tag = language
            .LanguageTag()
            .unwrap_or_default()
            .to_string()
            .to_ascii_lowercase();
        if tag.starts_with("zh") {
            chinese = true;
        }
    }
    match windows::Media::Ocr::OcrEngine::TryCreateFromUserProfileLanguages() {
        Ok(engine) => {
            let hint = if chinese {
                String::new()
            } else {
                "没有中文语言包，已用系统现有语言。可以到 Windows 语言设置里添加中文。".into()
            };
            Ok((Some(engine), hint, !chinese))
        }
        Err(_) => Ok((
            None,
            "没有可用的 Windows 文字识别语言包。".into(),
            true,
        )),
    }
}

#[cfg(windows)]
fn software_bitmap(png: &[u8]) -> Result<windows::Graphics::Imaging::SoftwareBitmap, String> {
    use windows::Graphics::Imaging::BitmapDecoder;
    use windows::Storage::Streams::{DataWriter, InMemoryRandomAccessStream};

    let stream = InMemoryRandomAccessStream::new().map_err(|err| format!("准备识别画面失败: {err}"))?;
    let writer = DataWriter::CreateDataWriter(&stream).map_err(|err| format!("准备识别画面失败: {err}"))?;
    writer
        .WriteBytes(png)
        .map_err(|err| format!("准备识别画面失败: {err}"))?;
    writer
        .StoreAsync()
        .map_err(|err| format!("准备识别画面失败: {err}"))?
        .join()
        .map_err(|err| format!("准备识别画面失败: {err}"))?;
    writer
        .DetachStream()
        .map_err(|err| format!("准备识别画面失败: {err}"))?;
    stream
        .Seek(0)
        .map_err(|err| format!("准备识别画面失败: {err}"))?;
    let decoder = BitmapDecoder::CreateAsync(&stream)
        .map_err(|err| format!("准备识别画面失败: {err}"))?
        .join()
        .map_err(|err| format!("准备识别画面失败: {err}"))?;
    decoder
        .GetSoftwareBitmapAsync()
        .map_err(|err| format!("准备识别画面失败: {err}"))?
        .join()
        .map_err(|err| format!("准备识别画面失败: {err}"))
}

#[cfg(windows)]
fn ensure_apartment() {
    unsafe {
        let _ = windows::Win32::System::WinRT::RoInitialize(
            windows::Win32::System::WinRT::RO_INIT_MULTITHREADED,
        );
    }
}

pub fn copy_text(text: &str) -> Result<(), String> {
    let text = clip_text(text);
    if text.is_empty() {
        return Err("没有可复制的文字".into());
    }
    let mut clipboard = arboard::Clipboard::new().map_err(|err| format!("打不开剪贴板: {err}"))?;
    clipboard
        .set_text(text)
        .map_err(|err| format!("复制文字失败: {err}"))
}

pub fn open_language_settings() -> Result<(), String> {
    #[cfg(windows)]
    {
        std::process::Command::new("cmd")
            .args(["/C", "start", "", "ms-settings:regionlanguage"])
            .spawn()
            .map_err(|err| format!("打不开语言设置: {err}"))?;
        return Ok(());
    }
    #[cfg(not(windows))]
    {
        Err("当前平台没有 Windows 语言设置".into())
    }
}

#[cfg(test)]
mod tests {
    use super::{clip_text, prepare_rgba};

    #[test]
    fn clip_stops_at_ten_thousand_chars() {
        let text = "字".repeat(10_005);
        assert_eq!(clip_text(&text).chars().count(), 10_000);
    }

    #[test]
    fn dark_text_is_scaled_and_inverted() {
        let image = image::RgbaImage::from_pixel(2, 2, image::Rgba([0, 0, 0, 255]));
        let prepared = prepare_rgba(image);
        assert_eq!(prepared.dimensions(), (4, 4));
        assert_eq!(prepared.get_pixel(0, 0).0, [255, 255, 255, 255]);
    }

    #[test]
    fn light_text_stays_light() {
        let image = image::RgbaImage::from_pixel(2, 2, image::Rgba([250, 250, 250, 255]));
        let prepared = prepare_rgba(image);
        assert_eq!(prepared.get_pixel(1, 1).0[0], 250);
    }
}
