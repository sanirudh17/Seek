//! Bounded PresentationML reading. Never executes macros, external links or OLE.
use quick_xml::{Reader, events::Event};
use std::{
    collections::HashMap,
    io::{Cursor, Read},
};
/// The package's saved first-slide image is useful without an Office provider.
/// It is explicitly labelled as a saved preview, not a freshly rendered slide.
pub fn thumbnail(bytes: &[u8], edge: u16) -> Option<(u32, u32, Vec<u8>)> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).ok()?;
    for name in [
        "docProps/thumbnail.png",
        "docProps/thumbnail.jpeg",
        "docProps/thumbnail.jpg",
    ] {
        let Ok(entry) = archive.by_name(name) else {
            continue;
        };
        if entry.size() > 2 * 1024 * 1024 {
            continue;
        }
        let mut encoded = Vec::new();
        entry
            .take(2 * 1024 * 1024 + 1)
            .read_to_end(&mut encoded)
            .ok()?;
        let mut reader = image::ImageReader::new(Cursor::new(encoded))
            .with_guessed_format()
            .ok()?;
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(4096);
        limits.max_image_height = Some(4096);
        limits.max_alloc = Some(32 * 1024 * 1024);
        reader.limits(limits);
        let decoded = reader
            .decode()
            .ok()?
            .thumbnail(u32::from(edge), u32::from(edge))
            .into_rgba8();
        let (width, height) = decoded.dimensions();
        let mut pixels = decoded.into_raw();
        for pixel in pixels.chunks_exact_mut(4) {
            pixel.swap(0, 2);
        }
        return Some((width, height, pixels));
    }
    None
}
fn part(archive: &mut zip::ZipArchive<Cursor<&[u8]>>, name: &str) -> anyhow::Result<String> {
    let entry = archive.by_name(name)?;
    anyhow::ensure!(entry.size() <= 4 * 1024 * 1024, "Slide XML exceeds limit");
    let mut xml = String::new();
    entry.take(4 * 1024 * 1024 + 1).read_to_string(&mut xml)?;
    anyhow::ensure!(xml.len() <= 4 * 1024 * 1024, "Slide XML exceeds limit");
    Ok(xml)
}
fn attr(e: &quick_xml::events::BytesStart<'_>, name: &[u8]) -> anyhow::Result<Option<String>> {
    for a in e.attributes() {
        let a = a?;
        if a.key.as_ref() == name {
            return Ok(Some(a.unescape_value()?.into_owned()));
        }
    }
    Ok(None)
}
pub fn slide(bytes: &[u8], page: u32) -> anyhow::Result<(u32, String)> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))?;
    let rels = part(&mut archive, "ppt/_rels/presentation.xml.rels")?;
    let mut reader = Reader::from_str(&rels);
    let mut targets = HashMap::new();
    loop {
        match reader.read_event()? {
            Event::Start(e) | Event::Empty(e) if e.local_name().as_ref() == b"Relationship" => {
                if attr(&e, b"TargetMode")?.as_deref() == Some("External") {
                    continue;
                }
                let Some(id) = attr(&e, b"Id")? else {
                    continue;
                };
                let Some(target) = attr(&e, b"Target")? else {
                    continue;
                };
                let target = target
                    .strip_prefix("/ppt/")
                    .or_else(|| target.strip_prefix("ppt/"))
                    .unwrap_or(&target);
                if target.starts_with("slides/")
                    && target.ends_with(".xml")
                    && !target.split('/').any(|s| s == ".." || s.is_empty())
                    && !target.contains(['\\', ':'])
                {
                    targets.insert(id, format!("ppt/{target}"));
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    let presentation = part(&mut archive, "ppt/presentation.xml")?;
    let mut reader = Reader::from_str(&presentation);
    let mut slides = Vec::new();
    loop {
        match reader.read_event()? {
            Event::Start(e) | Event::Empty(e) if e.local_name().as_ref() == b"sldId" => {
                // Relationship IDs are namespace-qualified; the numeric slide id is unrelated.
                let id = e
                    .attributes()
                    .filter_map(Result::ok)
                    .find(|a| a.key.local_name().as_ref() == b"id" && a.key.as_ref() != b"id")
                    .map(|a| a.unescape_value().map(|v| v.into_owned()))
                    .transpose()?;
                let target = id
                    .and_then(|id| targets.get(&id).cloned())
                    .ok_or_else(|| anyhow::anyhow!("Missing slide relationship"))?;
                slides.push(target);
            }
            Event::Eof => break,
            _ => {}
        }
    }
    let name = slides
        .get(page as usize)
        .ok_or_else(|| anyhow::anyhow!("Slide out of range"))?;
    let xml = part(&mut archive, name)?;
    let mut reader = Reader::from_str(&xml);
    let mut in_text = false;
    let mut text = String::new();
    loop {
        match reader.read_event()? {
            Event::Start(e) if e.local_name().as_ref() == b"t" => in_text = true,
            Event::End(e) if e.local_name().as_ref() == b"t" => in_text = false,
            Event::End(e) if e.local_name().as_ref() == b"p" => text.push('\n'),
            Event::Empty(e) if e.local_name().as_ref() == b"br" => text.push('\n'),
            Event::Text(e) if in_text => text.push_str(&e.decode()?),
            Event::CData(e) if in_text => text.push_str(&e.decode()?),
            Event::GeneralRef(e) if in_text => {
                text.push_str(&quick_xml::escape::unescape(&format!("&{};", e.decode()?))?)
            }
            Event::Eof => break,
            _ => {}
        }
    }
    let text = text.trim().chars().take(24000).collect::<String>();
    Ok((
        slides.len() as u32,
        if text.is_empty() {
            "This slide has no extractable text. Open the presentation to see its artwork.".into()
        } else {
            text
        },
    ))
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    #[test]
    #[ignore = "Requires an explicitly selected read-only presentation via SEEK_TEST_PREVIEW_PPT"]
    fn selected_real_presentation_first_and_last_slides() {
        let path = std::env::var_os("SEEK_TEST_PREVIEW_PPT")
            .expect("Set SEEK_TEST_PREVIEW_PPT to the presentation to inspect");
        let bytes = std::fs::read(path).unwrap();
        crate::document_preview::check_archive(&bytes).unwrap();
        let start = std::time::Instant::now();
        let (pages, text) = slide(&bytes, 0).unwrap();
        assert!(pages > 0 && !text.is_empty());
        let (same, last) = slide(&bytes, pages - 1).unwrap();
        assert_eq!(same, pages);
        assert!(!last.is_empty());
        let thumb = thumbnail(&bytes, 768);
        eprintln!(
            "Selected presentation: {pages} slides, saved preview {:?}, first/last + thumbnail checks {:.3} ms (test harness)",
            thumb.as_ref().map(|(w, h, _)| (*w, *h)),
            start.elapsed().as_secs_f64() * 1000.
        );
    }
    #[test]
    fn slide_order_relationships_unicode_and_entities() {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, text) in [
            (
                "ppt/presentation.xml",
                r#"<p:presentation xmlns:p="p" xmlns:r="r"><p:sldIdLst><p:sldId id="8" r:id="second"/><p:sldId id="9" r:id="first"/></p:sldIdLst></p:presentation>"#,
            ),
            (
                "ppt/_rels/presentation.xml.rels",
                r#"<Relationships><Relationship Id="first" Target="slides/slide1.xml"/><Relationship Id="second" Target="slides/slide9.xml"/><Relationship Id="external" Target="https://example.invalid" TargetMode="External"/></Relationships>"#,
            ),
            ("ppt/slides/slide1.xml", "<s><p><t>Last</t></p></s>"),
            ("ppt/slides/slide9.xml", "<s><p><t>Café &amp; Ω</t></p></s>"),
        ] {
            zip.start_file(name, zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(text.as_bytes()).unwrap();
        }
        let bytes = zip.finish().unwrap().into_inner();
        assert_eq!(slide(&bytes, 0).unwrap(), (2, "Café & Ω".into()));
        assert_eq!(slide(&bytes, 1).unwrap(), (2, "Last".into()));
        assert!(slide(&bytes, 2).is_err());
        assert!(slide(b"broken", 0).is_err());
    }
    #[test]
    fn saved_preview_decodes_bounded_pixels_and_broken_images_fall_back() {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let mut png = Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
            32,
            16,
            image::Rgba([255, 0, 0, 255]),
        ))
        .write_to(&mut png, image::ImageFormat::Png)
        .unwrap();
        zip.start_file(
            "docProps/thumbnail.png",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        zip.write_all(png.get_ref()).unwrap();
        let bytes = zip.finish().unwrap().into_inner();
        let (w, h, pixels) = thumbnail(&bytes, 16).unwrap();
        assert_eq!((w, h), (16, 8));
        assert_eq!(&pixels[..4], &[0, 0, 255, 255]);
        assert!(thumbnail(b"broken", 768).is_none());
    }
}
