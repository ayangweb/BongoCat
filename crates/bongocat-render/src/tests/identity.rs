//! An id says which array it indexes, and is not interchangeable.

use super::*;

#[test]
fn strong_resource_ids_preserve_source_identity() {
    assert_eq!(DrawableId::new(7).index(), 7);
    assert_eq!(TextureId::new(2).index(), 2);
    assert_eq!(DrawableId::new(7).to_string(), "7");
    assert_eq!(TextureId::new(2).to_string(), "2");
}
