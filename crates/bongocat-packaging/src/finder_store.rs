//! The Finder window a mounted disk image opens with.
//!
//! A disk image's window layout is not in its file system: Finder keeps it in a
//! `.DS_Store` database in the volume root and reads that when the volume is
//! opened. An image without one gets Finder's own defaults — a window sized to
//! its contents, 48 px icons, and items laid out alphabetically — which is what
//! a plain `hdiutil create -srcfolder` image produces.
//!
//! The alternative is what `create-dmg`, and therefore Tauri, does: mount the
//! image and drive Finder with AppleScript. That needs a graphical login
//! session and an Automation consent prompt, so it cannot run in the unattended
//! release job, and a build that is declined leaves a half-arranged image
//! behind. The database is written here instead, at build time, from the same
//! numbers a `create-dmg` invocation would pass.
//!
//! The format is undocumented by Apple and is described here only as far as one
//! volume root needs it. The store is a buddy-allocator B-tree: a 4-byte file
//! header, a 32-byte `Bud1` prelude, and three allocated blocks — the directory
//! that points at the tree root, the tree's single leaf page, and the allocator's
//! block table. Five records fit in one page, so no second page and no separator
//! keys are needed, and the smallest possible allocator bookkeeping block is used.
//!
//! The records, all on the volume root `.` unless a name says otherwise:
//!
//! | Field  | Name            | What it holds                                    |
//! | ------ | --------------- | ------------------------------------------------ |
//! | `bwsp` | `.`             | window bounds, and the Finder chrome to hide     |
//! | `icvp` | `.`             | icon view settings, including the icon size      |
//! | `vSrn` | `.`             | the view settings version Finder writes          |
//! | `Iloc` | `app`           | where the application bundle's icon sits         |
//! | `Iloc` | `Applications`  | where the drop link's icon sits                  |
//!
//! `bwsp` and `icvp` carry binary property lists, and `plist` writes those.
//!
//! Verification: `ds_parser 0.4.0`, an independent implementation, parses the
//! bytes this module produces without a single warning, and Finder on macOS 27
//! applies every value in them — both icon positions, the window bounds, the
//! hidden chrome, and the icon and label sizes. A store Finder cannot read is
//! ignored rather than acted on, so a malformed one costs the installer its
//! layout and nothing else.

use std::cmp::Ordering;

use plist::{Dictionary, Value};

/// The file name Finder looks for in a directory.
pub const FILE_NAME: &str = ".DS_Store";

/// The tree page a volume root's records are read from.
///
/// The directory block advertises this size, and it is also the limit the single
/// leaf is checked against: a layout that does not fit here would need a real
/// multi-page tree, which this module does not write.
const PAGE_SIZE: usize = 4096;

/// The size of the `Bud1` prelude that opens the data region.
const PRELUDE_SIZE: usize = 32;

/// The four-byte file header: the alignment the format documents, big-endian.
const FILE_HEADER: u32 = 1;

/// The smallest block the allocator hands out.
const MINIMUM_BLOCK: usize = 32;

/// How many block addresses the allocator's table reserves.
const RESERVED_BLOCKS: usize = 256;

/// How many free-list buckets the allocator's table reserves.
const FREE_LIST_BUCKETS: usize = 32;

/// The name of the directory block in the allocator's table of contents.
const DIRECTORY_NAME: &[u8] = b"DSDB";

/// A window layout that cannot be written.
///
/// The name of a record is part of the page it is written to, so a name too long
/// for the format's 32-bit length field is reported the same way as a layout
/// that outgrows the page: neither can be stored.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(
        "the Finder window layout does not fit in one {page}-byte .DS_Store page ({needed} bytes)"
    )]
    TooLarge { needed: usize, page: usize },
    #[error("could not encode the Finder property list: {0}")]
    PropertyList(#[from] plist::Error),
}

