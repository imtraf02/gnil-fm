use std::borrow::Cow;

use gpui::{AssetSource, SharedString};

macro_rules! asset_registry {
    ($($path:literal => $source:literal),+ $(,)?) => {
        const ASSET_PATHS: &[&str] = &[$($path),+];

        fn embedded_asset(path: &str) -> Option<&'static [u8]> {
            match path {
                $($path => Some(include_bytes!($source)),)+
                _ => None,
            }
        }
    };
}

asset_registry! {
    "brand/gnil-fm.svg" => "../../../assets/brand/gnil-fm.svg",
    "icons/folder-closed.svg" => "../../../assets/icons/folder-closed.svg",
    "icons/folder-open.svg" => "../../../assets/icons/folder-open.svg",
    "icons/folder-favorite.svg" => "../../../assets/icons/folder-favorite.svg",
    "icons/folder-symlink.svg" => "../../../assets/icons/folder-symlink.svg",
    "icons/folder-readonly.svg" => "../../../assets/icons/folder-readonly.svg",
    "icons/folder-downloads.svg" => "../../../assets/icons/folder-downloads.svg",
    "icons/folder-pictures.svg" => "../../../assets/icons/folder-pictures.svg",
    "icons/folder-documents.svg" => "../../../assets/icons/folder-documents.svg",
    "icons/folder-videos.svg" => "../../../assets/icons/folder-videos.svg",
    "icons/folder-music.svg" => "../../../assets/icons/folder-music.svg",
    "icons/folder-desktop.svg" => "../../../assets/icons/folder-desktop.svg",
    "icons/file-generic.svg" => "../../../assets/icons/file-generic.svg",
    "icons/file-code.svg" => "../../../assets/icons/file-code.svg",
    "icons/file-text.svg" => "../../../assets/icons/file-text.svg",
    "icons/file-image.svg" => "../../../assets/icons/file-image.svg",
    "icons/file-document.svg" => "../../../assets/icons/file-document.svg",
    "icons/file-archive.svg" => "../../../assets/icons/file-archive.svg",
    "icons/file-media.svg" => "../../../assets/icons/file-media.svg",
    "icons/empty-state.svg" => "../../../assets/icons/empty-state.svg",
    "icons/trash-empty.svg" => "../../../assets/icons/trash-empty.svg",
    "icons/trash.svg" => "../../../assets/icons/trash.svg",
    "icons/device-usb.svg" => "../../../assets/icons/device-usb.svg",
    "icons/device-drive.svg" => "../../../assets/icons/device-drive.svg",
    "icons/device-eject.svg" => "../../../assets/icons/device-eject.svg",
    "icons/place-home.svg" => "../../../assets/icons/place-home.svg",
    "icons/place-downloads.svg" => "../../../assets/icons/place-downloads.svg",
    "icons/place-pictures.svg" => "../../../assets/icons/place-pictures.svg",
    "icons/place-documents.svg" => "../../../assets/icons/place-documents.svg",
    "icons/place-videos.svg" => "../../../assets/icons/place-videos.svg",
    "icons/place-music.svg" => "../../../assets/icons/place-music.svg",
    "icons/place-desktop.svg" => "../../../assets/icons/place-desktop.svg",
    "icons/action-close.svg" => "../../../assets/icons/action-close.svg",
    "icons/action-back.svg" => "../../../assets/icons/action-back.svg",
    "icons/action-forward.svg" => "../../../assets/icons/action-forward.svg",
    "icons/action-up.svg" => "../../../assets/icons/action-up.svg",
    "icons/action-search.svg" => "../../../assets/icons/action-search.svg",
    "icons/action-new.svg" => "../../../assets/icons/action-new.svg",
    "icons/action-sort.svg" => "../../../assets/icons/action-sort.svg",
    "icons/action-layout-grid.svg" => "../../../assets/icons/action-layout-grid.svg",
    "icons/action-layout-list.svg" => "../../../assets/icons/action-layout-list.svg",
    "icons/action-panel-right.svg" => "../../../assets/icons/action-panel-right.svg",
    "icons/action-more.svg" => "../../../assets/icons/action-more.svg",
    "icons/action-restore.svg" => "../../../assets/icons/action-restore.svg",
    "icons/action-cut.svg" => "../../../assets/icons/action-cut.svg",
    "icons/action-copy.svg" => "../../../assets/icons/action-copy.svg",
    "icons/action-pause.svg" => "../../../assets/icons/action-pause.svg",
    "icons/action-play.svg" => "../../../assets/icons/action-play.svg",
    "icons/action-retry.svg" => "../../../assets/icons/action-retry.svg",
    "icons/action-paste.svg" => "../../../assets/icons/action-paste.svg",
    "icons/action-rename.svg" => "../../../assets/icons/action-rename.svg",
    "icons/action-link.svg" => "../../../assets/icons/action-link.svg",
    "icons/action-open-with.svg" => "../../../assets/icons/action-open-with.svg",
    "icons/action-permissions.svg" => "../../../assets/icons/action-permissions.svg",
    "icons/action-star.svg" => "../../../assets/icons/action-star.svg",
    "icons/action-star-filled.svg" => "../../../assets/icons/action-star-filled.svg",
    "icons/status-warning.svg" => "../../../assets/icons/status-warning.svg",
    "icons/settings-appearance.svg" => "../../../assets/icons/settings-appearance.svg",
    "icons/settings-keymap.svg" => "../../../assets/icons/settings-keymap.svg",
    "icons/settings-file-view.svg" => "../../../assets/icons/settings-file-view.svg",
    "icons/settings-performance.svg" => "../../../assets/icons/settings-performance.svg",
}

pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> anyhow::Result<Option<Cow<'static, [u8]>>> {
        Ok(embedded_asset(path).map(Cow::Borrowed))
    }

    fn list(&self, path: &str) -> anyhow::Result<Vec<SharedString>> {
        let prefix = format!("{path}/");
        Ok(ASSET_PATHS
            .iter()
            .filter_map(|asset| asset.strip_prefix(&prefix))
            .map(SharedString::from)
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::{ASSET_PATHS, Assets, embedded_asset};
    use gpui::AssetSource as _;

    #[test]
    fn registry_lists_every_embedded_icon() {
        let listed = Assets.list("icons").expect("icon list");
        let embedded = ASSET_PATHS
            .iter()
            .filter(|path| path.starts_with("icons/"))
            .count();
        assert_eq!(listed.len(), embedded);
    }

    #[test]
    fn functional_icons_follow_the_lucide_svg_contract() {
        for path in ASSET_PATHS.iter().filter(|path| {
            path.starts_with("icons/")
                && !matches!(**path, "icons/empty-state.svg" | "icons/trash-empty.svg")
        }) {
            let source = std::str::from_utf8(
                embedded_asset(path).expect("registered icon must have embedded bytes"),
            )
            .expect("SVG must be UTF-8");
            assert!(source.contains("viewBox=\"0 0 24 24\""), "{path}");
            assert!(source.contains("stroke=\"currentColor\""), "{path}");
            assert!(source.contains("stroke-width=\"2\""), "{path}");
            assert!(!source.contains("<linearGradient"), "{path}");
            assert!(!source.contains("<filter"), "{path}");
        }
    }
}
