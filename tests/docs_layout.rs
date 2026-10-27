//! 文档形状：用户/开发者分家、中英成对、rustdoc 装配（改坏了这里先红）。
//!
//! 规则：
//!
//! 1. `docs/` 顶层只有索引 `README.md`、crate 根与 guide 页的底本、`user/`、`dev/`、`theme/`；
//! 2. 八份底本都是成对的：`X.zh.md` ↔ `X.en.md`，中文那份必须含 CJK，英文那份一个都不许含，
//!    两份的代码块数量一致（漏译一个示例就会红）；
//! 3. 每份底本都必须挂进 rustdoc（`src/lib.rs` / `src/guide.rs` 的 `include_str!` 与 `manual!`），反之亦然；
//! 4. `docs/user/` 面向**使用者**，只许写 `pub` 面：不出现 `src/`、`tests/`、`$starN`、`is_synthetic`、
//!    `from_exprs` 这类内部标识；
//! 5. rustdoc 也面向使用者：`src/` 里的文档注释不出现仓库路径（`src/`、`tests/`、`Cargo.toml`）；
//! 6. 仓库里不许再出现旧路径（平铺手册、`dev/` 下的 api/grammar，或没有语言后缀的 `docs/user/api.md`）。
//!
//! 注意分界是"读者是谁"，不是"有没有 Rust"：用户文档里当然有 Rust（那是库的 API）。

use std::fs;
use std::path::{Path, PathBuf};

/// 用户文档里不该出现的实现细节标识。
const INTERNAL_MARKERS: &[&str] = &[
    "src/",
    "tests/",
    "$star",
    "$plus",
    "$opt",
    "$group",
    "synthetic",
    "from_exprs",
    "core_isolation",
];

fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

fn docs_dir() -> PathBuf {
    root().join("docs")
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|error| panic!("{} should be readable: {error}", path.display()))
}

/// `docs/` 下所有 `.md`（递归），返回相对 docs/ 的路径，如 `user/cli.zh.md`。
fn manual_files() -> Vec<String> {
    fn walk(base: &Path, dir: &Path, out: &mut Vec<String>) {
        for entry in fs::read_dir(dir).expect("the docs directory should be readable") {
            let path = entry.expect("directory entry should be readable").path();
            if path.is_dir() {
                walk(base, &path, out);
            } else if path.extension().is_some_and(|ext| ext == "md") {
                let relative = path.strip_prefix(base).expect("relative to docs/").to_path_buf();
                out.push(relative.to_string_lossy().replace('\\', "/"));
            }
        }
    }
    let base = docs_dir();
    let mut out = Vec::new();
    walk(&base, &base, &mut out);
    out.sort();
    out
}

/// `src/lib.rs` 与 `src/guide.rs` 装配到的文档，返回相对 `docs/` 的路径。
///
/// 两种写法：`manual!("user/api")` 展开成一对语言版本；crate 根与 `guide` 页手写的
/// `include_str!("../docs/x.md")` 直接给出文件名。
fn wired_manuals() -> Vec<String> {
    let mut out = Vec::new();
    for file in ["src/lib.rs", "src/guide.rs"] {
        let text = read(&root().join(file));

        let mut rest = text.as_str();
        while let Some(start) = rest.find("manual!(\"") {
            let tail = &rest[start + "manual!(\"".len()..];
            let end = tail.find('"').expect("the manual! argument should be closed");
            let stem = &tail[..end];
            out.push(format!("{stem}.zh.md"));
            out.push(format!("{stem}.en.md"));
            rest = &tail[end..];
        }

        let mut rest = text.as_str();
        while let Some(start) = rest.find("include_str!(\"") {
            let tail = &rest[start + "include_str!(\"".len()..];
            let end = tail.find('"').expect("the include_str! argument should be closed");
            let raw = &tail[..end];
            // `../docs/user/cli.zh.md` → `user/cli.zh.md`
            let normalized = raw.trim_start_matches("../");
            if let Some(path) = normalized.strip_prefix("docs/") {
                out.push(path.to_string());
            }
            rest = &tail[end..];
        }
    }
    out.retain(|path| path.ends_with(".md"));
    out.sort();
    out.dedup();
    assert!(!out.is_empty(), "src/lib.rs and src/guide.rs should wire at least one manual");
    out
}

