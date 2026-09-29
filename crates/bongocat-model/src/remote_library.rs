//! Parsing the remote model library catalog out of its README document.
//!
//! The remote library is a Markdown document, not a schema the product owns, so
//! the parser is deliberately narrow: it recognizes the documented catalog table
//! (名称/作者/预览图/资源地址 and its English aliases), extracts only the four
//! fields the settings window renders, and skips every row it cannot turn into a
//! downloadable entry. A row without an HTTPS `*.zip` link on a GitHub host has
//! no download path the product can follow — those rows are the network-drive
//! shares the reader is meant to open in a browser, not entries we can import.

/// One downloadable entry of the remote model library.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemoteModelEntry {
    /// The model's display name, as the catalog titles it.
    pub name: String,
    /// The model's author, as the catalog credits it.
    pub author: String,
    /// The HTTPS preview image URL, when the catalog ships one.
    pub preview_url: Option<String>,
    /// The HTTPS GitHub `*.zip` download URL. The parser skips rows without one.
    pub download_url: String,
}

/// The largest number of entries one document may yield.
///
/// The catalog is bounded parsing all the way down: a document that keeps
/// producing rows stops being a catalog somewhere past this, and a runaway
/// document must not turn into unbounded memory in the settings worker.
pub const REMOTE_LIBRARY_MAXIMUM_ENTRIES: usize = 128;

/// The longest cell text one column may carry, in characters.
const MAXIMUM_CELL_CHARS: usize = 512;

/// The most cells one table row may carry before it stops being a catalog row.
const MAXIMUM_ROW_CELLS: usize = 8;

/// The columns a catalog table must name, with the synonyms each accepts.
const NAME_HEADER: [&str; 2] = ["名称", "name"];
const AUTHOR_HEADER: [&str; 2] = ["作者", "author"];
const PREVIEW_HEADER: [&str; 3] = ["预览图", "预览", "preview"];
const RESOURCE_HEADER: [&str; 3] = ["资源地址", "资源", "resource"];