/// The window a BongoCat disk image opens with, in Finder's icon-view
/// coordinates: the origin is the window's bottom-left corner on the screen, and
/// the positions are measured from the top-left of the window's contents.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WindowLayout {
    /// Window width, in points.
    pub width: u32,
    /// Window height, in points.
    pub height: u32,
    /// Where the window opens on the screen.
    pub origin: (u32, u32),
    /// Where the application bundle's icon sits in the window.
    pub app: (u32, u32),
    /// Where the `/Applications` drop link's icon sits in the window.
    pub applications: (u32, u32),
    /// Icon edge length, in points.
    pub icon_size: u32,
    /// Label size, in points.
    pub text_size: u32,
}

impl Default for WindowLayout {
    /// The window Tauri v2 opens by default, which is the one this product
    /// ships.
    ///
    /// The size and the two icon positions are Tauri v2's documented
    /// `bundle.macOS.dmg` defaults. The origin, the icon size and the label size
    /// are `create-dmg`'s own defaults, which is what Tauri gets for not passing
    /// `windowPosition`, `--icon-size` or `--text-size`: it builds its disk image
    /// by running that script, so these are the numbers its default image opens
    /// with.
    fn default() -> Self {
        Self {
            width: 660,
            height: 400,
            origin: (10, 60),
            app: (180, 170),
            applications: (480, 170),
            icon_size: 128,
            text_size: 16,
        }
    }
}

/// Encodes the `.DS_Store` a volume holding `app` and an `applications` drop
/// link opens with.
///
/// The two names are the names Finder will see in the volume root, so an icon
/// position only applies to a file that is actually there under that name.
pub fn window(layout: &WindowLayout, app: &str, applications: &str) -> Result<Vec<u8>, Error> {
    let mut records = vec![
        record(".", b"bwsp", Payload::plist(window_settings(layout))?)?,
        record(".", b"icvp", Payload::plist(icon_view_settings(layout))?)?,
        record(".", b"vSrn", Payload::long(1))?,
        record(
            applications,
            b"Iloc",
            Payload::icon_position(layout.applications),
        )?,
        record(app, b"Iloc", Payload::icon_position(layout.app))?,
    ];
    // Finder reads the tree in key order, so the records have to be written that
    // way whatever order they were built in.
    records.sort_by(Record::order);

    let leaf = leaf_page(&records);
    if leaf.len() > PAGE_SIZE {
        return Err(Error::TooLarge {
            needed: leaf.len(),
            page: PAGE_SIZE,
        });
    }
    let directory = directory(records.len());

    // Block layout of the data region, which starts after the file header: the
    // prelude, then the three blocks, each 32-byte aligned and allocated a power
    // of two.
    let directory_offset = PRELUDE_SIZE as u32;
    let directory_block = block(directory.len());
    let leaf_offset = directory_offset + directory_block;
    let leaf_block = block(leaf.len());
    let table_offset = leaf_offset + leaf_block;
    let table_block = block(block_table_size());

    let table = block_table([
        address(table_offset, table_block),
        address(directory_offset, directory_block),
        address(leaf_offset, leaf_block),
    ]);

    let mut data = prelude(table_offset, table_block, address(leaf_offset, leaf_block));
    data.extend_from_slice(&directory);
    data.resize(directory_offset as usize + directory_block as usize, 0);
    data.extend_from_slice(&leaf);
    data.resize(leaf_offset as usize + leaf_block as usize, 0);
    data.extend_from_slice(&table);
    data.resize(table_offset as usize + table_block as usize, 0);

    // The block addresses in the table point into the data region, and the file
    // header sits in front of it.
    let mut file = FILE_HEADER.to_be_bytes().to_vec();
    file.extend_from_slice(&data);
    Ok(file)
}