/// CJK 统一表意文字与中文标点的粗略范围：够用来区分"这份是中文"与"这份是英文"。
fn has_cjk(text: &str) -> bool {
    text.chars().any(|ch| {
        matches!(
            u32::from(ch),
            0x3000..=0x303F | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF | 0xFF00..=0xFFEF
        )
    })
}

fn code_fences(text: &str) -> usize {
    text.lines().filter(|line| line.trim_start().starts_with("```")).count()
}

#[test]
fn docs_have_the_expected_shape() {
    let mut top: Vec<String> = fs::read_dir(docs_dir())
        .expect("the docs directory should be readable")
        .map(|entry| entry.expect("directory entry should be readable").file_name().to_string_lossy().into_owned())
        .collect();
    top.sort();
    assert_eq!(
        top,
        ["README.md", "dev", "guide.en.md", "guide.zh.md", "intro.en.md", "intro.zh.md", "theme", "user"],
        "docs/ holds the index, the crate root and guide sources, user/, dev/ and theme/: file new manuals \
         under the right reader"
    );

    // 用户 4 对（api / grammar / config / cli），开发者 2 对（architecture / internals）。
    for (dir, expect) in [("user", 8usize), ("dev", 4usize)] {
        let count = fs::read_dir(docs_dir().join(dir))
            .expect("subdirectory should be readable")
            .filter(|entry| {
                entry
                    .as_ref()
                    .expect("directory entry should be readable")
                    .path()
                    .extension()
                    .is_some_and(|ext| ext == "md")
            })
            .count();
        assert_eq!(count, expect, "the number of manuals in docs/{dir}/ changed: update this test");
    }
}

#[test]
fn every_manual_exists_in_both_languages() {
    let mut manuals: Vec<String> = manual_files();
    manuals.retain(|file| file != "README.md");
    for file in &manuals {
        let counterpart = if let Some(stem) = file.strip_suffix(".zh.md") {
            format!("{stem}.en.md")
        } else if let Some(stem) = file.strip_suffix(".en.md") {
            format!("{stem}.zh.md")
        } else {
            panic!("docs/{file} has no language suffix: name it `X.zh.md` / `X.en.md`");
        };
        assert!(
            manuals.contains(&counterpart),
            "docs/{file} has no counterpart docs/{counterpart}: every manual ships in both languages"
        );
    }
}

#[test]
fn each_language_file_stays_in_its_language() {
    for file in manual_files() {
        let text = read(&docs_dir().join(&file));
        if file.ends_with(".zh.md") {
            assert!(has_cjk(&text), "docs/{file} is the Chinese manual but contains no Chinese text");
        } else if file.ends_with(".en.md") {
            let offending: String = text.chars().filter(|ch| has_cjk(&ch.to_string())).take(8).collect();
            assert!(
                !has_cjk(&text),
                "docs/{file} is the English manual but contains Chinese text ({offending}): the two language \
                 versions are kept apart, and a stray character here usually means a half-finished translation"
            );
        }
    }
}

#[test]
fn both_languages_keep_the_same_code_blocks() {
    for file in manual_files() {
        let Some(stem) = file.strip_suffix(".zh.md") else { continue };
        let chinese = read(&docs_dir().join(&file));
        let english = read(&docs_dir().join(format!("{stem}.en.md")));
        assert_eq!(
            code_fences(&chinese),
            code_fences(&english),
            "docs/{stem}.zh.md and docs/{stem}.en.md have a different number of code blocks: a translation that \
             drops or adds an example silently stops testing it in one language"
        );
    }
}

#[test]
fn every_manual_is_wired_into_rustdoc() {
    let mut manuals = manual_files();
    manuals.retain(|file| file != "README.md"); // 索引只给人看，不进 rustdoc

    assert_eq!(
        manuals,
        wired_manuals(),
        "manuals under docs/ must match the include_str! / manual! calls in src/lib.rs and src/guide.rs: \
         either a new manual is not wired into rustdoc, or it points at a missing file"
    );
}

