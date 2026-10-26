//! 代码用英文、注释用中文：**非注释部分**不允许出现 CJK 字符。
//!
//! 这条约束的理由很实际：错误消息、CLI 帮助、断言描述都是**产物**，要给任何读得懂英文的
//! 人看；而本地化（多语言文案）对本库没有价值，所以不引入。注释面向维护者，保持中文。
//!
//! 检查范围是 `src/` 与 `tests/` 下的 `.rs`：跳过 `//`、`//!`、`///`、`/* … */` 之后，
//! 剩下的字符（代码、字符串字面量、字符字面量）一旦含 CJK 就失败。
//!
//! 例外：本文件自身要放中文字符做样例，所以扫描时跳过它。

use std::fs;
use std::path::{Path, PathBuf};

fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

/// CJK 与全角标点的范围：汉字、假名、全角标点都算"中文文本"。
fn is_cjk(ch: char) -> bool {
    matches!(
        ch,
        '\u{3000}'..='\u{303f}'   // CJK 标点
            | '\u{3040}'..='\u{30ff}' // 假名
            | '\u{3400}'..='\u{4dbf}' // 扩展 A
            | '\u{4e00}'..='\u{9fff}' // 基本区
            | '\u{f900}'..='\u{faff}' // 兼容区
            | '\u{ff00}'..='\u{ffef}' // 全角字符
    )
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries {
        let path = entry.expect("directory entry should be readable").path();
        if path.is_dir() {
            collect(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

/// 返回「去注释后仍含 CJK」的行号与内容。
fn offending_lines(source: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut line = 1usize;
    let mut chars = source.chars().peekable();

    while let Some(ch) = chars.next() {
        match ch {
            '\n' => {
                line += 1;
                continue;
            }
            '/' if chars.peek() == Some(&'/') => {
                // 行注释：一路吃到行尾（换行留给下一轮处理）。
                while let Some(&next) = chars.peek() {
                    if next == '\n' {
                        break;
                    }
                    chars.next();
                }
                continue;
            }
            '/' if chars.peek() == Some(&'*') => {
                // 块注释：跳到 `*/`，中间的换行照样计数。
                chars.next();
                let mut previous = '\0';
                for next in chars.by_ref() {
                    match next {
                        '\n' => line += 1,
                        '/' if previous == '*' => break,
                        _ => {}
                    }
                    previous = next;
                }
                continue;
            }
            _ => {}
        }
        if is_cjk(ch) {
            out.push((line, format!("CJK character {ch:?}")));
        }
    }
    out
}

#[test]
fn code_is_english_and_comments_may_be_chinese() {
    let mut files = Vec::new();
    for dir in ["src", "tests"] {
        collect(&root().join(dir), &mut files);
    }
    files.sort();

    let mut problems = Vec::new();
    for path in &files {
        // 这个文件要用中文字符当测试样本，跳过它自己。
        if path.ends_with(Path::new("tests").join("code_language.rs")) {
            continue;
        }
        let source = fs::read_to_string(path).expect("source file should be readable");
        for (line, what) in offending_lines(&source) {
            let relative = path.strip_prefix(root()).unwrap_or(path);
            problems.push(format!("{}:{line}: {what}", relative.display()));
        }
    }

    assert!(
        problems.is_empty(),
        "code must be ASCII/English, comments may be Chinese:\n{}",
        problems.join("\n")
    );
}

#[test]
fn the_scanner_ignores_comments_but_not_strings() {
    let source = "// 注释\n/// 文档注释\n/* 块注释\n继续 */\nlet a = \"中文\";\nlet b = 1;\n";
    let found = offending_lines(source);
    assert!(!found.is_empty(), "the string literal should be reported: {found:?}");
    assert!(
        found.iter().all(|(line, _)| *line == 5),
        "only line 5 (the string literal) should be reported: {found:?}"
    );
}
