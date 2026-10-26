//! 命令行入口：加载 JSON 配置，把源码分析成 token 流或语法树，并以 JSON 输出。
//!
//! 这条路径不带动作，输出的是带位置信息的默认语法树；需要语义动作时请把本 crate 当库用，
//! 见 [`analyzer`] 的文档。

use std::fs;
use std::io::{self, Read};
use std::path::PathBuf;
use std::process::ExitCode;

use analyzer::core::Analyzer;
use analyzer::core::tokenizer::Span;
use analyzer::core::value::Value;
use analyzer::json;
use analyzer::json::Config;

const USAGE: &str = "\
usage: analyzer [options] [file]

Analyze a source file with the lexical rules and grammar from a configuration file
and print the result as JSON (the built-in postscript.json is used by default).

arguments:
  <file>                file to analyze; omit it or pass - to read standard input

options:
  -c, --config <file>   use this configuration file (default: built-in postscript.json)
  -t, --tokens          print the token stream only, without parsing
      --bnf             print the current grammar as BNF (read-only view)
      --compact         print compact JSON (two-space indentation by default)
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
                println!("analyzer {}", env!("CARGO_PKG_VERSION"));
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

    let source = read_source(options.input.as_deref())?;
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
    input: Option<String>,
}

fn read_source(path: Option<&str>) -> Result<String, String> {
    match path {
        None | Some("-") => {
            let mut buffer = String::new();
            io::stdin()
                .read_to_string(&mut buffer)
                .map_err(|error| format!("failed to read standard input: {error}"))?;
            Ok(buffer)
        }
        Some(path) => fs::read_to_string(path).map_err(|error| format!("failed to read {path}: {error}")),
    }
}

fn render(value: &Value, compact: bool) -> String {
    if compact {
        json::to_string(value)
    } else {
        json::to_string_pretty(value)
    }
}

/// 把错误信息和出错行一起交给用户，避免只看到行列号还要自己数。
fn annotate(message: &str, source: &str, span: Span) -> String {
    let mut text = message.to_string();
    if let Some(line) = source.lines().nth(span.line.saturating_sub(1)) {
        text.push_str(&format!(
            "\n  {line}\n  {}^",
            " ".repeat(span.column.saturating_sub(1))
        ));
    }
    text
}