/// The `Bud1` prelude: where the block table is, how big it is, the same offset
/// again, and the leaf's block address.
fn prelude(table_offset: u32, table_block: u32, leaf_address: u32) -> Vec<u8> {
    let mut prelude = Vec::with_capacity(PRELUDE_SIZE);
    prelude.extend_from_slice(b"Bud1");
    prelude.extend_from_slice(&table_offset.to_be_bytes());
    prelude.extend_from_slice(&table_block.to_be_bytes());
    prelude.extend_from_slice(&table_offset.to_be_bytes());
    prelude.extend_from_slice(&leaf_address.to_be_bytes());
    prelude.resize(PRELUDE_SIZE, 0);
    prelude
}

/// The directory block: the tree root's block index, the separator count a leaf
/// has none of, the record count, how many nodes the tree has, and the page size.
fn directory(records: usize) -> Vec<u8> {
    let mut directory = Vec::new();
    directory.extend_from_slice(&TREE_ROOT.to_be_bytes());
    directory.extend_from_slice(&0_u32.to_be_bytes());
    directory.extend_from_slice(&record_count(records).to_be_bytes());
    directory.extend_from_slice(&TREE_NODES.to_be_bytes());
    directory.extend_from_slice(&page_size().to_be_bytes());
    directory
}

/// How many records the directory block reports.
fn record_count(records: usize) -> u32 {
    u32::try_from(records).expect("a .DS_Store record count fits the 32-bit field")
}

/// The page size as the format's 32-bit field carries it.
fn page_size() -> u32 {
    u32::try_from(PAGE_SIZE).expect("a .DS_Store page is far below the 32-bit field")
}

/// The block the tree root is: the table, the directory and the leaf page are
/// blocks 0, 1 and 2, and the root is the leaf.
const TREE_ROOT: u32 = 2;

/// How many nodes the tree has, which is the one leaf page: the records of a
/// volume root fit in it, so the tree is never taller than one level.
const TREE_NODES: u32 = 1;

/// The single leaf page holding every record: a separator count a leaf has none
/// of, the record count, then the records themselves.
fn leaf_page(records: &[Record]) -> Vec<u8> {
    let mut leaf = Vec::new();
    leaf.extend_from_slice(&0_u32.to_be_bytes());
    leaf.extend_from_slice(&(records.len() as u32).to_be_bytes());
    for record in records {
        leaf.extend_from_slice(&record.encode());
    }
    leaf
}

/// The allocator's block table: the three block addresses, the table of contents
/// naming the directory block, and the empty free list.
fn block_table(addresses: [u32; 3]) -> Vec<u8> {
    let mut table = Vec::new();
    table.extend_from_slice(&(addresses.len() as u32).to_be_bytes());
    table.extend_from_slice(&0_u32.to_be_bytes());
    for address in addresses {
        table.extend_from_slice(&address.to_be_bytes());
    }
    table.resize(8 + RESERVED_BLOCKS * 4, 0);
    table.extend_from_slice(&1_u32.to_be_bytes());
    table.push(DIRECTORY_NAME.len() as u8);
    table.extend_from_slice(DIRECTORY_NAME);
    table.extend_from_slice(&1_u32.to_be_bytes());
    table.resize(table.len() + FREE_LIST_BUCKETS * 4, 0);
    table
}

/// How long the block table is: the block count, the reserved block addresses,
/// the one table-of-contents entry, and the free list.
fn block_table_size() -> usize {
    8 + RESERVED_BLOCKS * 4 + 4 + 1 + DIRECTORY_NAME.len() + 4 + FREE_LIST_BUCKETS * 4
}

/// The size the allocator gives a block holding `length` bytes.
fn block(length: usize) -> u32 {
    u32::try_from(length.max(MINIMUM_BLOCK).next_power_of_two())
        .expect("a .DS_Store block is a page-sized length")
}

/// A block address: the block's offset in the data region, with log2 of its
/// size in the low five bits.
fn address(offset: u32, size: u32) -> u32 {
    (offset & !(MINIMUM_BLOCK as u32 - 1)) | size.trailing_zeros()
}

