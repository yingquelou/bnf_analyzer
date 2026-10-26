//! 文档分家：用户文档只讲公开面，开发者文档才谈内部（改坏了这里先红）。
//!
//! 五条规则：
//!
//! 1. `docs/` 下只有索引 `README.md` 与 `user/`、`dev/` 两个目录；
//! 2. 六份手册都必须挂进 rustdoc（`src/guide.rs` 的 `include_str!`），反之引用的路径必须存在；
//! 3. `docs/user/` 面向**使用者**，只许写 `pub` 面：不出现 `src/`、`tests/`、`$starN`、
//!    `is_synthetic`、`from_exprs` 这类内部标识；
//! 4. rustdoc 也面向使用者：`src/` 里的文档注释不出现仓库路径（`src/`、`tests/`、`Cargo.toml`），
//!    实现动机与文件清单归 `docs/dev/`；
//! 5. 仓库里不许再出现旧路径（平铺的 `docs/api.md`，或上一版的 `docs/dev/api.md`）。
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

/// `docs/` 下所有 `.md`（递归），返回相对 docs/ 的路径，如 `user/cli.md`。
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

/// `src/guide.rs` 里 `include_str!("…")` 指到的文档，返回相对 `docs/` 的路径。
fn wired_manuals() -> Vec<String> {
    let guide = fs::read_to_string(root().join("src").join("guide.rs")).expect("src/guide.rs should be readable");
    let mut out = Vec::new();
    let mut rest = guide.as_str();
    while let Some(start) = rest.find("include_str!(\"") {
        let tail = &rest[start + "include_str!(\"".len()..];
        let end = tail.find('"').expect("the include_str! argument should be closed");
        let raw = &tail[..end];
        // `../docs/user/cli.md` → `user/cli.md`
        let normalized = raw.trim_start_matches("../");
        let path = normalized.strip_prefix("docs/").unwrap_or(normalized);
        out.push(path.to_string());
        rest = &tail[end..];
    }
    out.sort();
    assert!(!out.is_empty(), "src/guide.rs should wire at least one manual");
    out
}

#[test]
fn docs_are_split_into_two_readers() {
    let mut top: Vec<String> = fs::read_dir(docs_dir())
        .expect("the docs directory should be readable")
        .map(|entry| entry.expect("directory entry should be readable").file_name().to_string_lossy().into_owned())
        .collect();
    top.sort();
    assert_eq!(
        top,
        ["README.md", "dev", "user"],
        "docs/ holds only the index plus user/ and dev/: file new manuals under the right reader"
    );

    // 用户 4 份（api / grammar / config / cli），开发者 2 份（architecture / internals）。
    for (dir, expect) in [("user", 4usize), ("dev", 2usize)] {
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
fn every_manual_is_wired_into_rustdoc() {
    let mut manuals = manual_files();
    manuals.retain(|file| file != "README.md"); // 索引只给人看，不进 rustdoc

    assert_eq!(
        manuals,
        wired_manuals(),
        "manuals under docs/ must match the include_str! calls in src/guide.rs://
         either a new manual is not wired into rustdoc, or it points at a missing file"
    );
}

#[test]
fn user_docs_only_talk_about_the_public_surface() {
    for file in manual_files() {
        if !file.starts_with("user/") {
            continue;
        }
        let text = fs::read_to_string(docs_dir().join(&file)).expect("the manual should be readable");
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
        let text = fs::read_to_string(&path).expect("the source should be readable");
        for (index, line) in text.lines().enumerate() {
            let trimmed = line.trim_start();
            if !(trimmed.starts_with("//!") || trimmed.starts_with("///")) {
                continue;
            }
            for marker in REPO_MARKERS {
                assert!(
                    !trimmed.contains(marker),
                    "{}:{} mentions `{marker}` in a doc comment: rustdoc targets users, repository paths belong in docs/dev/architecture.md",
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
        let text = fs::read_to_string(&path).expect("file should be readable");
        for stale in ["docs/api.md", "docs/cli.md", "docs/grammar.md", "docs/dev/api.md", "docs/dev/grammar.md"] {
            assert!(
                !text.contains(stale),
                "{}, still references the old path `{stale}`: manuals are split by reader under docs/user/ and docs/dev/",
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
