//! Markdown images resolve against the opened document, including encoded local paths.
use gpui::{Image, ImageCacheError, ImageFormat, ImageSource, Resource, SharedUri};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
pub fn local_path(folder: &Path, source: &str) -> Option<PathBuf> {
    if Path::new(source).is_absolute() {
        return Some(PathBuf::from(source));
    }
    if let Ok(url) = url::Url::parse(source) {
        return (url.scheme() == "file")
            .then(|| url.to_file_path().ok())
            .flatten();
    }
    let base = url::Url::from_directory_path(folder).ok()?;
    base.join(source).ok()?.to_file_path().ok()
}
/// Remote images load only after the reader asks for them (Load images).
fn held() -> ImageSource {
    ImageSource::Custom(Arc::new(|_, _| {
        Some(Err(ImageCacheError::Asset(
            "Remote image: choose Load images".into(),
        )))
    }))
}
pub fn source(folder: &Path, uri: &SharedUri) -> ImageSource {
    let value = uri.as_ref();
    if let Ok(data) = data_url::DataUrl::process(value) {
        let mime = data.mime_type();
        if let Some(format) =
            ImageFormat::from_mime_type(&format!("{}/{}", mime.type_, mime.subtype))
            && let Ok((bytes, _)) = data.decode_to_vec()
        {
            return ImageSource::Image(Arc::new(Image::from_bytes(format, bytes)));
        }
    }
    if url::Url::parse(value).is_ok_and(|url| matches!(url.scheme(), "http" | "https")) {
        return ImageSource::Resource(Resource::Uri(uri.clone()));
    }
    if let Some(path) = local_path(folder, value) {
        return ImageSource::Resource(Resource::Path(Arc::from(path)));
    }
    ImageSource::Custom(Arc::new(|_, _| {
        Some(Err(ImageCacheError::Asset(
            "Unsupported image location".into(),
        )))
    }))
}
pub fn cached_source(
    folder: &Path,
    remote: bool,
    uri: &SharedUri,
    cache: &std::sync::Mutex<std::collections::HashMap<SharedUri, Arc<Image>>>,
) -> ImageSource {
    if !remote
        && url::Url::parse(uri.as_ref()).is_ok_and(|url| matches!(url.scheme(), "http" | "https"))
    {
        return held();
    }
    if !uri.as_ref().starts_with("data:") {
        return source(folder, uri);
    }
    let mut cache = cache.lock().unwrap();
    if let Some(image) = cache.get(uri) {
        return ImageSource::Image(image.clone());
    }
    let source = source(folder, uri);
    if let ImageSource::Image(image) = &source {
        if cache.len() >= 64
            && let Some(first) = cache.keys().next().cloned()
        {
            cache.remove(&first);
        }
        cache.insert(uri.clone(), image.clone());
    }
    source
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn relative_and_file_urls_preserve_spaces_and_unicode() {
        let folder = Path::new(r"C:\Owned\Notes");
        assert_eq!(
            local_path(folder, "images/picture%20one.png"),
            Some(folder.join(r"images\picture one.png"))
        );
        assert_eq!(
            local_path(folder, "../other.png"),
            Some(PathBuf::from(r"C:\Owned\other.png"))
        );
        let path = folder.join("\u{4e2d}.png");
        let url = url::Url::from_file_path(&path).unwrap();
        assert_eq!(local_path(folder, url.as_str()), Some(path));
    }
    #[test]
    fn http_and_data_images_load_while_unsupported_schemes_do_not() {
        for uri in [
            "https://example.com/picture.png",
            "http://localhost/image.png",
        ] {
            assert!(matches!(
                source(Path::new(r"C:\Owned"), &uri.to_owned().into()),
                ImageSource::Resource(Resource::Uri(_))
            ));
        }
        assert!(matches!(
            source(
                Path::new(r"C:\Owned"),
                &"data:image/png;base64,AA==".to_owned().into()
            ),
            ImageSource::Image(_)
        ));
        assert!(matches!(
            source(
                Path::new(r"C:\Owned"),
                &"javascript:alert(1)".to_owned().into()
            ),
            ImageSource::Custom(_)
        ));
    }
}
