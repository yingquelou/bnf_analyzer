//! 词法分析：按规则把源码切分成带位置信息的 token 流。
//!
//! 裁决规则：在当前位置对所有规则做锚定匹配，**最长匹配胜出**；
//! 长度相同时比较 `priority`（大的优先），再相同则按配置中的声明顺序取先者。
//! 没有任何规则能匹配时立即报错，并给出精确的行列位置。
//!
//! 词法只知道"token 类别"，不知道配对关系——括号的嵌套完全由文法决定。

use std::fmt;

use crate::core::rules::{CompiledRule, Rules, Slot};

/// 源码中的一段字节区间，附起始行列（1 基，列按字符计）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    /// 起始字节偏移（含）。
    pub start: usize,
    /// 结束字节偏移（不含）。
    pub end: usize,
    /// 起始行号（1 基）。
    pub line: usize,
    /// 起始列号（1 基，按字符计）。
    pub column: usize,
}

impl Span {
    /// 由字节偏移与起止行列构造。
    pub const fn new(start: usize, end: usize, line: usize, column: usize) -> Self {
        Self {
            start,
            end,
            line,
            column,
        }
    }

    /// 合并成覆盖一段区间的 span，位置信息取自起点。
    pub const fn merge(open: Span, close: Span) -> Self {
        Self {
            start: open.start,
            end: close.end,
            line: open.line,
            column: open.column,
        }
    }

    /// 区间字节长度。
    pub const fn len(&self) -> usize {
        self.end - self.start
    }

    /// 是否为零宽区间。
    pub const fn is_empty(&self) -> bool {
        self.start == self.end
    }
}

impl fmt::Display for Span {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}, column {}", self.line, self.column)
    }
}

/// 一个词法单元。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    /// token 类别：普通 token 为规则名，字面量终结符为其原文。
    pub kind: String,
    /// 源码中的原始文本。
    pub text: String,
    /// 在源码中的位置。
    pub span: Span,
}

impl Token {
    /// 由类别、原文与位置构造。
    pub fn new(kind: impl Into<String>, text: impl Into<String>, span: Span) -> Self {
        Self {
            kind: kind.into(),
            text: text.into(),
            span,
        }
    }

    /// 判断是否属于某个类别。
    pub fn is(&self, kind: &str) -> bool {
        self.kind == kind
    }
}

impl fmt::Display for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}({:?})", self.kind, self.text)
    }
}

/// 词法分析器。
#[derive(Debug, Clone)]
pub struct Tokenizer {
    rules: Rules,
}

impl Tokenizer {
    /// 用一套已编译的规则构造。
    pub fn new(rules: Rules) -> Self {
        Self { rules }
    }

    /// 当前生效的规则。
    pub fn rules(&self) -> &Rules {
        &self.rules
    }

    /// 扫描整段源码。
    pub fn tokenize(&self, source: &str) -> Result<Vec<Token>, LexError> {
        let mut tokens = Vec::new();
        let mut cursor = Cursor::new();

        while cursor.offset < source.len() {
            let rest = &source[cursor.offset..];
            let ch = rest.chars().next().expect("offset is on a char boundary");

            let Some((rule, len)) = self.longest_match(rest) else {
                return Err(LexError::Unexpected {
                    ch,
                    span: cursor.span(ch.len_utf8()),
                });
            };
            if len == 0 {
                // 装配期（`Rules::from_rules`）已经拒绝了能匹配空串的规则，这里只是兜底：
                // 万一将来有别的构造路径绕过校验，也不至于死循环。
                return Err(LexError::EmptyMatch {
                    rule: rule.source.clone(),
                    span: cursor.span(0),
                });
            }

            let span = cursor.span(len);
            let text = &rest[..len];
            if let Slot::Emit(kind) = &rule.slot {
                tokens.push(Token::new(kind.clone(), text, span));
            }
            cursor.advance(text);
        }

        Ok(tokens)
    }

    /// 在 `rest`（从当前位置开始的剩余源码）上选出最长匹配。
    fn longest_match<'a>(&'a self, rest: &str) -> Option<(&'a CompiledRule, usize)> {
        let mut best: Option<(&CompiledRule, usize)> = None;
        for rule in self.rules.entries() {
            let Some(found) = rule.regex.find(rest) else {
                continue;
            };
            // 正则已用 `\A` 锚定，这里只是兜底断言。
            if found.start() != 0 {
                continue;
            }
            let len = found.end();
            let better = match best {
                None => true,
                Some((current, current_len)) => {
                    len > current_len
                        || (len == current_len
                            && (rule.priority > current.priority
                                || (rule.priority == current.priority
                                    && rule.order < current.order)))
                }
            };
            if better {
                best = Some((rule, len));
            }
        }
        best
    }
}

/// 扫描位置，跟踪字节偏移与行列号。
struct Cursor {
    offset: usize,
    line: usize,
    column: usize,
}

impl Cursor {
    fn new() -> Self {
        Self {
            offset: 0,
            line: 1,
            column: 1,
        }
    }

    /// 以当前位置为起点、给定字节长度构造 span。
    fn span(&self, len: usize) -> Span {
        Span::new(self.offset, self.offset + len, self.line, self.column)
    }

    /// 消费一段文本并推进行列。
    fn advance(&mut self, text: &str) {
        for ch in text.chars() {
            self.offset += ch.len_utf8();
            if ch == '\n' {
                self.line += 1;
                self.column = 1;
            } else {
                self.column += 1;
            }
        }
    }
}