/// Derives the stable per-entry identity from its download URL.
///
/// The identity has to survive catalog refreshes and application restarts so a
/// refresh does not orphan a download in flight; the URL is the one field every
/// entry carries that is already unique within a document. FNV-1a is the whole
/// algorithm: a stable, dependency-free hash whose collision behaviour on a
/// handful of URLs is not a product concern.
#[must_use]
pub fn entry_id(download_url: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in download_url.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// Parses every catalog table the document carries, in document order.
///
/// A catalog table is a Markdown table whose header names a name column and a
/// resource column. Rows without a usable download link are skipped, entries the
/// document repeats are deduplicated by URL keeping the first, and the result is
/// capped at [`REMOTE_LIBRARY_MAXIMUM_ENTRIES`].
#[must_use]
pub fn parse_remote_library(readme: &str) -> Vec<RemoteModelEntry> {
    let mut entries: Vec<RemoteModelEntry> = Vec::new();
    let mut lines = readme.lines().peekable();
    while let Some(line) = lines.next() {
        let Some(header) = table_row(line) else {
            continue;
        };
        let Some(columns) = catalog_columns(&header) else {
            continue;
        };
        // The separator row (`| --- | --- |`) is the line that makes a header a
        // header; anything else is an ordinary table mid-document.
        match lines.peek() {
            Some(next) if is_separator_row(next) => {
                lines.next();
            }
            _ => continue,
        }
        for line in lines.by_ref() {
            let Some(cells) = table_row(line) else {
                break;
            };
            if entries.len() >= REMOTE_LIBRARY_MAXIMUM_ENTRIES {
                break;
            }
            let Some(entry) = parse_entry(&cells, &columns) else {
                continue;
            };
            if entries
                .iter()
                .any(|existing| existing.download_url == entry.download_url)
            {
                continue;
            }
            entries.push(entry);
        }
    }
    entries
}

struct CatalogColumns {
    name: usize,
    author: Option<usize>,
    preview: Option<usize>,
    resource: usize,
}

/// Locates the four columns from the header's own names.
fn catalog_columns(header: &[String]) -> Option<CatalogColumns> {
    let lowered: Vec<String> = header.iter().map(|cell| cell.to_lowercase()).collect();
    let name = lowered
        .iter()
        .position(|cell| NAME_HEADER.contains(&cell.as_str()))?;
    let resource = lowered
        .iter()
        .position(|cell| RESOURCE_HEADER.contains(&cell.as_str()))?;
    if name == resource {
        return None;
    }
    let author = lowered
        .iter()
        .position(|cell| AUTHOR_HEADER.contains(&cell.as_str()))
        .filter(|index| *index != name && *index != resource);
    let preview = lowered
        .iter()
        .position(|cell| PREVIEW_HEADER.contains(&cell.as_str()))
        .filter(|index| Some(*index) != author && *index != name && *index != resource);
    Some(CatalogColumns {
        name,
        author,
        preview,
        resource,
    })
}

fn is_separator_row(line: &str) -> bool {
    table_row(line).is_some_and(|cells| {
        !cells.is_empty()
            && cells
                .iter()
                .all(|cell| !cell.is_empty() && cell.bytes().all(|byte| b"-: ".contains(&byte)))
    })
}

/// Splits one Markdown table row into its trimmed cells, or `None` when the line
/// is not a table row. An escaped pipe stays inside its cell as a placeholder
/// until the field extractors restore it.
fn table_row(line: &str) -> Option<Vec<String>> {
    let trimmed = line.trim();
    let rest = trimmed.strip_prefix('|')?;
    let rest = rest.strip_suffix('|').unwrap_or(rest);
    let escaped = rest.replace("\\|", "\u{0}");
    let mut cells: Vec<String> = Vec::new();
    for cell in escaped.split('|') {
        if cells.len() >= MAXIMUM_ROW_CELLS {
            return None;
        }
        cells.push(cell.trim().chars().take(MAXIMUM_CELL_CHARS).collect());
    }
    Some(cells)
}

fn parse_entry(cells: &[String], columns: &CatalogColumns) -> Option<RemoteModelEntry> {
    let cell = |column: usize| cells.get(column).map(String::as_str).unwrap_or_default();
    let name = strip_markdown_link(cell(columns.name));
    if name.is_empty() {
        return None;
    }
    let download_url = github_zip_url(cell(columns.resource))?;
    let author = columns
        .author
        .map(|column| strip_markdown_link(cell(column)))
        .unwrap_or_default();
    let preview_url = columns.preview.and_then(|column| image_url(cell(column)));
    Some(RemoteModelEntry {
        name,
        author,
        preview_url,
        download_url,
    })
}

/// Reduces a cell to its visible text: a markdown link contributes its label, and
/// the escaped-pipe placeholder its pipe.
fn strip_markdown_link(cell: &str) -> String {
    let mut text = String::with_capacity(cell.len());
    let mut rest = cell;
    loop {
        match rest.find("](http") {
            Some(label_end) => {
                let label_start = rest[..label_end].rfind('[');
                match label_start {
                    Some(label_start) => {
                        text.push_str(rest[label_start + 1..label_end].trim_matches('`'));
                        rest = &rest[label_end..];
                        match rest.find(')') {
                            Some(close) => rest = &rest[close + 1..],
                            None => break,
                        }
                    }
                    // A bare "](http" without an opening bracket is not a link;
                    // keep scanning past it rather than dropping the rest.
                    None => {
                        text.push_str(&rest[..label_end + 1]);
                        rest = &rest[label_end + 1..];
                    }
                }
            }
            None => {
                text.push_str(rest);
                break;
            }
        }
    }
    text.replace('\u{0}', "|").trim().to_owned()
}

/// Extracts the first HTTPS GitHub `*.zip` link a resource cell advertises.
fn github_zip_url(cell: &str) -> Option<String> {
    markdown_links(cell).into_iter().find(|url| {
        (url.starts_with("https://github.com/")
            || url.starts_with("https://raw.githubusercontent.com/"))
            && url.to_ascii_lowercase().ends_with(".zip")
    })
}

/// Extracts the first HTTPS image URL a preview cell advertises, from an HTML
/// `img` tag or a markdown image, in that order.
fn image_url(cell: &str) -> Option<String> {
    if let Some(start) = cell.find("<img") {
        let rest = &cell[start..];
        if let Some(src) = rest.find("src=\"") {
            let rest = &rest[src + "src=\"".len()..];
            if let Some(end) = rest.find('"') {
                let url = rest[..end].trim();
                if url.starts_with("https://") && url.chars().count() <= MAXIMUM_CELL_CHARS {
                    return Some(url.to_owned());
                }
            }
        }
    }
    markdown_links(cell)
        .into_iter()
        .find(|url| url.starts_with("https://") && url.chars().count() <= MAXIMUM_CELL_CHARS)
}

/// Collects every `](https://…)` markdown link target in a cell.
fn markdown_links(cell: &str) -> Vec<String> {
    let mut urls = Vec::new();
    let mut rest = cell;
    while let Some(start) = rest.find("](https://") {
        rest = &rest[start + 2..];
        match rest.find(')') {
            Some(end) => {
                urls.push(rest[..end].replace('\u{0}', "|"));
                rest = &rest[end..];
            }
            None => break,
        }
    }
    urls
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOCUMENT: &str = r#"<p align="center">
  <strong>中文</strong> | <a href="./README.en-US.md">English</a>
</p>

## 📚 模型列表

| 名称                  | 作者                                            | 预览图                                                                                                                                                              | 资源地址                                                                                                                                                             |
| --------------------- | ----------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 经典小键盘 · 标准模式 | [@MMmmmoko](https://space.bilibili.com/5808772) | <div align="center"><img src="https://i0.hdslb.com/bfs/openplatform/cover-a.png" height="100" alt="经典小键盘" /></div>                                             | [夸克网盘](https://pan.quark.cn/s/617015c498b2) / [GitHub](https://github.com/ayangweb/Awesome-BongoCat/raw/master/models/Chinese/keyboard-standard.zip)              |
| 温迪 · 标准模式       | [@狐言 0v0](https://space.bilibili.com/1)       | <div align="center"><img src="https://i0.hdslb.com/bfs/openplatform/cover-b.png" height="100" alt="温迪" /></div>                                                    | [夸克网盘](https://pan.quark.cn/s/be26650c9962) / [GitHub](https://github.com/ayangweb/Awesome-BongoCat/raw/master/models/Chinese/venti-standard.zip)                  |
| 仅网盘模型            | [@某人](https://space.bilibili.com/2)           | <div align="center"><img src="https://i0.hdslb.com/bfs/openplatform/cover-c.png" height="100" alt="仅网盘" /></div>                                                  | [夸克网盘](https://pan.quark.cn/s/cc8830fc86d7)                                                                                                                        |
| 无预览模型            | [@某人](https://space.bilibili.com/3)           |                                                                                                                                                                     | [GitHub](https://github.com/ayangweb/Awesome-BongoCat/raw/master/models/Chinese/no-preview.zip)                                                                        |
"#;

    #[test]
    fn parses_every_documented_field_of_a_downloadable_row() {
        let entries = parse_remote_library(DOCUMENT);
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].name, "经典小键盘 · 标准模式");
        assert_eq!(entries[0].author, "@MMmmmoko");
        assert_eq!(
            entries[0].preview_url.as_deref(),
            Some("https://i0.hdslb.com/bfs/openplatform/cover-a.png")
        );
        assert_eq!(
            entries[0].download_url,
            "https://github.com/ayangweb/Awesome-BongoCat/raw/master/models/Chinese/keyboard-standard.zip"
        );
    }

    #[test]
    fn skips_rows_without_a_github_zip_link() {
        let entries = parse_remote_library(DOCUMENT);
        assert!(entries.iter().all(|entry| !entry.name.contains("仅网盘")));
    }

    #[test]
    fn keeps_rows_without_a_preview() {
        let entries = parse_remote_library(DOCUMENT);
        let last = entries.last().expect("the no-preview row survives");
        assert_eq!(last.name, "无预览模型");
        assert_eq!(last.preview_url, None);
    }

    #[test]
    fn entry_identity_follows_the_download_url() {
        assert_ne!(
            entry_id("https://github.com/a/b/raw/main/one.zip"),
            entry_id("https://github.com/a/b/raw/main/two.zip")
        );
        assert_eq!(
            entry_id("https://github.com/a/b/raw/main/one.zip"),
            entry_id("https://github.com/a/b/raw/main/one.zip")
        );
    }

    #[test]
    fn ignores_tables_that_are_not_the_catalog() {
        let document = r#"## Notes

| Flag | Value |
| ---- | ----- |
| zip  | 1     |
"#;
        assert!(parse_remote_library(document).is_empty());
    }

    #[test]
    fn accepts_the_english_header_aliases() {
        let document = r#"## Models

| Name | Author | Preview | Resource |
| ---- | ------ | ------- | -------- |
| Standard | [@a](https://example.com) | ![cover](https://i0.hdslb.com/bfs/openplatform/cover-d.png) | [GitHub](https://github.com/ayangweb/Awesome-BongoCat/raw/master/models/standard.zip) |
"#;
        let entries = parse_remote_library(document);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "Standard");
        assert_eq!(entries[0].author, "@a");
        assert_eq!(
            entries[0].preview_url.as_deref(),
            Some("https://i0.hdslb.com/bfs/openplatform/cover-d.png")
        );
    }

    #[test]
    fn keeps_an_escaped_pipe_inside_its_name() {
        let document = "| 名称 | 作者 | 预览图 | 资源地址 |\n\
                        | ---- | ---- | ------ | -------- |\n\
                        | 亮 \\| 暗 · 标准模式 | [@a](https://example.com) | | [GitHub](https://github.com/a/b/raw/main/escaped.zip) |\n";
        let entries = parse_remote_library(document);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "亮 | 暗 · 标准模式");
    }

    #[test]
    fn deduplicates_repeated_urls_keeping_the_first() {
        let document = "| 名称 | 作者 | 预览图 | 资源地址 |\n\
                        | ---- | ---- | ------ | -------- |\n\
                        | 第一 | [@a](https://example.com) | | [GitHub](https://github.com/a/b/raw/main/same.zip) |\n\
                        | 第二 | [@b](https://example.com) | | [GitHub](https://github.com/a/b/raw/main/same.zip) |\n";
        let entries = parse_remote_library(document);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "第一");
    }

    #[test]
    fn caps_the_entry_count() {
        let mut document = String::from(
            "| 名称 | 作者 | 预览图 | 资源地址 |\n| ---- | ---- | ------ | -------- |\n",
        );
        for index in 0..(REMOTE_LIBRARY_MAXIMUM_ENTRIES + 16) {
            document.push_str(&format!(
                "| 模型 {index} | [@a](https://example.com) | | [GitHub](https://github.com/a/b/raw/main/model-{index}.zip) |\n"
            ));
        }
        let entries = parse_remote_library(&document);
        assert_eq!(entries.len(), REMOTE_LIBRARY_MAXIMUM_ENTRIES);
    }
}