/// The `bwsp` window settings: the window's bounds, and the Finder chrome the
/// installer window does not have.
///
/// The path bar is deliberately not hidden. It is what names the mounted volume
/// in the window, Tauri leaves it visible, and hiding it is not one of the
/// statements a `create-dmg` window is arranged with.
fn window_settings(layout: &WindowLayout) -> Dictionary {
    let mut settings = Dictionary::new();
    settings.insert("ContainerShowSidebar".to_owned(), false.into());
    settings.insert("ShowSidebar".to_owned(), false.into());
    settings.insert("ShowStatusBar".to_owned(), false.into());
    settings.insert("ShowTabView".to_owned(), false.into());
    settings.insert("ShowToolbar".to_owned(), false.into());
    settings.insert(
        "WindowBounds".to_owned(),
        appkit_rectangle(layout.origin, (layout.width, layout.height)).into(),
    );
    settings
}

/// The `icvp` icon view settings.
///
/// The key set is the one Finder itself writes for an icon view, and it is
/// complete on purpose: with the background and grid keys left out, Finder stops
/// honouring `iconSize` and opens the window with its own 48 px icons (measured
/// on macOS 27). `backgroundType` 0 with a white background is what an icon
/// view without a background picture is, and `arrangeBy` "none" is what leaves
/// the layout to the `Iloc` icon positions.
fn icon_view_settings(layout: &WindowLayout) -> Dictionary {
    let mut settings = Dictionary::new();
    settings.insert("arrangeBy".to_owned(), "none".into());
    settings.insert("backgroundColorBlue".to_owned(), 1.0_f64.into());
    settings.insert("backgroundColorGreen".to_owned(), 1.0_f64.into());
    settings.insert("backgroundColorRed".to_owned(), 1.0_f64.into());
    settings.insert("backgroundType".to_owned(), 0_i32.into());
    settings.insert("gridOffsetX".to_owned(), 0.0_f64.into());
    settings.insert("gridOffsetY".to_owned(), 0.0_f64.into());
    settings.insert("gridSpacing".to_owned(), 54.0_f64.into());
    settings.insert("iconSize".to_owned(), f64::from(layout.icon_size).into());
    settings.insert("labelOnBottom".to_owned(), true.into());
    settings.insert("showIconPreview".to_owned(), true.into());
    settings.insert("showItemInfo".to_owned(), false.into());
    settings.insert("textSize".to_owned(), f64::from(layout.text_size).into());
    settings.insert("viewOptionsVersion".to_owned(), 1_i32.into());
    settings
}

/// The string Finder stores `WindowBounds` as: a nested brace pair around the
/// origin and the size, as in `{{10, 60}, {660, 400}}`, rather than a plain
/// four-number rectangle.
fn appkit_rectangle(origin: (u32, u32), size: (u32, u32)) -> String {
    let (x, y) = origin;
    let (width, height) = size;
    format!("{{{{{}, {}}}, {{{}, {}}}}}", x, y, width, height)
}

/// One `.DS_Store` record.
struct Record {
    /// The item the record belongs to, as UTF-16 code units: Finder keys its
    /// tree on that form and stores the name in it.
    name: Vec<u16>,
    /// How many code units the name has, which the record stores as a 32-bit
    /// length before it.
    name_length: u32,
    /// Finder's field code.
    code: &'static [u8; 4],
    /// The record's value.
    value: Payload,
}

/// Builds a record, rejecting a name the format's 32-bit length cannot hold.
fn record(name: &str, code: &'static [u8; 4], value: Payload) -> Result<Record, Error> {
    let name: Vec<u16> = name.encode_utf16().collect();
    let name_length = u32::try_from(name.len()).map_err(|_| Error::TooLarge {
        needed: name.len(),
        page: PAGE_SIZE,
    })?;
    Ok(Record {
        name,
        name_length,
        code,
        value,
    })
}