/// 词法分析错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LexError {
    /// 当前位置没有任何规则能匹配。
    Unexpected {
        /// 无法匹配的字符。
        ch: char,
        /// 它在源码中的位置。
        span: Span,
    },
    /// 某条规则匹配到了空串，扫描无法推进。
    EmptyMatch {
        /// 规则的正则源码（装配期本该拦住，留着便于对照配置）。
        rule: String,
        /// 匹配发生的位置。
        span: Span,
    },
}

impl fmt::Display for LexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LexError::Unexpected { ch, span } => write!(f, "{span}: no rule matches character {ch:?}"),
            LexError::EmptyMatch { rule, span } => {
                write!(f, "{span}: rule {rule} matches an empty string, the scanner cannot advance")
            }
        }
    }
}

impl std::error::Error for LexError {}

impl LexError {
    /// 出错位置，便于调用方高亮源码。
    pub fn span(&self) -> Span {
        match self {
            LexError::Unexpected { span, .. } | LexError::EmptyMatch { span, .. } => *span,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::rules::Rule;

    /// 内置词法规则 + 文法里那几个字面量，模拟 `postscript.json` 装配后的最终规则集。
    fn tokenizer() -> Tokenizer {
        let rules = Rules::from_rules(vec![
            Rule::skipping_regex("space", r"[\s]+"),
            Rule::skipping_regex("comment", "%[^\n]*"),
            Rule::regex("string", r"(?s)\((?:[^()\\]|\\.|\([^()\\]*\))*\)"),
            Rule::regex("hexstring", r"(?s)<[0-9A-Fa-f\s]*>"),
            Rule::regex("immediate", r"//[^\s()<>\[\]{}/%]*").with_priority(30),
            Rule::regex("name", r"/[^\s()<>\[\]{}/%]*").with_priority(20),
            Rule::regex("number", r"[-+]?(?:\d+#[0-9A-Za-z]+|\d+\.\d*|\.\d+|\d+)").with_priority(10),
            Rule::regex("boolean", "true|false").with_priority(5),
            Rule::regex("operator", r"[^\s()<>\[\]{}/%]+"),
        ])
        .expect("the rules should be valid");
        let literals: Vec<String> = ["{", "}", "[", "]", "<<", ">>"]
            .iter()
            .map(|text| text.to_string())
            .collect();
        Tokenizer::new(rules.with_literals(&literals).expect("the literals should be valid"))
    }

    fn kinds(source: &str) -> Vec<(String, String)> {
        tokenizer()
            .tokenize(source)
            .expect("should succeed")
            .into_iter()
            .map(|token| (token.kind, token.text))
            .collect()
    }

    #[test]
    fn splits_basic_postscript_tokens() {
        assert_eq!(
            kinds("/a 42 dup"),
            vec![
                ("name".to_string(), "/a".to_string()),
                ("number".to_string(), "42".to_string()),
                ("operator".to_string(), "dup".to_string()),
            ]
        );
    }

    #[test]
    fn skips_whitespace_and_comments() {
        assert_eq!(
            kinds("% comment\n  /x  "),
            vec![("name".to_string(), "/x".to_string())]
        );
    }

    #[test]
    fn literal_delimiters_keep_their_text_as_kind() {
        assert_eq!(
            kinds("<< >> { }"),
            vec![
                ("<<".to_string(), "<<".to_string()),
                (">>".to_string(), ">>".to_string()),
                ("{".to_string(), "{".to_string()),
                ("}".to_string(), "}".to_string()),
            ]
        );
    }

    #[test]
    fn same_length_falls_back_to_priority() {
        assert_eq!(
            kinds("true truely"),
            vec![
                ("boolean".to_string(), "true".to_string()),
                ("operator".to_string(), "truely".to_string()),
            ]
        );
    }

    #[test]
    fn recognizes_strings_and_hex_strings() {
        assert_eq!(
            kinds(r"(hi (nested) \n) <41FF>"),
            vec![
                ("string".to_string(), r"(hi (nested) \n)".to_string()),
                ("hexstring".to_string(), "<41FF>".to_string()),
            ]
        );
    }

    #[test]
    fn distinguishes_names_from_operators() {
        assert_eq!(
            kinds("/name //immediate 16#FF .5 -3"),
            vec![
                ("name".to_string(), "/name".to_string()),
                ("immediate".to_string(), "//immediate".to_string()),
                ("number".to_string(), "16#FF".to_string()),
                ("number".to_string(), ".5".to_string()),
                ("number".to_string(), "-3".to_string()),
            ]
        );
    }

    #[test]
    fn tracks_line_and_column() {
        let tokens = tokenizer().tokenize("a\n  bc").expect("should succeed");
        assert_eq!(tokens[0].span.line, 1);
        assert_eq!(tokens[0].span.column, 1);
        assert_eq!(tokens[1].span.line, 2);
        assert_eq!(tokens[1].span.column, 3);
        assert_eq!(tokens[1].span.start, 4);
    }

    #[test]
    fn reports_unmatched_character() {
        let error = tokenizer().tokenize("(unclosed").expect_err("should fail");
        match error {
            LexError::Unexpected { ch, span } => {
                assert_eq!(ch, '(');
                assert_eq!(span.line, 1);
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn rejects_empty_matching_rules() {
        // 装配期就拒绝：`a*` 能匹配空串，扫描器会停在原地。
        let error = Rules::from_rules(vec![Rule::regex("zero", "a*")])
            .expect_err("a rule that matches the empty string should be rejected");
        assert!(matches!(
            error,
            crate::core::rules::RulesError::EmptyPattern(ref name) if name == "zero"
        ));
        assert!(error.to_string().contains("empty string"), "{error}");
    }
}
