use gpui::{AssetSource, SharedString};
use std::borrow::Cow;

/// Resolve Seek UI artwork to the licensed reference Hugeicons family.
/// Kit assets remain available for components not yet exposed by Seek.
/// GPUI caches the embedded SVGs; this never reads files during rendering.
pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> anyhow::Result<Option<Cow<'static, [u8]>>> {
        if let Some(bytes) = crate::ui_icons::load(path) {
            return Ok(Some(Cow::Borrowed(bytes)));
        }
        if path == "seek/hidden-files.svg" {
            return Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/hidden-files.svg"
            ))));
        }
        let bytes = gpui_component_assets::AllAssets.load(path)?;
        Ok(bytes.map(|bytes| {
            if path.starts_with("icons/") && path.ends_with(".svg") {
                Cow::Owned(
                    String::from_utf8_lossy(&bytes)
                        .replace("stroke-width=\"2\"", "stroke-width=\"1.5\"")
                        .into_bytes(),
                )
            } else {
                bytes
            }
        }))
    }

    fn list(&self, path: &str) -> anyhow::Result<Vec<SharedString>> {
        gpui_component_assets::AllAssets.list(path)
    }
}
