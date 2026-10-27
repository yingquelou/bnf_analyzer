//! core 的自包含与命名边界（三条硬规则，改坏了这里先红）。
//!
//! 1. `src/core/` 里的模块只准用 `crate::core::…` 引用自己人，不得回头依赖
//!    `crate::json`、`crate::guide` 这类兄弟模块——core 要能整块切出去复用。
//! 2. 除正则引擎外不引入第三方库：错误类型、JSON 读写都由本 crate 自己实现。
//! 3. 名字只留在自己那一层：crate 根只声明模块、不 re-export；core 的门面固定是
//!    四个类型 + 八个模块。要短名由使用方自己 `use` 或取别名，库不替用户决定。
//!
//! 前两条也是 `json` 层能当作"用 core 写出来的示例"的前提。

use std::fs;
use std::path::{Path, PathBuf};

/// core 里允许出现的 use 根：标准库、正则引擎、以及 crate 内部路径。
const ALLOWED_USE_ROOTS: &[&str] = &[
    "std", "core", "alloc", "regex", "crate", "self", "super",
];

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("src/core should be readable") {
        let path = entry.expect("directory entry should be readable").path();
        if path.is_dir() {
            collect(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

/// core 目录下的全部源文件，按路径排序，返回（文件名, 文本）。
fn core_sources() -> Vec<(String, String)> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join("core");
    let mut paths = Vec::new();
    collect(&root, &mut paths);
    paths.sort();
    paths
        .iter()
        .map(|path| {
            let name = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            let text = fs::read_to_string(path).expect("source should be readable");
            (name, text)
        })
        .collect()
}

/// 去掉行尾注释，只留代码部分。文档注释（`//!` / `///`）整体变成空行——
/// 那里提到 `crate::` 只是文字，不是真引用。
fn code_of(line: &str) -> &str {
    match line.find("//") {
        Some(index) => &line[..index],
        None => line,
    }
}

#[test]
fn core_only_refers_to_itself() {
    for (name, text) in core_sources() {
        for line in text.lines() {
            let code = code_of(line);
            for (index, _) in code.match_indices("crate::") {
                let tail = &code[index + "crate::".len()..];
                assert!(
                    tail.starts_with("core::"),
                    "{name} refers to `crate::{head}`: core must not depend on other parts of the crate\n  {line}",
                    head = &tail[..tail.len().min(32)]
                );
            }
        }
    }
}

#[test]
fn core_has_no_third_party_dependency_but_regex() {
    for (name, text) in core_sources() {
        // 无论用不用 `use` 引进来，这两个都不该出现在 core 里。
        for banned in ["thiserror", "serde"] {
            assert!(
                !text.contains(banned),
                "{name} mentions `{banned}`: core allows regex as its only external crate"
            );
        }
        for line in text.lines() {
            let line = line.trim_start();
            // 文档里的示例站在使用者视角写 `use bnf_analyzer::…`，那是 doctest 而不是真依赖。
            if line.starts_with("//") {
                continue;
            }
            let Some(rest) = line.strip_prefix("use ").or_else(|| line.strip_prefix("pub use "))
            else {
                continue;
            };
            // `use { a, b }` 这种没有根路径，跳过。
            let root = rest
                .split(|ch: char| !(ch.is_alphanumeric() || ch == '_'))
                .next()
                .unwrap_or_default();
            if root.is_empty() {
                continue;
            }
            assert!(
                ALLOWED_USE_ROOTS.contains(&root),
                "{name} imports `{root}`: core may only depend on regex"
            );
        }
    }
}

#[test]
fn manifest_depends_on_regex_only() {
    let manifest = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml"))
        .expect("Cargo.toml should be readable");
    let names = dependency_names(&manifest);
    assert_eq!(names, ["regex"], "only regex should be listed: {names:?}");
}

/// 取依赖名：既认手写的 `[dependencies]` 段，也认 `cargo package` 规范化后的 `[dependencies.regex]` 表。
fn dependency_names(manifest: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut in_table = false;
    for raw in manifest.lines() {
        let line = raw.trim();
        if line.starts_with('[') {
            in_table = line == "[dependencies]";
            if let Some(name) = line
                .strip_prefix("[dependencies.")
                .and_then(|rest| rest.strip_suffix(']'))
            {
                names.push(name.trim().to_string());
            }
            continue;
        }
        if !in_table || line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((key, _)) = line.split_once('=') {
            names.push(key.trim().to_string());
        }
    }
    names.sort();
    names
}

/// 收集一段源码里 `pub use` 引入的名字（跳过注释）。
fn exported_names(text: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("pub use ") {
        let tail = &rest[start + "pub use ".len()..];
        let Some(end) = tail.find(';') else { break };
        let path = &tail[..end];
        let last = path.rsplit("::").next().unwrap_or(path).trim();
        match last.strip_prefix('{') {
            Some(inner) => {
                for raw in inner.split(',') {
                    let name = raw.trim().trim_end_matches('}').trim();
                    if !name.is_empty() {
                        names.push(name.to_string());
                    }
                }
            }
            None => names.push(last.to_string()),
        }
        rest = &tail[end..];
    }
    names.sort();
    names
}

/// 收集 `pub mod name;` 的模块名（跳过注释）。
fn public_modules(text: &str) -> Vec<String> {
    let mut names: Vec<String> = text
        .lines()
        .map(|line| code_of(line).trim())
        .filter_map(|code| code.strip_prefix("pub mod "))
        .filter_map(|rest| rest.strip_suffix(';'))
        .map(|name| name.trim().to_string())
        .collect();
    names.sort();
    names
}

#[test]
fn crate_root_does_not_reexport_core_names() {
    let lib = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join("lib.rs"))
        .expect("src/lib.rs should be readable");
    for line in lib.lines() {
        let code = code_of(line).trim();
        assert!(
            !code.starts_with("pub use"),
            "the crate root must not re-export names, use full paths instead://n  {line}"
        );
    }
    assert!(
        exported_names(&lib).is_empty(),
        "the crate root should only declare modules, found: {:?}",
        exported_names(&lib)
    );
    assert_eq!(
        public_modules(&lib),
        ["core", "guide", "json"],
        "the crate root should declare three modules (json only with the json feature)"
    );
}

#[test]
fn core_facade_stays_a_short_list() {
    let mod_rs =
        fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join("core").join("mod.rs"))
            .expect("src/core/mod.rs should be readable");

    // core 门面只有四个类型：其余名字留在各自模块里。
    assert_eq!(
        exported_names(&mod_rs),
        ["AnalyzeError", "Analyzer", "Document", "SetupError"],
        "the core facade changed: keep new names in submodules or update this list"
    );

    // 八个引擎模块，一个环节一个。
    assert_eq!(
        public_modules(&mod_rs),
        [
            "grammar", "parser", "pattern", "rules", "semantics", "tokenizer", "value", "visit",
        ],
        "the list of core submodules changed"
    );
}