impl Record {
    /// Finder's record order: by name, then by field code.
    fn order(left: &Self, right: &Self) -> Ordering {
        left.name
            .cmp(&right.name)
            .then_with(|| left.code.cmp(right.code))
    }

    /// The name, the field code, the value's type tag, and the value.
    fn encode(&self) -> Vec<u8> {
        let mut encoded = Vec::new();
        encoded.extend_from_slice(&self.name_length.to_be_bytes());
        for unit in &self.name {
            encoded.extend_from_slice(&unit.to_be_bytes());
        }
        encoded.extend_from_slice(self.code);
        encoded.extend_from_slice(self.value.type_tag());
        match &self.value {
            // Only the length-prefixed values carry a length; `long` is the
            // four bytes themselves.
            Payload::Blob(blob) => {
                encoded.extend_from_slice(&(blob.len() as u32).to_be_bytes());
                encoded.extend_from_slice(blob);
            }
            Payload::Long(value) => encoded.extend_from_slice(&value.to_be_bytes()),
        }
        encoded
    }
}

/// One record's value, in the encoding Finder reads.
enum Payload {
    /// A length-prefixed value, which is how `Iloc`, `bwsp` and `icvp` carry
    /// theirs.
    Blob(Vec<u8>),
    /// A 32-bit integer.
    Long(u32),
}

impl Payload {
    /// An icon position (`Iloc`): the two coordinates, then the eight trailing
    /// bytes whose meaning varies between Finder versions. Finder writes
    /// `ff ff ff ff ff ff 00 00` here, and reads positions out of records it
    /// wrote itself.
    fn icon_position((x, y): (u32, u32)) -> Self {
        let mut blob = Vec::with_capacity(16);
        blob.extend_from_slice(&x.to_be_bytes());
        blob.extend_from_slice(&y.to_be_bytes());
        blob.extend_from_slice(&[0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00, 0x00]);
        Self::Blob(blob)
    }

    /// A 32-bit integer (`long`).
    fn long(value: u32) -> Self {
        Self::Long(value)
    }

    /// A binary property list, which is how `bwsp` and `icvp` carry settings.
    fn plist(settings: Dictionary) -> Result<Self, Error> {
        let mut blob = Vec::new();
        Value::from(settings).to_writer_binary(&mut blob)?;
        Ok(Self::Blob(blob))
    }

    /// The value's on-disk type tag, which every record carries.
    fn type_tag(&self) -> &'static [u8; 4] {
        match self {
            Self::Blob(_) => b"blob",
            Self::Long(_) => b"long",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Error, FILE_NAME, PAGE_SIZE, Payload, WindowLayout, block, window};
    use plist::Value;

    /// The window the product ships, spelled out so the defaults have something
    /// to be compared against.
    const SHIPPED: WindowLayout = WindowLayout {
        width: 660,
        height: 400,
        origin: (10, 60),
        app: (180, 170),
        applications: (480, 170),
        icon_size: 128,
        text_size: 16,
    };
    const APP: &str = "BongoCat.app";
    const APPLICATIONS: &str = "Applications";

    /// One record read back out of an encoded store, as the bytes Finder sees.
    struct Decoded {
        name: String,
        code: String,
        /// The value, without the length prefix a blob carries.
        value: Vec<u8>,
    }

    impl Decoded {
        /// The value read as an icon position.
        fn icon_position(&self) -> (u32, u32) {
            assert_eq!(self.value.len(), 16, "an icon position is sixteen bytes");
            let coordinate = |at: usize| {
                u32::from_be_bytes(
                    self.value[at..at + 4]
                        .try_into()
                        .expect("a four-byte coordinate"),
                )
            };
            (coordinate(0), coordinate(4))
        }

