//! JSON 文本的错误：位置 + 说明，便于直接贴给配置文件作者。

use std::fmt;

use crate::core::AnalyzeError;

/// 解析 JSON 文本时的错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsonError {
    /// 1 基行号。
    pub line: usize,
    /// 1 基列号（按字符计）。
    pub column: usize,
    /// 面向人的说明。
    pub message: String,
}

impl JsonError {
    /// 由行列与说明构造。
    pub fn new(line: usize, column: usize, message: impl Into<String>) -> Self {
        Self {
            line,
            column,
            message: message.into(),
        }
    }
}

impl From<AnalyzeError> for JsonError {
    /// 分析器的错误已经带位置，这里只做搬运。
    ///
    /// 分析器的错误文本以 `line X, column Y: ` 开头，先把它摘掉，
    /// 免得 [`JsonError`] 的 `Display` 再加一次。
    fn from(error: AnalyzeError) -> Self {
        let span = error.span();
        let rendered = error.to_string();
        let prefix = format!("{span}: ");
        let message = rendered
            .strip_prefix(&prefix)
            .unwrap_or(&rendered)
            .to_string();
        Self::new(span.line, span.column, message)
    }
}

impl fmt::Display for JsonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "JSON parse failed (line {}, column {}): {}",
            self.line, self.column, self.message
        )
    }
}

impl std::error::Error for JsonError {}
