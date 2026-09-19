use std::io::Cursor;

use anyhow::{Context as _, anyhow, bail};
use image::{
    DynamicImage, GenericImageView, GrayImage, ImageDecoder as _, ImageReader, Limits, Luma,
    imageops::FilterType,
};
use tesseract::{PageSegMode, Tesseract};

use crate::configs::scam_detect::ScamDetectConfig;

#[derive(Debug)]
pub(super) struct OcrOutput {
    pub text: String,
    pub width: u32,
    pub height: u32,
}

pub(super) fn validate(config: &ScamDetectConfig) -> anyhow::Result<()> {
    new_tesseract(config).map(|_| ())
}

pub(super) fn process_image_and_ocr(
    bytes: &[u8],
    config: &ScamDetectConfig,
) -> anyhow::Result<OcrOutput> {
    let reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .context("detect image format")?;
    let mut decoder = reader
        .into_decoder()
        .context("create image decoder")?;
    let (source_width, source_height) = decoder.dimensions();
    let decoded_pixels = u64::from(source_width).saturating_mul(u64::from(source_height));
    if decoded_pixels > config.max_decoded_pixels {
        bail!(
            "decoded image has {decoded_pixels} pixels, exceeding configured {} pixel limit",
            config.max_decoded_pixels
        );
    }

    let mut limits = Limits::default();
    limits.max_alloc = Some(
        config
            .max_decoded_pixels
            .saturating_mul(8),
    );
    decoder
        .set_limits(limits)
        .context("apply image decode limits")?;

    let image = DynamicImage::from_decoder(decoder).context("decode image")?;
    let image = resize_if_needed(image, config.max_image_width);
    let (width, height) = image.dimensions();
    let processed = preprocess(image);

    let psm6 = run_tesseract(&processed, config, PageSegMode::PsmSingleBlock)?;
    let text = if psm6.trim().chars().count() >= config.ocr_min_chars_for_psm6 {
        psm6
    } else {
        let psm11 = run_tesseract(&processed, config, PageSegMode::PsmSparseText)?;
        choose_longer(psm6, psm11)
    };

    Ok(OcrOutput { text: limit_text(text, config.ocr_text_limit), width, height })
}

fn new_tesseract(config: &ScamDetectConfig) -> anyhow::Result<Tesseract> {
    Tesseract::new(
        config.tessdata_dir.as_deref(),
        Some(&config.tesseract_lang),
    )
    .map_err(|error| anyhow!("initialize Tesseract: {error}"))
}

fn run_tesseract(
    image: &GrayImage,
    config: &ScamDetectConfig,
    page_seg_mode: PageSegMode,
) -> anyhow::Result<String> {
    let width = i32::try_from(image.width()).context("OCR image width exceeds i32")?;
    let height = i32::try_from(image.height()).context("OCR image height exceeds i32")?;
    let mut tesseract = new_tesseract(config)?
        .set_frame(image.as_raw(), width, height, 1, width)
        .map_err(|error| anyhow!("set Tesseract image frame: {error}"))?;
    tesseract.set_page_seg_mode(page_seg_mode);
    let mut tesseract = tesseract
        .recognize()
        .map_err(|error| anyhow!("recognize image text: {error}"))?;
    tesseract
        .get_text()
        .map_err(|error| anyhow!("read recognized image text: {error}"))
}

fn resize_if_needed(image: DynamicImage, max_width: u32) -> DynamicImage {
    let (width, height) = image.dimensions();
    if max_width == 0 || width <= max_width {
        return image;
    }

    let ratio = max_width as f32 / width as f32;
    let new_height = (height as f32 * ratio).round().max(1.0) as u32;
    image.resize(max_width, new_height, FilterType::Triangle)
}

fn preprocess(image: DynamicImage) -> GrayImage {
    let mut gray = image.to_luma8();
    stretch_contrast(&mut gray);
    light_threshold(&mut gray);
    gray
}

fn stretch_contrast(image: &mut GrayImage) {
    let mut min = u8::MAX;
    let mut max = u8::MIN;

    for pixel in image.pixels() {
        let value = pixel[0];
        min = min.min(value);
        max = max.max(value);
    }

    if max <= min || max.saturating_sub(min) < 20 {
        return;
    }

    for pixel in image.pixels_mut() {
        let value = pixel[0];
        let stretched = ((value.saturating_sub(min) as u16) * 255 / (max - min) as u16) as u8;
        *pixel = Luma([stretched]);
    }
}

fn light_threshold(image: &mut GrayImage) {
    for pixel in image.pixels_mut() {
        let value = pixel[0];
        let adjusted = if value > 210 {
            255
        } else if value < 45 {
            0
        } else {
            value
        };
        *pixel = Luma([adjusted]);
    }
}