#[test]
fn the_language_switcher_is_wired_next_to_both_bodies() {
    let switcher = read(&docs_dir().join("theme").join("switcher.html"));
    for marker in ["data-set-lang=\"zh\"", "data-set-lang=\"en\"", "[data-lang=\"zh\"]", "[data-lang=\"en\"]"] {
        assert!(
            switcher.contains(marker),
            "docs/theme/switcher.html lost `{marker}`: the page would show both languages at once"
        );
    }

    for file in ["src/lib.rs", "src/guide.rs"] {
        let text = read(&root().join(file));
        assert!(
            text.contains("docs/theme/switcher.html"),
            "{file} does not include docs/theme/switcher.html: its pages would have no language switch"
        );
        assert_eq!(
            text.matches("data-lang=\\\"zh\\\"").count(),
            text.matches("data-lang=\\\"en\\\"").count(),
            "{file} must open exactly one Chinese block and one English block per page"
        );
    }
}

#[test]
fn user_docs_only_talk_about_the_public_surface() {
    for file in manual_files() {
        if !file.starts_with("user/") {
            continue;
        }
        let text = read(&docs_dir().join(&file));
        for marker in INTERNAL_MARKERS {
            assert!(
                !text.contains(marker),
                "docs/{file} mentions the internal marker `{marker}`: user manuals cover the \
                 public surface only, implementation details belong in docs/dev/"
            );
        }
    }
}

/// rustdoc 是给使用者看的：模块文档只讲这层提供什么、怎么用，不指仓库里的文件。
#[test]
fn rustdoc_never_points_at_repository_files() {
    const REPO_MARKERS: &[&str] = &["src/", "tests/", "Cargo.toml"];

    let mut files = Vec::new();
    collect(&root().join("src"), &mut files);
    for path in files {
        if !path.extension().is_some_and(|ext| ext == "rs") {
            continue;
        }
        let text = read(&path);
        for (index, line) in text.lines().enumerate() {
            let trimmed = line.trim_start();
            if !(trimmed.starts_with("//!") || trimmed.starts_with("///")) {
                continue;
            }
            for marker in REPO_MARKERS {
                assert!(
                    !trimmed.contains(marker),
                    "{}:{} mentions `{marker}` in a doc comment: rustdoc targets users, repository paths belong in docs/dev/architecture.zh.md",
                    path.strip_prefix(root()).unwrap_or(&path).display(),
                    index + 1
                );
            }
        }
    }
}

#[test]
fn no_references_to_old_layouts() {
    let mut files: Vec<PathBuf> = vec![root().join("README.md")];
    for dir in ["src", "tests", "docs", "schema", "examples"] {
        collect(&root().join(dir), &mut files);
    }
    for path in files {
        if path.extension().is_some_and(|ext| ext == "json") {
            continue; // schema 与示例配置里不会写文档路径
        }
        if path.ends_with(Path::new("tests").join("docs_layout.rs")) {
            continue; // 本文件自己就在列这些旧路径
        }
        let text = read(&path);
        for stale in [
            "docs/api.md",
            "docs/cli.md",
            "docs/grammar.md",
            "docs/dev/api.md",
            "docs/dev/grammar.md",
            "docs/user/api.md",
            "docs/user/cli.md",
            "docs/user/config.md",
            "docs/user/grammar.md",
            "docs/dev/architecture.md",
            "docs/dev/internals.md",
        ] {
            assert!(
                !text.contains(stale),
                "{}, still references the old path `{stale}`: every manual is a `.zh.md` / `.en.md` pair under docs/user/ and docs/dev/",
                path.strip_prefix(root()).unwrap_or(&path).display()
            );
        }
    }
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries {
        let path = entry.expect("directory entry should be readable").path();
        if path.is_dir() {
            collect(&path, out);
        } else {
            out.push(path);
        }
    }
}
