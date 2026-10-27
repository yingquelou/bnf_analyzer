//! 命令行入口：加载 JSON 配置，把源码分析成 token 流或语法树，并以 JSON 输出。
//!
//! 这条路径不带动作，输出的是带位置信息的默认语法树；需要语义动作时请把本 crate 当库用，
//! 见 [`bnf_analyzer`] 的文档。

use std::fs;
use std::io::{self, Read};
use std::path::PathBuf;
use std::process::ExitCode;

use bnf_analyzer::core::Analyzer;
use bnf_analyzer::core::tokenizer::Span;
use bnf_analyzer::core::value::Value;
use bnf_analyzer::json;
use bnf_analyzer::json::Config;
use unicode_width::UnicodeWidthChar;

const USAGE: &str = "\
usage: bnf-analyzer [options] [file]

Analyze a source file with the lexical rules and grammar from a configuration file
and print the result as JSON (the built-in postscript.json is used by default).

arguments:
  <file>                file to analyze; omit it or pass - to read standard input

options:
  -c, --config <file>   use this configuration file (default: built-in postscript.json)
  -t, --tokens          print the token stream only, without parsing
      --bnf             print the current grammar as BNF (read-only view)
      --compact         print compact JSON (two-space indentation by default)
      --encoding <enc>  how to read the file: utf8 (default), latin1, or auto
  -h, --help            print this help
  -V, --version         print version
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: &[String]) -> Result<(), String> {
    let mut options = Options::default();
    let mut index = 0;
    while index < args.len() {
        let arg = args[index].as_str();
        index += 1;
        match arg {
            "-h" | "--help" => {
                println!("{USAGE}");
                return Ok(());
            }
            "-V" | "--version" => {
                println!("bnf-analyzer {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            "-c" | "--config" => {
                let value = args
                    .get(index)
                    .ok_or_else(|| format!("{arg} needs a file path\n\n{USAGE}"))?;
                options.config = Some(PathBuf::from(value));
                index += 1;
            }
            "-t" | "--tokens" => options.tokens_only = true,
            "--bnf" => options.bnf = true,
            "--compact" => options.compact = true,
            "--encoding" => {
                let value = args
                    .get(index)
                    .ok_or_else(|| format!("{arg} needs a value\n\n{USAGE}"))?;
                options.encoding = Encoding::parse(value)?;
                index += 1;
            }
            _ if arg.starts_with('-') && arg != "-" => {
                return Err(format!("unknown option: {arg}\n\n{USAGE}"));
            }
            _ => {
                if options.input.is_some() {
                    return Err(format!("only one input file can be given\n\n{USAGE}"));
                }
                options.input = Some(arg.to_string());
            }
        }
    }

    let config = match &options.config {
        Some(path) => Config::load(path).map_err(|error| error.to_string())?,
        None => Config::builtin(),
    };
    let analyzer = Analyzer::from_config(config).map_err(|error| error.to_string())?;

    if options.bnf {
        print!("{}", analyzer.grammar().to_bnf());
        return Ok(());
    }

    let source = read_source(options.input.as_deref(), options.encoding)?;
    let output = if options.tokens_only {
        let tokens = analyzer
            .tokenize(&source)
            .map_err(|error| annotate(&error.to_string(), &source, error.span()))?;
        let value = Value::List(tokens.into_iter().map(Value::Token).collect());
        render(&value, options.compact)
    } else {
        let document = analyzer
            .analyze(&source)
            .map_err(|error| annotate(&error.to_string(), &source, error.span()))?;
        render(&document.root, options.compact)
    };

    println!("{output}");
    Ok(())
}

#[derive(Default)]
struct Options {
    config: Option<PathBuf>,
    tokens_only: bool,
    bnf: bool,
    compact: bool,
    encoding: Encoding,
    input: Option<String>,
}

/// 源码文件的解码方式。
///
/// PostScript 这类语言按字节处理，文件不一定是 UTF-8：`latin1` 把每个字节直接当成
/// 一个码点（ISO-8859-1 与 Unicode 前 256 个码点逐一对齐，不需要查表），
/// `auto` 先按 UTF-8 试一次，失败再退到 `latin1`。
#[derive(Default, Clone, Copy, PartialEq, Eq, Debug)]
enum Encoding {
    #[default]
    Utf8,
    Latin1,
    Auto,
}

impl Encoding {
    fn parse(text: &str) -> Result<Self, String> {
        match text {
            "utf8" | "utf-8" => Ok(Encoding::Utf8),
            "latin1" | "latin-1" | "iso-8859-1" => Ok(Encoding::Latin1),
            "auto" => Ok(Encoding::Auto),
            other => Err(format!(
                "unknown encoding: {other} (use utf8, latin1, or auto)\n\n{USAGE}"
            )),
        }
    }

    /// 把字节解成字符串。失败时带字节偏移——比"不是合法 UTF-8"这句话有用。
    fn decode(self, bytes: Vec<u8>, source: &str) -> Result<String, String> {
        match self {
            Encoding::Utf8 => match String::from_utf8(bytes) {
                Ok(text) => Ok(text),
                Err(error) => {
                    let offset = error.utf8_error().valid_up_to();
                    Err(format!(
                        "failed to read {source}: invalid UTF-8 at byte {offset}\
                         \n  pass --encoding latin1 to read it as Latin-1, or --encoding auto to detect"
                    ))
                }
            },
            Encoding::Latin1 => Ok(bytes.into_iter().map(char::from).collect()),
            Encoding::Auto => match String::from_utf8(bytes) {
                Ok(text) => Ok(text),
                Err(error) => Ok(error.into_bytes().into_iter().map(char::from).collect()),
            },
        }
    }
}

fn read_source(path: Option<&str>, encoding: Encoding) -> Result<String, String> {
    match path {
        None | Some("-") => {
            let mut bytes = Vec::new();
            io::stdin()
                .read_to_end(&mut bytes)
                .map_err(|error| format!("failed to read standard input: {error}"))?;
            encoding.decode(bytes, "standard input")
        }
        Some(path) => {
            let bytes = fs::read(path).map_err(|error| format!("failed to read {path}: {error}"))?;
            encoding.decode(bytes, path)
        }
    }
}

fn render(value: &Value, compact: bool) -> String {
    if compact {
        json::to_string(value)
    } else {
        json::to_string_pretty(value)
    }
}

/// 制表符展开到的列宽，与终端的默认制表位一致。
const TAB_WIDTH: usize = 8;

/// 回显源码行时的缩进。
const INDENT: &str = "  ";

/// 把错误信息和出错行一起交给用户，避免只看到行列号还要自己数。
///
/// `span.column` 按**字符**计，这里换算成**终端列**：制表符跳到下一个 8 的倍数，
/// 宽字符（CJK 等）按两列算，组合字符不算列。列是从行首算起的**绝对**列，
/// 所以要把回显缩进先加上——否则 tab 的落点会整体偏掉一个缩进的宽度。
fn annotate(message: &str, source: &str, span: Span) -> String {
    let mut text = message.to_string();
    let Some(line) = source.lines().nth(span.line.saturating_sub(1)) else {
        return text;
    };
    let mut absolute = INDENT.len();
    for ch in line.chars().take(span.column.saturating_sub(1)) {
        absolute += match ch {
            '\t' => TAB_WIDTH - absolute % TAB_WIDTH,
            other => UnicodeWidthChar::width(other).unwrap_or(0),
        };
    }
    let caret = " ".repeat(absolute - INDENT.len());
    text.push_str(&format!("\n{INDENT}{line}\n{INDENT}{caret}^"));
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latin1_maps_every_byte_to_one_code_point() {
        let text = Encoding::Latin1
            .decode(vec![b'a', 0xE9, 0xE8, 0xFF], "x")
            .expect("latin1 accepts every byte");
        assert_eq!(text, "a\u{e9}\u{e8}\u{ff}");
    }

    #[test]
    fn utf8_reports_the_byte_offset() {
        let error = Encoding::Utf8
            .decode(vec![b'o', b'k', 0xE9], "sample.ps")
            .expect_err("0xE9 alone is not valid UTF-8");
        assert!(error.contains("byte 2"), "{error}");
        assert!(error.contains("--encoding latin1"), "{error}");
    }

    #[test]
    fn auto_falls_back_to_latin1() {
        assert_eq!(Encoding::Auto.decode(vec![b'1'], "x").unwrap(), "1");
        assert_eq!(Encoding::Auto.decode(vec![0xE9], "x").unwrap(), "\u{e9}");
    }

    #[test]
    fn encoding_names_are_parsed() {
        assert!(Encoding::parse("utf-8").is_ok());
        assert!(Encoding::parse("iso-8859-1").is_ok());
        let error = Encoding::parse("klingon").expect_err("unknown encoding");
        assert!(error.contains("unknown encoding"), "{error}");
    }

    #[test]
    fn caret_counts_a_tab_as_the_next_tab_stop() {
        let source = "\tbad )";
        let span = Span::new(6, 7, 1, 6);
        let text = annotate("boom", source, span);
        // 回显缩进 2 列 + tab 跳到第 8 列 + `bad ` 4 列：`^` 落在第 12 列。
        assert!(text.ends_with(&format!("\n  {source}\n  {}^", " ".repeat(10))), "{text}");
    }

    #[test]
    fn caret_counts_wide_characters_as_two_columns() {
        let source = "\u{1f642} x )";
        let span = Span::new(6, 7, 1, 5);
        let text = annotate("boom", source, span);
        // 宽字符两列 + 空格一列 + `x` 一列 + 空格一列 = 5 列。
        assert!(text.ends_with(&format!("\n  {source}\n  {}^", " ".repeat(5))), "{text}");
    }
}
