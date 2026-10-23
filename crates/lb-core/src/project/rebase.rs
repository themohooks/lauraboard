//! Every file path a channel stores (library media, graphics images and
//! fonts, standby picture), so they can be rewritten when the channel's own
//! folder moves (id changed, channel duplicated or imported).

use super::{CHANNELS_DIR, ChannelData};
use crate::graphics::ElementKind;

/// Stored-path prefix of files kept in a channel's own folder
/// (`channels/<id>/`).
pub fn channel_prefix(id: &str) -> String {
    format!("{CHANNELS_DIR}/{id}/")
}

impl ChannelData {
    /// Visit every stored file path of the channel.
    pub fn for_each_path_mut(&mut self, mut f: impl FnMut(&mut String)) {
        for m in &mut self.library.items {
            f(&mut m.path);
        }
        for e in &mut self.graphics.elements {
            if let ElementKind::Image { path } = &mut e.kind {
                f(path);
            }
            if let Some(img) = e.kind.shape_mut().and_then(|s| s.image.as_mut()) {
                f(img);
            }
            for font in e.kind.styles_mut().into_iter().filter_map(|s| s.font.as_mut()) {
                f(font);
            }
        }
        let fb = &mut self.config.fallback;
        for p in [&mut fb.image, &mut fb.media].into_iter().flatten() {
            f(p);
        }
    }

    /// Point paths inside `channels/<from>/` to `channels/<to>/`.
    pub fn rebase_paths(&mut self, from: &str, to: &str) {
        let (old, new) = (channel_prefix(from), channel_prefix(to));
        self.for_each_path_mut(|p| {
            if let Some(rest) = p.strip_prefix(&old) {
                *p = format!("{new}{rest}");
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ChannelConfig;
    use crate::media::{MediaItem, MediaKind};
    use crate::{Dur, media::Library};

    #[test]
    fn rebases_only_own_files() {
        let mut d = ChannelData::new("x".into(), ChannelConfig::new("a"));
        d.library = Library::new(vec![
            MediaItem::new("own", "channels/a/media/1.mp4", MediaKind::Program, Dur::secs(1)),
            MediaItem::new("shared", "media/2.mp4", MediaKind::Program, Dur::secs(1)),
            MediaItem::new("other", "channels/ab/media/3.mp4", MediaKind::Program, Dur::secs(1)),
        ]);
        d.config.fallback.image = Some("channels/a/media/standby.png".into());
        d.rebase_paths("a", "b");
        let p: Vec<_> = d.library.items.iter().map(|m| m.path.as_str()).collect();
        assert_eq!(p, ["channels/b/media/1.mp4", "media/2.mp4", "channels/ab/media/3.mp4"]);
        assert_eq!(d.config.fallback.image.as_deref(), Some("channels/b/media/standby.png"));
    }
}