fn choose_longer(first: String, second: String) -> String {
    if second.trim().chars().count() > first.trim().chars().count() { second } else { first }
}

fn limit_text(text: String, limit: usize) -> String {
    text.chars().take(limit).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageFormat, RgbImage};
    use std::io::Cursor;
    use std::time::Duration;

    fn config() -> ScamDetectConfig {
        ScamDetectConfig {
            enabled: true,
            queue_capacity: 1,
            workers: 1,
            max_images_per_message: 1,
            max_upload_mb: 1,
            download_timeout: Duration::from_secs(1),
            scan_timeout: Duration::from_secs(1),
            job_ttl: Duration::from_secs(1),
            max_image_width: 4,
            max_decoded_pixels: 16,
            ocr_text_limit: 4,
            ocr_min_chars_for_psm6: 20,
            ocr_max_concurrent: 1,
            block_threshold: 0.8,
            review_threshold: 0.55,
            tesseract_lang: "eng".to_owned(),
            tessdata_dir: None,
        }
    }

    fn png(width: u32, height: u32) -> Vec<u8> {
        let image = DynamicImage::ImageRgb8(RgbImage::new(width, height));
        let mut bytes = Cursor::new(Vec::new());
        image
            .write_to(&mut bytes, ImageFormat::Png)
            .unwrap();
        bytes.into_inner()
    }

    fn block_text_image() -> GrayImage {
        const GLYPHS: [[&str; 7]; 4] = [
            ["11111", "10000", "10000", "11111", "00001", "00001", "11111"],
            ["01110", "10001", "10000", "10000", "10000", "10001", "01110"],
            ["01110", "10001", "10001", "11111", "10001", "10001", "10001"],
            ["10001", "11011", "10101", "10101", "10001", "10001", "10001"],
        ];
        const SCALE: u32 = 12;
        const LEFT: u32 = 36;
        const TOP: u32 = 24;
        const GLYPH_WIDTH: u32 = 5 * SCALE;
        const GAP: u32 = SCALE;

        let mut image = GrayImage::from_pixel(360, 132, Luma([255]));
        for (glyph_index, glyph) in GLYPHS.iter().enumerate() {
            let glyph_x = LEFT + glyph_index as u32 * (GLYPH_WIDTH + GAP);
            for (row, pattern) in glyph.iter().enumerate() {
                for (column, bit) in pattern.bytes().enumerate() {
                    if bit != b'1' {
                        continue;
                    }
                    for y in 0..SCALE {
                        for x in 0..SCALE {
                            image.put_pixel(
                                glyph_x + column as u32 * SCALE + x,
                                TOP + row as u32 * SCALE + y,
                                Luma([0]),
                            );
                        }
                    }
                }
            }
        }
        image
    }

    #[test]
    fn rejects_images_over_decoded_pixel_limit() {
        let error = process_image_and_ocr(&png(5, 4), &config()).unwrap_err();

        assert!(
            error
                .to_string()
                .contains("exceeding configured 16 pixel limit")
        );
    }

    #[test]
    fn resizes_wide_images_and_preserves_aspect_ratio() {
        let resized = resize_if_needed(DynamicImage::ImageRgb8(RgbImage::new(8, 4)), 4);

        assert_eq!(resized.dimensions(), (4, 2));
    }

    #[test]
    fn chooses_longer_trimmed_ocr_output() {
        assert_eq!(
            choose_longer("one".to_owned(), "one two".to_owned()),
            "one two"
        );
        assert_eq!(
            choose_longer("same".to_owned(), "same".to_owned()),
            "same"
        );
    }

    #[test]
    fn truncates_text_by_character_count() {
        assert_eq!(limit_text("abกขค".to_owned(), 4), "abกข");
        assert_eq!(limit_text("text".to_owned(), 0), "");
    }

    #[test]
    fn invalid_tessdata_directory_fails_validation() {
        let mut config = config();
        config.tessdata_dir = Some("/definitely/not/tessdata".to_owned());

        assert!(validate(&config).is_err());
    }

    #[test]
    fn linked_tesseract_recognizes_a_smoke_image_end_to_end() {
        let mut bytes = Cursor::new(Vec::new());
        DynamicImage::ImageLuma8(block_text_image())
            .write_to(&mut bytes, ImageFormat::Png)
            .unwrap();
        let mut config = config();
        config.max_image_width = 1600;
        config.max_decoded_pixels = 16_777_216;
        let output = process_image_and_ocr(&bytes.into_inner(), &config)
            .expect("linked Tesseract should recognize a simple image");

        assert!(
            output
                .text
                .chars()
                .filter(char::is_ascii_alphabetic)
                .count()
                >= 3,
            "unexpected OCR output: {:?}",
            output.text
        );
        assert_eq!((output.width, output.height), (360, 132));
    }
}