        /// The value read as a binary property list, which decodes the blob this
        /// module wrote with the property list parser rather than with itself.
        fn settings(&self) -> plist::Dictionary {
            plist::from_bytes(&self.value).expect("the settings are a binary property list")
        }
    }

    /// Reads a big-endian `u32` out of `bytes` at `at`.
    fn be32(bytes: &[u8], at: usize) -> u32 {
        u32::from_be_bytes(
            bytes[at..at + 4]
                .try_into()
                .expect("a four-byte field is present"),
        )
    }

    /// The records of an encoded store, in the order its leaf page holds them.
    fn decode(store: &[u8]) -> Vec<Decoded> {
        // The prelude names the leaf page's block address, which packs the page's
        // offset in the data region with log2 of its size. Data-region offsets
        // count from the four-byte file header.
        let leaf = ((be32(store, 20) & !0x1f) as usize) + 4;

        let mut records = Vec::new();
        let mut at = leaf + 8;
        for _ in 0..be32(store, leaf + 4) {
            let mut name = String::new();
            let name_length = be32(store, at);
            at += 4;
            for _ in 0..name_length {
                let unit = u16::from_be_bytes(
                    store[at..at + 2]
                        .try_into()
                        .expect("a two-byte name unit is present"),
                );
                name.push(char::from_u32(u32::from(unit)).expect("a UTF-16 unit"));
                at += 2;
            }
            let code = String::from_utf8(store[at..at + 4].to_vec()).expect("a field code");
            let tag = String::from_utf8(store[at + 4..at + 8].to_vec()).expect("a type tag");
            at += 8;
            let length = match tag.as_str() {
                "blob" => {
                    let length = be32(store, at) as usize;
                    at += 4;
                    length
                }
                "long" => 4,
                other => panic!("unexpected value type tag {other}"),
            };
            records.push(Decoded {
                name,
                code,
                value: store[at..at + length].to_vec(),
            });
            at += length;
        }
        records
    }

