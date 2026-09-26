//! What a thing is, by name.
//!
//! A texture id and a drawable id are indexes into arrays the renderer owns, so
//! printing one has to say which array it indexes into: a log line that said only
//! "3" would be unreadable in exactly the situation a reader needs it. The ids are
//! distinct newtypes rather than bare integers so a texture cannot be passed where
//! a drawable is expected.

use super::*;

impl fmt::Display for DrawableId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct TextureId(pub(crate) usize);

impl TextureId {
    pub const fn new(index: usize) -> Self {
        Self(index)
    }

    pub const fn index(self) -> usize {
        self.0
    }
}

impl fmt::Display for TextureId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}
