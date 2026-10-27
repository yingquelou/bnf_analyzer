//! `postscript.json` 的文档模型：从 [`Value`] 树读出来，也能写回去。
//!
//! 读的一侧是手写字段检查（键名白名单 + 逐字段类型），与
//! `schema/config.schema.json` 的 `additionalProperties: false` 对齐；
//! 写的一侧走 [`Value::to_value`](Config::to_value) + [`super::to_string_pretty`]，
//! 所以"读进来什么样、写出去还是什么样"这件事由同一套结构定义保证。

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::path::Path;

use crate::core::grammar::GrammarSpec;
use crate::core::rules::Rule;
use crate::core::value::Value;

use super::error::JsonError;

/// 内置配置，编译期嵌入。
pub const DEFAULT_CONFIG: &str = include_str!("../../examples/postscript.json");

/// 本库能识别的配置格式版本。
///
/// version 2 起 `pattern` / `rhs` 只接受结构化对象；version 1 的字符串写法已废弃。
pub const SUPPORTED_VERSION: u32 = 2;

/// 顶层允许的键。
pub const CONFIG_KEYS: &[&str] = &["$schema", "version", "rules", "grammar"];

/// 已废弃的版本号，用于给出迁移提示。
const LEGACY_VERSION: u32 = 1;

/// 完整的配置文档。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// 编辑器用的 schema 路径，加载时忽略，但会原样保留以便回写。
    pub schema: Option<String>,
    /// 配置格式版本；当前只接受 2。
    pub version: u32,
    /// 词法规则（token 类别）。
    pub rules: Vec<Rule>,
    /// 文法骨架。
    pub grammar: GrammarSpec,
}

impl Default for Config {
    fn default() -> Self {
        Self::builtin()
    }
}

impl Config {
    /// 内置的 PostScript 配置。
    pub fn builtin() -> Self {
        Self::parse(DEFAULT_CONFIG).expect("the builtin configuration must be valid")
    }

    /// 解析一段 JSON 文本。
    ///
    /// 文本先由 [`crate::json::parse`]（也就是本库自己的 JSON 文法）变成值树，
    /// 再逐字段读成结构体。
    pub fn parse(text: &str) -> Result<Self, ConfigError> {
        let value = crate::json::parse(text)?;
        Self::from_value(&value)
    }

    /// 从文件读配置。
    pub fn load(path: impl AsRef<Path>) -> Result<Self, ConfigError> {
        let path = path.as_ref();
        let text = fs::read_to_string(path).map_err(|source| ConfigError::Io {
            path: path.display().to_string(),
            source,
        })?;
        Self::parse(&text)
    }

    /// 从值树读配置：严格的键名白名单 + 字段类型检查。
    ///
    /// 版本号在字段解析**之前**检查：旧版配置会触发一堆"字符串被拒"的报错，
    /// 而真正该先告诉用户的是"这个版本号已经废弃，按提示迁移"。
    pub fn from_value(value: &Value) -> Result<Self, ConfigError> {
        let map = value.as_map().ok_or_else(|| {
            invalid(format!(
                "the configuration must be an object (keys: {}), found {}",
                CONFIG_KEYS.join(", "),
                value.type_name()
            ))
        })?;
        for key in map.keys() {
            if !CONFIG_KEYS.contains(&key.as_str()) {
                return Err(invalid(format!(
                    "unknown field `{key}`, expected one of: {}",
                    CONFIG_KEYS.join(", ")
                )));
            }
        }

        let version = match map.get("version") {
            None => SUPPORTED_VERSION,
            Some(value) => {
                let number = value
                    .as_int()
                    .ok_or_else(|| invalid(format!("`version` must be an integer, found {}", value.type_name())))?;
                u32::try_from(number)
                    .map_err(|_| invalid(format!("`version` is out of range: {number}")))?
            }
        };
        if version == LEGACY_VERSION {
            return Err(ConfigError::LegacyVersion);
        }
        if version != SUPPORTED_VERSION {
            return Err(ConfigError::UnsupportedVersion(version));
        }

        let schema = match map.get("$schema") {
            None => None,
            Some(value) => Some(
                value
                    .as_text()
                    .ok_or_else(|| {
                        invalid(format!("`$schema` must be a string, found {}", value.type_name()))
                    })?
                    .to_string(),
            ),
        };

        let rules = match map.get("rules") {
            None => Vec::new(),
            Some(value) => {
                let items = value.as_list().ok_or_else(|| {
                    invalid(format!("`rules` must be an array, found {}", value.type_name()))
                })?;
                items
                    .iter()
                    .enumerate()
                    .map(|(index, item)| {
                        Rule::from_value(item).map_err(|error| invalid(format!("rules[{index}]: {error}")))
                    })
                    .collect::<Result<Vec<_>, _>>()?
            }
        };

        let grammar = match map.get("grammar") {
            None => GrammarSpec::default(),
            Some(value) => GrammarSpec::from_value(value).map_err(invalid)?,
        };

        Ok(Self {
            schema,
            version,
            rules,
            grammar,
        })
    }

    /// 转回值树，字段名与配置里的键一一对应。
    pub fn to_value(&self) -> Value {
        let mut map = BTreeMap::new();
        if let Some(schema) = &self.schema {
            map.insert("$schema".to_string(), Value::text(schema));
        }
        map.insert("version".to_string(), Value::Int(i64::from(self.version)));
        map.insert(
            "rules".to_string(),
            Value::List(self.rules.iter().map(Rule::to_value).collect()),
        );
        map.insert("grammar".to_string(), self.grammar.to_value());
        Value::Map(map)
    }

    /// 回写配置（用于 `--dump-config` 之类的场景）。
    pub fn to_json_pretty(&self) -> String {
        super::to_string_pretty(&self.to_value())
    }
}

fn invalid(message: String) -> ConfigError {
    ConfigError::Invalid(message)
}

/// 配置读取 / 解析错误。
#[derive(Debug)]
pub enum ConfigError {
    /// 读文件失败。
    Io {
        /// 文件路径。
        path: String,
        /// 底层错误。
        source: std::io::Error,
    },
    /// JSON 本身不合法。
    Json(JsonError),
    /// 版本号不认识。
    UnsupportedVersion(u32),
    /// 仍是已废弃的 version 1。
    LegacyVersion,
    /// 字段层面的问题，消息里带路径（如 `rules[0].pattern：…`）。
    Invalid(String),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::Io { path, source } => {
                write!(f, "failed to read configuration file {path}: {source}")
            }
            ConfigError::Json(error) => error.fmt(f),
            ConfigError::UnsupportedVersion(version) => write!(
                f,
                "unsupported configuration version {version} (this crate understands version = {SUPPORTED_VERSION})"
            ),
            ConfigError::LegacyVersion => write!(
                f,
                "configuration version {LEGACY_VERSION} is no longer supported: patterns and \
                 right-hand sides must be structured objects. Wrap a regex as {{ \"regex\": … }}, \
                 rewrite BNF text as {{ \"rule\": … }} / {{ \"token\": … }} / {{ \"seq\": [ … ] }}, \
                 and set version to {SUPPORTED_VERSION}"
            ),
            ConfigError::Invalid(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for ConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ConfigError::Io { source, .. } => Some(source),
            ConfigError::Json(error) => Some(error),
            _ => None,
        }
    }
}

impl From<JsonError> for ConfigError {
    fn from(error: JsonError) -> Self {
        ConfigError::Json(error)
    }
}