    /// The one record with `code` for `name`.
    fn record<'a>(records: &'a [Decoded], name: &str, code: &str) -> &'a Decoded {
        records
            .iter()
            .find(|record| record.name == name && record.code == code)
            .unwrap_or_else(|| panic!("no {code} record for {name}"))
    }

    /// The store the product ships, encoded.
    fn store() -> Vec<u8> {
        window(&SHIPPED, APP, APPLICATIONS).expect("the shipped layout fits")
    }

    #[test]
    fn the_shipped_window_is_the_one_tauri_opens_by_default() {
        assert_eq!(WindowLayout::default(), SHIPPED);
    }

    #[test]
    fn the_store_starts_with_the_file_header_and_the_allocator_prelude() {
        let store = store();
        assert_eq!(&store[0..4], &1_u32.to_be_bytes());
        assert_eq!(&store[4..8], b"Bud1");
    }

    #[test]
    fn records_are_written_in_the_order_finder_reads_them() {
        let keys: Vec<(String, String)> = decode(&store())
            .iter()
            .map(|record| (record.name.clone(), record.code.clone()))
            .collect();
        // Names compare as UTF-16 code units and then by field code, which puts
        // the volume root's records first, `bwsp` before `icvp` before `vSrn`.
        assert_eq!(
            keys,
            [
                (".".to_owned(), "bwsp".to_owned()),
                (".".to_owned(), "icvp".to_owned()),
                (".".to_owned(), "vSrn".to_owned()),
                (APPLICATIONS.to_owned(), "Iloc".to_owned()),
                (APP.to_owned(), "Iloc".to_owned()),
            ]
        );
    }

    #[test]
    fn both_items_get_the_position_the_layout_asks_for() {
        let records = decode(&store());
        let app = record(&records, APP, "Iloc");
        assert_eq!(app.icon_position(), (180, 170));
        // The eight trailing bytes are the ones Finder writes itself.
        assert_eq!(
            app.value[8..],
            [0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00, 0x00]
        );
        assert_eq!(
            record(&records, APPLICATIONS, "Iloc").icon_position(),
            (480, 170)
        );
    }

    #[test]
    fn the_window_settings_size_the_window_and_hide_the_finder_chrome() {
        let settings = record(&decode(&store()), ".", "bwsp").settings();
        assert_eq!(
            settings.get("WindowBounds").and_then(Value::as_string),
            Some("{{10, 60}, {660, 400}}")
        );
        for hidden in ["ShowSidebar", "ShowStatusBar", "ShowTabView", "ShowToolbar"] {
            assert_eq!(
                settings.get(hidden).and_then(Value::as_boolean),
                Some(false),
                "{hidden} has to be hidden in the installer window"
            );
        }
        assert_eq!(
            settings
                .get("ContainerShowSidebar")
                .and_then(Value::as_boolean),
            Some(false)
        );
        // The path bar names the mounted volume and stays visible, so it is not
        // written at all.
        assert!(settings.get("ShowPathbar").is_none());
    }

    #[test]
    fn the_icon_view_settings_pin_the_shipped_sizes() {
        let settings = record(&decode(&store()), ".", "icvp").settings();
        assert_eq!(
            settings.get("iconSize").and_then(Value::as_real),
            Some(128.0)
        );
        assert_eq!(
            settings.get("textSize").and_then(Value::as_real),
            Some(16.0)
        );
        assert_eq!(
            settings.get("arrangeBy").and_then(Value::as_string),
            Some("none")
        );
        assert_eq!(
            settings
                .get("backgroundType")
                .and_then(Value::as_signed_integer),
            Some(0)
        );
    }

    #[test]
    fn the_icon_view_settings_carry_every_key_finder_writes() {
        // Finder stops honouring `iconSize` when the background and grid keys are
        // missing and opens the window with its own 48 px icons instead, so this
        // key set is a contract with Finder rather than a preference.
        let settings = record(&decode(&store()), ".", "icvp").settings();
        let mut keys: Vec<&str> = settings.keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            [
                "arrangeBy",
                "backgroundColorBlue",
                "backgroundColorGreen",
                "backgroundColorRed",
                "backgroundType",
                "gridOffsetX",
                "gridOffsetY",
                "gridSpacing",
                "iconSize",
                "labelOnBottom",
                "showIconPreview",
                "showItemInfo",
                "textSize",
                "viewOptionsVersion",
            ]
        );
    }

    #[test]
    fn the_view_version_is_a_four_byte_long() {
        let records = decode(&store());
        assert_eq!(record(&records, ".", "vSrn").value, 1_u32.to_be_bytes());
    }

    #[test]
    fn encoding_the_same_layout_twice_produces_the_same_bytes() {
        assert_eq!(
            store(),
            store(),
            "a release artifact has to be reproducible"
        );
    }

    #[test]
    fn a_layout_that_does_not_fit_one_page_is_rejected() {
        let long = "L".repeat(PAGE_SIZE);
        let result = window(&SHIPPED, &format!("{long}.app"), APPLICATIONS);
        assert!(
            matches!(result, Err(Error::TooLarge { page, .. }) if page == PAGE_SIZE),
            "a name that cannot fit the page is reported as a layout that does not fit"
        );
    }

    #[test]
    fn the_store_is_written_under_the_name_finder_looks_for() {
        assert_eq!(FILE_NAME, ".DS_Store");
    }

    #[test]
    fn a_block_is_the_smallest_power_of_two_that_holds_its_contents() {
        assert_eq!(block(1), 32);
        assert_eq!(block(32), 32);
        assert_eq!(block(33), 64);
        assert_eq!(block(1173), 2048);
    }

    #[test]
    fn only_a_blob_value_carries_a_length() {
        assert_eq!(Payload::icon_position((1, 2)).type_tag(), b"blob");
        assert_eq!(Payload::long(7).type_tag(), b"long");
    }
}
