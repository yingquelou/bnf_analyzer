//! 文法骨架：只有结构化声明，没有文本解析。
//!
//! 1. **Rust 内嵌**（作为依赖库使用）——用 [`Expr`] 直接拼文法：
//!
//!    ```
//!    use bnf_analyzer::core::grammar::{Expr, Grammar};
//!
//!    let grammar = Grammar::builder()
//!        .start("program")
//!        .production("program", Expr::star(Expr::rule("value")))
//!        .production(
//!            "value",
//!            Expr::alt([Expr::token("number"), Expr::rule("procedure")]),
//!        )
//!        .production(
//!            "procedure",
//!            Expr::seq([
//!                Expr::literal("{"),
//!                Expr::star(Expr::rule("value")),
//!                Expr::literal("}"),
//!            ]),
//!        )
//!        .build()
//!        .expect("文法合法");
//!    assert_eq!(grammar.literals(), vec!["{", "}"]);
//!    ```
//!
//! 2. **JSON 声明**（作为可执行 crate 使用）——每条产生式的右部是与 [`Expr`]
//!    一一对应的对象，键名就是变体名：
//!
//!    ```json
//!    {
//!      "name": "procedure",
//!      "rhs": { "seq": [
//!        { "literal": "{" },
//!        { "star": { "rule": "value" } },
//!        { "literal": "}" }
//!      ] }
//!    }
//!    ```
//!
//! 两种方式最后都降级成同一份 CFG（[`Grammar`]），再由同一个 Earley 分析器消费；
//! 差别只在"有没有动作"——JSON 路径没有动作，输出默认语法树。
//!
//! 右部**不接受字符串**：`"rhs": "value*"` 会被拒绝并提示改成对象。这样配置里不存在
//! "再解析一段文法"的隐式步骤，读到什么就是什么。`*`/`+`/`?`/分组这类记法在结构化
//! 形式里分别写成 `star`/`plus`/`opt` 与直接嵌套——重复怎么实现是库的事，语法树里
//! 不会因此多出一层包装节点。
//!
//! 同一名字可以声明多次，等价于多个候选：
//!
//! ```json
//! { "name": "value", "rhs": { "token": "number" } },
//! { "name": "value", "rhs": { "rule": "procedure" } }
//! ```
//!
//! [`Grammar::to_bnf`] 把结构化文法渲染成 BNF 文本——那是**只读视图**，方便评审与
//! 写文档，不是可以被读回来的输入格式。
//!
//! 两种写法的逐项对照见 `guide::user::grammar`；辅助产生式怎么降级见
//! [`guide::dev::internals`](crate::guide::dev::internals)。
//! 引号字面量对应的词法规则不需要另外声明：[`Grammar::literals`] 会把它交给
//! [`Rules::with_literals`](crate::core::rules::Rules::with_literals) 自动补齐。

use std::collections::{BTreeSet, HashMap, HashSet};
use std::fmt;

use crate::core::rules::Rules;
use crate::core::tokenizer::Token;
use crate::core::value::Value;

/// 一份 JSON 声明的文法（配置里的 `grammar` 段）。
///
/// 它是"声明"，不是编译结果：`parse_grammar()` 才会降级成 [`Grammar`]。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GrammarSpec {
    /// 起始符号；省略时取第一条产生式。
    pub start: Option<String>,
    /// 全部产生式；同名多条即多个候选。
    pub productions: Vec<ProductionSpec>,
}

/// 一条 JSON 声明的产生式。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProductionSpec {
    /// 左部。
    pub name: String,
    /// 右部：与 [`Expr`] 一一对应的结构化对象。
    pub rhs: Expr,
}

/// `grammar` 段允许的键。
pub const GRAMMAR_KEYS: &[&str] = &["start", "productions"];

/// `productions[]` 元素允许的键。
pub const PRODUCTION_KEYS: &[&str] = &["name", "rhs"];

/// 文法表达式，既是 Rust 内嵌文法的书写方式，也是 JSON 右部的中间表示。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expr {
    /// 引用非终结符，必须能找到同名产生式。
    Rule(String),
    /// 引用词法类别，必须是 `rules` 里存在的规则名。
    Token(String),
    /// 字面量终结符，按 token 原文匹配。
    Literal(String),
    /// 连接：`a b c`。
    Seq(Vec<Expr>),
    /// 选择：`a | b | c`。
    Alt(Vec<Expr>),
    /// 零或多次。
    Star(Box<Expr>),
    /// 一或多次。
    Plus(Box<Expr>),
    /// 零或一次。
    Opt(Box<Expr>),
}

impl Expr {
    /// 引用非终结符（必须有同名产生式）。
    pub fn rule(name: impl Into<String>) -> Self {
        Expr::Rule(name.into())
    }

    /// 引用词法类别（必须是 `rules` 里的规则名）。
    pub fn token(name: impl Into<String>) -> Self {
        Expr::Token(name.into())
    }

    /// 字面量终结符（按 token 原文匹配）。
    pub fn literal(text: impl Into<String>) -> Self {
        Expr::Literal(text.into())
    }

    /// 连接：`a b c`。
    pub fn seq(items: impl IntoIterator<Item = Expr>) -> Self {
        Expr::Seq(items.into_iter().collect())
    }

    /// 选择：`a | b | c`。
    pub fn alt(items: impl IntoIterator<Item = Expr>) -> Self {
        Expr::Alt(items.into_iter().collect())
    }

    /// 零或多次。
    pub fn star(expr: Expr) -> Self {
        Expr::Star(Box::new(expr))
    }

    /// 一或多次。
    pub fn plus(expr: Expr) -> Self {
        Expr::Plus(Box::new(expr))
    }

    /// 零或一次。
    pub fn opt(expr: Expr) -> Self {
        Expr::Opt(Box::new(expr))
    }
}

fn escape_literal(text: &str) -> String {
    text.replace('\\', "\\\\").replace('"', "\\\"")
}

// ---------------------------------------------------------------------------
// 结构化 JSON 形式
// ---------------------------------------------------------------------------

/// 结构化表达式允许的键：写别的会被当作未知键拒绝，并在提示里列出这几项。
pub const EXPR_KEYS: &[&str] = &[
    "rule", "token", "literal", "seq", "alt", "star", "plus", "opt",
];

fn expr_keys_hint() -> String {
    EXPR_KEYS.join(", ")
}

impl GrammarSpec {
    /// 从 [`Value`] 树读取 `grammar` 段。
    ///
    /// `start` 与 `productions` 都可省略；未知键会被拒绝（与 schema 的
    /// `additionalProperties: false` 一致）。
    pub fn from_value(value: &Value) -> Result<Self, String> {
        let map = value.as_map().ok_or_else(|| {
            format!(
                "grammar must be an object (keys: {}), found {}",
                GRAMMAR_KEYS.join(", "),
                value.type_name()
            )
        })?;
        for key in map.keys() {
            if !GRAMMAR_KEYS.contains(&key.as_str()) {
                return Err(format!(
                    "unknown field `{key}` in grammar, expected one of: {}",
                    GRAMMAR_KEYS.join(", ")
                ));
            }
        }
        let start = match map.get("start") {
            None => None,
            Some(value) => Some(
                value
                    .as_text()
                    .ok_or_else(|| {
                        format!("`grammar.start` must be a string, found {}", value.type_name())
                    })?
                    .to_string(),
            ),
        };
        let productions = match map.get("productions") {
            None => Vec::new(),
            Some(value) => {
                let items = value.as_list().ok_or_else(|| {
                    format!(
                        "`grammar.productions` must be an array, found {}",
                        value.type_name()
                    )
                })?;
                items
                    .iter()
                    .enumerate()
                    .map(|(index, item)| {
                        ProductionSpec::from_value(item)
                            .map_err(|error| format!("grammar.productions[{index}]: {error}"))
                    })
                    .collect::<Result<Vec<_>, _>>()?
            }
        };
        Ok(Self {
            start,
            productions,
        })
    }

    /// 转回 [`Value`] 树；`start` 为空时不写这个键。
    pub fn to_value(&self) -> Value {
        let mut map = std::collections::BTreeMap::new();
        if let Some(start) = &self.start {
            map.insert("start".to_string(), Value::text(start));
        }
        map.insert(
            "productions".to_string(),
            Value::List(self.productions.iter().map(ProductionSpec::to_value).collect()),
        );
        Value::Map(map)
    }

    /// 把声明编译成文法。
    pub fn parse_grammar(&self) -> Result<Grammar, GrammarError> {
        Grammar::from_spec(self)
    }
}

impl ProductionSpec {
    /// 从 [`Value`] 树读取一条产生式：`{ "name": …, "rhs": … }`。
    pub fn from_value(value: &Value) -> Result<Self, String> {
        let map = value.as_map().ok_or_else(|| {
            format!(
                "a production must be an object (keys: {}), found {}",
                PRODUCTION_KEYS.join(", "),
                value.type_name()
            )
        })?;
        for key in map.keys() {
            if !PRODUCTION_KEYS.contains(&key.as_str()) {
                return Err(format!(
                    "unknown field `{key}` in production, expected one of: {}",
                    PRODUCTION_KEYS.join(", ")
                ));
            }
        }
        let name = map
            .get("name")
            .ok_or_else(|| "production is missing `name`".to_string())?
            .as_text()
            .ok_or_else(|| "`name` must be a string".to_string())?
            .to_string();
        let rhs = map
            .get("rhs")
            .ok_or_else(|| format!("production `{name}` is missing `rhs`"))?;
        Ok(Self {
            name,
            rhs: Expr::from_value(rhs)?,
        })
    }

    /// 转成结构化 JSON 值。
    pub fn to_value(&self) -> Value {
        let mut map = std::collections::BTreeMap::new();
        map.insert("name".to_string(), Value::text(&self.name));
        map.insert("rhs".to_string(), self.rhs.to_value());
        Value::Map(map)
    }
}

impl Expr {
    /// 从 [`Value`] 树读取结构化表达式，例如 `{ "star": { "rule": "value" } }`。
    ///
    /// 一个对象只能有一个键；键名与 [`Expr`] 的变体一一对应。字符串一律拒绝：
    /// 右部没有文本写法。
    pub fn from_value(value: &Value) -> Result<Self, String> {
        if let Some(text) = value.as_text() {
            return Err(format!(
                "`rhs` does not accept a string ({text:?}): use a structured object such as \
                 {{\"rule\": \"value\"}} or {{\"star\": {{\"rule\": \"value\"}}}}; keys: {}",
                expr_keys_hint()
            ));
        }
        let map = value.as_map().ok_or_else(|| {
            format!(
                "an expression must be an object (one of {}), found {}; for example {{ \"rule\": \"value\" }}",
                expr_keys_hint(),
                value.type_name()
            )
        })?;
        if map.len() != 1 {
            return Err(format!(
                "an expression object must have exactly one key (one of {}), found {}; wrap a sequence in {{ \"seq\": [ … ] }}",
                expr_keys_hint(),
                map.len()
            ));
        }
        let (key, value) = map.iter().next().expect("length is one");
        match key.as_str() {
            "rule" => Ok(Expr::Rule(expr_string(key, value)?)),
            "token" => Ok(Expr::Token(expr_string(key, value)?)),
            "literal" => Ok(Expr::Literal(expr_string(key, value)?)),
            "seq" => Ok(Expr::Seq(expr_list(key, value)?)),
            "alt" => Ok(Expr::Alt(expr_list(key, value)?)),
            "star" => Ok(Expr::Star(Box::new(Expr::from_value(value)?))),
            "plus" => Ok(Expr::Plus(Box::new(Expr::from_value(value)?))),
            "opt" => Ok(Expr::Opt(Box::new(Expr::from_value(value)?))),
            other => Err(format!(
                "unknown expression key `{other}`, expected one of: {}",
                expr_keys_hint()
            )),
        }
    }

    /// 转回 [`Value`] 树：每个变体写成同名单键对象。
    pub fn to_value(&self) -> Value {
        let (key, value) = match self {
            Expr::Rule(name) => ("rule", Value::text(name)),
            Expr::Token(name) => ("token", Value::text(name)),
            Expr::Literal(text) => ("literal", Value::text(text)),
            Expr::Seq(items) => ("seq", Value::List(items.iter().map(Expr::to_value).collect())),
            Expr::Alt(items) => ("alt", Value::List(items.iter().map(Expr::to_value).collect())),
            Expr::Star(inner) => ("star", inner.to_value()),
            Expr::Plus(inner) => ("plus", inner.to_value()),
            Expr::Opt(inner) => ("opt", inner.to_value()),
        };
        let mut map = std::collections::BTreeMap::new();
        map.insert(key.to_string(), value);
        Value::Map(map)
    }
}

fn expr_string(key: &str, value: &Value) -> Result<String, String> {
    value
        .as_text()
        .map(str::to_string)
        .ok_or_else(|| format!("`{key}` must be a string, found {}", value.type_name()))
}

fn expr_list(key: &str, value: &Value) -> Result<Vec<Expr>, String> {
    let items = value
        .as_list()
        .ok_or_else(|| format!("`{key}` must be an array, found {}", value.type_name()))?;
    // 空的 `seq` 就是 ε；空的选择则没有意义。
    if key == "alt" && items.is_empty() {
        return Err("`alt` needs at least one alternative; use { \"seq\": [] } for the empty sequence".to_string());
    }
    items.iter().map(Expr::from_value).collect()
}

/// 渲染成 BNF 文本（只读视图）：`rule` 写裸名，`token` 写 `@名字`，`literal` 写引号。
impl fmt::Display for Expr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Expr::Rule(name) => f.write_str(name),
            // 显式的词法类别必须带着 `@` 才能与 `rule` 区分开。
            Expr::Token(name) => write!(f, "@{name}"),
            Expr::Literal(text) => write!(f, "\"{}\"", escape_literal(text)),
            Expr::Seq(items) => {
                for (index, item) in items.iter().enumerate() {
                    if index > 0 {
                        f.write_str(" ")?;
                    }
                    write_atom(item, f)?;
                }
                Ok(())
            }
            Expr::Alt(items) => {
                for (index, item) in items.iter().enumerate() {
                    if index > 0 {
                        f.write_str(" | ")?;
                    }
                    match item {
                        Expr::Seq(items) => {
                            for (position, atom) in items.iter().enumerate() {
                                if position > 0 {
                                    f.write_str(" ")?;
                                }
                                write_atom(atom, f)?;
                            }
                        }
                        other => write_atom(other, f)?,
                    }
                }
                Ok(())
            }
            Expr::Star(inner) => {
                write_atom(inner, f)?;
                f.write_str("*")
            }
            Expr::Plus(inner) => {
                write_atom(inner, f)?;
                f.write_str("+")
            }
            Expr::Opt(inner) => {
                write_atom(inner, f)?;
                f.write_str("?")
            }
        }
    }
}

/// 渲染一个可能带后缀的原子；`Alt` / `Seq` 需要括号。
fn write_atom(expr: &Expr, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match expr {
        Expr::Alt(_) | Expr::Seq(_) => write!(f, "( {expr} )"),
        other => write!(f, "{other}"),
    }
}

/// 终结符。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Terminal {
    /// 字面量：按 token 原文匹配。
    Literal(String),
    /// 词法类别：按 token 的 `kind` 匹配，即 `rules[].name`。
    Class(String),
}

impl Terminal {
    /// 用于错误提示与 BNF 视图的名字。
    pub fn label(&self) -> &str {
        match self {
            Terminal::Literal(text) | Terminal::Class(text) => text,
        }
    }

    /// 该终结符是否匹配指定 token。
    pub fn matches(&self, token: &Token) -> bool {
        match self {
            Terminal::Literal(text) => token.text == *text,
            Terminal::Class(name) => token.kind == *name,
        }
    }
}

/// 产生式右部的一个符号。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Item {
    /// 非终结符。
    NonTerminal(String),
    /// 终结符（字面量或词法类别）。
    Terminal(Terminal),
}

impl Item {
    /// 用于错误提示与 BNF 视图的名字。
    pub fn label(&self) -> &str {
        match self {
            Item::NonTerminal(name) => name,
            Item::Terminal(terminal) => terminal.label(),
        }
    }
}

/// 一条已降级的产生式。同名多条即为多个候选。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Production {
    /// 左部。
    pub name: String,
    /// 右部符号，按顺序；为空即 ε 产生式。
    pub items: Vec<Item>,
    /// 由 `*`、`+`、`?`、分组生成的辅助产生式。
    pub synthetic: bool,
}

impl Production {
    /// 是否为空产生式（ε）。
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

impl fmt::Display for Production {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let body = self
            .items
            .iter()
            .map(Item::label)
            .collect::<Vec<_>>()
            .join(" ");
        write!(f, "{} ::= {}", self.name, body)
    }
}

/// 编译好的文法。构造后不可变，可被多次分析复用。
#[derive(Debug, Clone)]
pub struct Grammar {
    start: String,
    productions: Vec<Production>,
    index: HashMap<String, Vec<usize>>,
    classes: Vec<ClassRef>,
    /// 声明原文（名字 + 渲染后的右部），用于输出 BNF 视图。
    sources: Vec<(String, String)>,
}

/// 记录词法类别的引用位置，供 [`Grammar::validate`] 报错。
#[derive(Debug, Clone)]
struct ClassRef {
    name: String,
    production: String,
}

impl Grammar {
    /// 流畅构建一份文法（Rust 内嵌用法）。
    pub fn builder() -> GrammarBuilder {
        GrammarBuilder::new()
    }

    /// 从一份声明（[`GrammarSpec`]）构建。
    pub fn from_spec(spec: &GrammarSpec) -> Result<Self, GrammarError> {
        let productions = spec
            .productions
            .iter()
            .map(|production| (production.name.clone(), production.rhs.clone()))
            .collect();
        Self::from_exprs(spec.start.clone(), productions)
    }

    /// 从「名字 + 表达式」列表构建，Rust 与 JSON 两条路共用。
    pub fn from_exprs(
        start: Option<String>,
        productions: Vec<(String, Expr)>,
    ) -> Result<Self, GrammarError> {
        if productions.is_empty() {
            return Err(GrammarError::Empty);
        }
        let mut names: HashSet<String> = HashSet::new();
        for (name, _) in &productions {
            if name.trim().is_empty() {
                return Err(GrammarError::EmptyName);
            }
            // 同名多次声明是合法的：等价于多个候选，顺序保持声明顺序。
            names.insert(name.clone());
        }
        let start = start.unwrap_or_else(|| productions[0].0.clone());
        if !names.contains(&start) {
            return Err(GrammarError::UnknownStart { start });
        }

        let mut lowering = Lowering {
            names: &names,
            productions: Vec::new(),
            classes: Vec::new(),
            counter: 0,
            production: String::new(),
        };
        let mut compiled = Vec::new();
        let mut sources: Vec<(String, String)> = Vec::new();
        for (name, expr) in &productions {
            lowering.production = name.clone();
            for alternative in top_level_alternatives(expr) {
                let items = lowering.top_level(alternative)?;
                compiled.push(Production {
                    name: name.clone(),
                    items,
                    synthetic: false,
                });
            }
            // 同名多次声明合并成一条 `a ::= x | y`，让 BNF 视图保持一名一行。
            match sources.iter_mut().find(|(source, _)| source == name) {
                Some((_, rhs)) => {
                    rhs.push_str(" | ");
                    rhs.push_str(&expr.to_string());
                }
                None => sources.push((name.clone(), expr.to_string())),
            }
        }
        compiled.extend(lowering.productions);

        // 不可派生的非终结符（只引用自己，或引用了这样的符号）永远推不出任何终结符，
        // 文法因此永远匹配不了输入。装配期就报出来，比让 `parse` 事后给一句
        // "expected …" 强。ε 产生式算可派生（它推出空串）。
        let productive = productive_names(&compiled);
        if let Some((name, _)) = productions
            .iter()
            .find(|(name, _)| !productive.contains(name.as_str()))
        {
            return Err(GrammarError::NonProductive { name: name.clone() });
        }

        let mut index: HashMap<String, Vec<usize>> = HashMap::new();
        for (position, production) in compiled.iter().enumerate() {
            index
                .entry(production.name.clone())
                .or_default()
                .push(position);
        }
        Ok(Self {
            start,
            productions: compiled,
            index,
            classes: lowering.classes,
            sources,
        })
    }

    /// 起始符号。
    pub fn start(&self) -> &str {
        &self.start
    }

    /// 全部产生式（含辅助产生式）。
    pub fn productions(&self) -> &[Production] {
        &self.productions
    }

    /// 按下标取产生式。
    pub fn production(&self, index: usize) -> &Production {
        &self.productions[index]
    }

    /// 起始符号对应的产生式下标。
    pub fn start_productions(&self) -> &[usize] {
        self.productions_of(&self.start)
    }

    /// 指定非终结符的全部候选产生式下标。
    pub fn productions_of(&self, name: &str) -> &[usize] {
        self.index.get(name).map(Vec::as_slice).unwrap_or(&[])
    }

    /// 该名字是否只对应内部辅助产生式（`$star1` 之类的 EBNF 降级产物）。
    ///
    /// 用户声明的产生式永远是 `synthetic = false`，所以这里为真意味着这个名字
    /// 不该暴露给用户——语法树、`to_bnf()`、错误提示都据此过滤。
    pub fn is_synthetic(&self, name: &str) -> bool {
        match self.index.get(name) {
            Some(indices) => indices
                .iter()
                .all(|index| self.productions[*index].synthetic),
            None => false,
        }
    }

    /// 文法里出现的全部字面量终结符，用于派生词法规则。
    pub fn literals(&self) -> Vec<String> {
        let mut set = BTreeSet::new();
        for production in &self.productions {
            for item in &production.items {
                if let Item::Terminal(Terminal::Literal(text)) = item {
                    set.insert(text.clone());
                }
            }
        }
        set.into_iter().collect()
    }

    /// 校验文法引用的词法类别都在 `rules` 里存在。
    pub fn validate(&self, rules: &Rules) -> Result<(), GrammarError> {
        for reference in &self.classes {
            let defined = rules
                .rules
                .iter()
                .any(|rule| rule.name == reference.name && !rule.skip);
            if !defined {
                return Err(GrammarError::UnknownTokenClass {
                    name: reference.name.clone(),
                    production: reference.production.clone(),
                });
            }
        }
        Ok(())
    }

    /// 渲染成 BNF 文本（**只读视图**：`rule` → 裸名，`token` → `@名字`，`literal` → 引号）。
    ///
    /// 用于人读、评审与写进设计文档；它不是输入格式，配置里一律用结构化对象。
    /// 空右部渲染成 `a ::= ;`，即 ε。
    pub fn to_bnf(&self) -> String {
        let mut text = String::new();
        for (name, rhs) in &self.sources {
            match rhs.trim_end() {
                "" => text.push_str(&format!("{name} ::= ;\n")),
                rhs => text.push_str(&format!("{name} ::= {rhs} ;\n")),
            }
        }
        text
    }
}

impl fmt::Display for Grammar {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_bnf())
    }
}

/// 算出可以推出「某个终结符串」的非终结符集合（ε 产生式算在内）。
///
/// 不动点迭代：一条产生式的右部全是终结符或已可派生的非终结符时，左部也可派生。
fn productive_names(productions: &[Production]) -> HashSet<String> {
    let mut productive: HashSet<String> = HashSet::new();
    loop {
        let mut changed = false;
        for production in productions {
            if productive.contains(&production.name) {
                continue;
            }
            let ready = production.items.iter().all(|item| match item {
                Item::Terminal(_) => true,
                Item::NonTerminal(name) => productive.contains(name),
            });
            if ready {
                productive.insert(production.name.clone());
                changed = true;
            }
        }
        if !changed {
            return productive;
        }
    }
}

/// 流畅构建器。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GrammarBuilder {
    start: Option<String>,
    productions: Vec<(String, Expr)>,
}

impl GrammarBuilder {
    /// 空构建器：没有起始符号，也没有产生式。
    pub fn new() -> Self {
        Self::default()
    }

    /// 指定起始符号；不指定则取第一条产生式。
    pub fn start(mut self, name: impl Into<String>) -> Self {
        self.start = Some(name.into());
        self
    }

    /// 追加一条产生式；同名多次出现视为多个候选（等价于 `Expr::alt`，也等价于
    /// JSON 里写两条同名 `productions`）。
    pub fn production(mut self, name: impl Into<String>, rhs: Expr) -> Self {
        self.productions.push((name.into(), rhs));
        self
    }

    /// 装配成文法：检查符号引用与可派生性，并降级 `*` / `+` / `?` 与分组。
    pub fn build(self) -> Result<Grammar, GrammarError> {
        Grammar::from_exprs(self.start, self.productions)
    }
}

/// 文法构建错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GrammarError {
    /// 一条产生式都没有。
    Empty,
    /// 某条产生式的 `name` 为空。
    EmptyName,
    /// 起始符号没有对应的产生式。
    UnknownStart {
        /// 起始符号名。
        start: String,
    },
    /// 右部引用了不存在的非终结符。
    UnknownSymbol {
        /// 引用发生的产生式。
        production: String,
        /// 找不到的非终结符名。
        name: String,
    },
    /// 右部引用了不存在的 token 类别。
    UnknownTokenClass {
        /// 找不到的类别名。
        name: String,
        /// 引用发生的产生式。
        production: String,
    },
    /// 某条产生式永远推不出终结符串（只递归自己，或依赖了这样的符号）。
    NonProductive {
        /// 推不出任何终结符串的非终结符名。
        name: String,
    },
}

impl fmt::Display for GrammarError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GrammarError::Empty => f.write_str("empty grammar: at least one production is required"),
            GrammarError::EmptyName => f.write_str("production name must not be empty"),
            GrammarError::UnknownStart { start } => {
                write!(f, "start symbol {start} has no production")
            }
            GrammarError::UnknownSymbol { production, name } => write!(
                f,
                "production {production} references undefined non-terminal {name} (no production with that name)"
            ),
            GrammarError::UnknownTokenClass { name, production } => write!(
                f,
                "production {production} references undefined token class {name}\
                 (no rule with that name, or the rule is a skip rule)"
            ),
            GrammarError::NonProductive { name } => write!(
                f,
                "non-terminal {name} can never produce a terminal string \
                 (it only refers to itself, directly or through other non-terminals)"
            ),
        }
    }
}

impl std::error::Error for GrammarError {}

// ---------------------------------------------------------------------------
// 表达式归一化与降级
// ---------------------------------------------------------------------------

/// 顶层的选择候选：`a ::= x | y` 展开成两条同名产生式。
///
/// **只展开最外层的 `Alt`**。`Seq` 里的 `Alt` 不在这里做分配律展开——那会让
/// 「一个 `seq` 里嵌 n 个二选一」变成 2ⁿ 条产生式；子表达式交给 [`Lowering::item`]
/// 降级成辅助产生式，规模保持线性。
fn top_level_alternatives(expr: &Expr) -> Vec<&Expr> {
    match expr {
        Expr::Alt(items) => items.iter().flat_map(top_level_alternatives).collect(),
        other => vec![other],
    }
}

struct Lowering<'a> {
    names: &'a HashSet<String>,
    productions: Vec<Production>,
    classes: Vec<ClassRef>,
    counter: usize,
    production: String,
}

impl Lowering<'_> {
    fn fresh(&mut self, kind: &str) -> String {
        self.counter += 1;
        // `$` 前缀表示内部名字：BNF 视图里的 `@` 已经是"词法类别"，不能复用。
        format!("${kind}{}", self.counter)
    }

    fn sequence(&mut self, atoms: &[Expr]) -> Result<Vec<Item>, GrammarError> {
        atoms.iter().map(|atom| self.item(atom)).collect()
    }

    /// 顶层表达式：`Seq` 直接摊成本条产生式的右部，其余当成单个符号。
    ///
    /// 只有顶层这样处理——嵌套的 `Seq` 会降级成一个辅助产生式，避免"把子表达式
    /// 平铺进父产生式"时与外侧符号做交叉积。
    fn top_level(&mut self, expr: &Expr) -> Result<Vec<Item>, GrammarError> {
        match expr {
            Expr::Seq(items) => self.sequence(items),
            other => Ok(vec![self.item(other)?]),
        }
    }

    fn item(&mut self, expr: &Expr) -> Result<Item, GrammarError> {
        match expr {
            Expr::Rule(name) => {
                if self.names.contains(name) {
                    Ok(Item::NonTerminal(name.clone()))
                } else {
                    Err(GrammarError::UnknownSymbol {
                        production: self.production.clone(),
                        name: name.clone(),
                    })
                }
            }
            Expr::Token(name) => {
                self.classes.push(ClassRef {
                    name: name.clone(),
                    production: self.production.clone(),
                });
                Ok(Item::Terminal(Terminal::Class(name.clone())))
            }
            Expr::Literal(text) => Ok(Item::Terminal(Terminal::Literal(text.clone()))),
            // 分组：一个辅助产生式装下整段右部，规模与子表达式大小成正比。
            Expr::Seq(items) => {
                let helper = self.fresh("group");
                let items = self.sequence(items)?;
                self.productions.push(Production {
                    name: helper.clone(),
                    items,
                    synthetic: true,
                });
                Ok(Item::NonTerminal(helper))
            }
            Expr::Alt(items) => {
                let helper = self.fresh("group");
                for alternative in items {
                    let items = self.top_level(alternative)?;
                    self.productions.push(Production {
                        name: helper.clone(),
                        items,
                        synthetic: true,
                    });
                }
                Ok(Item::NonTerminal(helper))
            }
            Expr::Opt(inner) => {
                let inner = self.item(inner)?;
                let name = self.fresh("opt");
                self.productions.push(Production {
                    name: name.clone(),
                    items: vec![inner],
                    synthetic: true,
                });
                self.productions.push(Production {
                    name: name.clone(),
                    items: Vec::new(),
                    synthetic: true,
                });
                Ok(Item::NonTerminal(name))
            }
            // 重复用**左递归**降级：`$star ::= $star a | ε`、`$plus ::= a | $plus a`。
            //
            // 右递归（`$star ::= a $star | ε`）在 Earley 里是最坏形状——每归约一层都要把
            // "剩余全部子值"复制给每个父项，时间 O(n³)、内存 O(n²)。左递归每步只把已累积
            // 的列表往后接一个元素，规模降到 O(n²) 时间 / O(n)；摊平后的顺序不变。
            Expr::Star(inner) => {
                let inner = self.item(inner)?;
                let name = self.fresh("star");
                self.productions.push(Production {
                    name: name.clone(),
                    items: vec![Item::NonTerminal(name.clone()), inner],
                    synthetic: true,
                });
                self.productions.push(Production {
                    name: name.clone(),
                    items: Vec::new(),
                    synthetic: true,
                });
                Ok(Item::NonTerminal(name))
            }
            Expr::Plus(inner) => {
                let inner = self.item(inner)?;
                let name = self.fresh("plus");
                self.productions.push(Production {
                    name: name.clone(),
                    items: vec![inner.clone()],
                    synthetic: true,
                });
                self.productions.push(Production {
                    name: name.clone(),
                    items: vec![Item::NonTerminal(name.clone()), inner],
                    synthetic: true,
                });
                Ok(Item::NonTerminal(name))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(productions: &[(&str, Expr)]) -> GrammarSpec {
        GrammarSpec {
            start: None,
            productions: productions
                .iter()
                .map(|(name, rhs)| ProductionSpec {
                    name: name.to_string(),
                    rhs: rhs.clone(),
                })
                .collect(),
        }
    }

    /// 手搭一个对象值。core 的测试只走 [`Value`]，不经过 JSON 文本，
    /// 所以 `--no-default-features` 下也照跑（core 不依赖 `json` 层）。
    fn obj(entries: &[(&str, Value)]) -> Value {
        Value::Map(
            entries
                .iter()
                .map(|(key, value)| ((*key).to_string(), value.clone()))
                .collect::<std::collections::BTreeMap<_, _>>(),
        )
    }

    #[test]
    fn lowers_ebnf_sugar_into_hidden_productions() {
        let grammar = spec(&[
            (
                "list",
                Expr::seq([
                    Expr::literal("["),
                    Expr::star(Expr::rule("item")),
                    Expr::literal("]"),
                ]),
            ),
            ("item", Expr::token("number")),
        ])
        .parse_grammar()
        .expect("the grammar should be valid");

        let visible: Vec<_> = grammar
            .productions()
            .iter()
            .filter(|production| !production.synthetic)
            .map(|production| production.name.as_str())
            .collect();
        assert_eq!(visible, vec!["list", "item"]);

        let star = grammar
            .productions()
            .iter()
            .find(|production| production.name == "$star1")
            .expect("should generate a helper production");
        assert_eq!(star.items.len(), 2);
    }

    #[test]
    fn lowers_repetition_left_recursively() {
        // `$star ::= $star item | ε`：左递归在 Earley 里是 O(n²)，右递归是 O(n³)。
        let grammar = spec(&[
            ("list", Expr::star(Expr::rule("item"))),
            ("item", Expr::token("number")),
        ])
        .parse_grammar()
        .expect("the grammar should be valid");

        let indices = grammar.productions_of("$star1");
        assert_eq!(indices.len(), 2, "one recursive production plus epsilon");
        let recursive = grammar.production(indices[0]);
        assert_eq!(
            recursive.items.first(),
            Some(&Item::NonTerminal("$star1".to_string())),
            "the recursive production must start with the non-terminal itself: {recursive}"
        );
        assert!(grammar.production(indices[1]).is_empty());
        assert!(grammar.is_synthetic("$star1"));
    }

    #[test]
    fn nested_alternatives_stay_linear() {
        // 一个 `seq` 里嵌 12 个二选一：分配律展开会得到 4096 条产生式，
        // 逐项降级则每项只多一个辅助产生式。
        let mut seq = Vec::new();
        for index in 0..12 {
            seq.push(Expr::alt([
                Expr::literal(format!("a{index}")),
                Expr::literal(format!("b{index}")),
            ]));
        }
        let grammar = Grammar::builder()
            .start("program")
            .production("program", Expr::seq(seq))
            .build()
            .expect("the grammar should be valid");
        assert!(
            grammar.productions().len() <= 30,
            "expected linear growth, got {} productions",
            grammar.productions().len()
        );
    }

    #[test]
    fn rejects_non_productive_nonterminal() {
        // `dead` 只能递归自己，永远推不出终结符串——装配期就该报错，而不是等解析。
        let error = Grammar::builder()
            .start("program")
            .production(
                "program",
                Expr::alt([Expr::literal("x"), Expr::rule("dead")]),
            )
            .production(
                "dead",
                Expr::seq([Expr::rule("dead"), Expr::literal("y")]),
            )
            .build()
            .expect_err("a non-productive non-terminal should be rejected");
        assert!(
            matches!(error, GrammarError::NonProductive { ref name } if name == "dead"),
            "{error}"
        );
        assert!(error.to_string().contains("never produce"), "{error}");
    }

    #[test]
    fn optional_and_empty_productions_stay_productive() {
        // ε 会产生空串，仍然算可派生——别把合法的可选结构误判成死循环。
        Grammar::builder()
            .start("program")
            .production("program", Expr::opt(Expr::token("number")))
            .build()
            .expect("`opt` must stay productive");
    }

    #[test]
    fn renders_back_to_bnf() {
        let grammar = spec(&[
            ("program", Expr::star(Expr::rule("value"))),
            (
                "value",
                Expr::alt([Expr::token("number"), Expr::rule("procedure")]),
            ),
            (
                "procedure",
                Expr::seq([
                    Expr::literal("{"),
                    Expr::star(Expr::rule("value")),
                    Expr::literal("}"),
                ]),
            ),
        ])
        .parse_grammar()
        .expect("the grammar should be valid");
        let bnf = grammar.to_bnf();
        assert!(bnf.contains("program ::= value* ;"), "{bnf}");
        assert!(bnf.contains("value ::= @number | procedure ;"), "{bnf}");
        assert!(bnf.contains("procedure ::= \"{\" value* \"}\" ;"), "{bnf}");
        assert!(!bnf.contains("$star"), "helper productions must not appear in the BNF view");
    }

    #[test]
    fn rejects_string_rhs() {
        let value = obj(&[(
            "productions",
            Value::List(vec![obj(&[
                ("name", Value::text("program")),
                ("rhs", Value::text("value*")),
            ])]),
        )]);
        let error = GrammarSpec::from_value(&value).expect_err("a string right-hand side should be rejected");
        assert!(error.contains("does not accept a string"), "{error}");
        assert!(error.contains("structured object"), "{error}");
    }

    #[test]
    fn rejects_unknown_keys() {
        let value = obj(&[(
            "productions",
            Value::List(vec![obj(&[
                ("name", Value::text("program")),
                ("rhs", obj(&[("regexp", Value::text("value"))])),
            ])]),
        )]);
        let error = GrammarSpec::from_value(&value).expect_err("unknown keys should be rejected");
        assert!(error.contains("unknown expression key"), "{error}");
    }

    #[test]
    fn rejects_unknown_grammar_keys() {
        let value = crate::core::value::Value::map();
        assert_eq!(
            GrammarSpec::from_value(&value).expect("an empty object is an empty grammar"),
            GrammarSpec::default()
        );
        let mut map = std::collections::BTreeMap::new();
        map.insert("extra".to_string(), crate::core::value::Value::Int(1));
        let error = GrammarSpec::from_value(&crate::core::value::Value::Map(map))
            .expect_err("unknown fields should be rejected");
        assert!(error.contains("unknown field"), "{error}");
    }

    #[test]
    fn merges_repeated_declarations_of_one_name() {
        let grammar = spec(&[
            ("value", Expr::token("number")),
            ("value", Expr::token("string")),
        ])
        .parse_grammar()
        .expect("declaring the same name twice should be allowed");
        assert_eq!(grammar.productions_of("value").len(), 2);
        assert_eq!(grammar.to_bnf(), "value ::= @number | @string ;\n");
    }

    /// 单键对象，如 `{ "star": … }`。
    fn unary(key: &str, inner: Value) -> Value {
        obj(&[(key, inner)])
    }

    #[test]
    fn builder_and_spec_produce_same_shapes() {
        let built = Grammar::builder()
            .start("program")
            .production("program", Expr::star(Expr::rule("value")))
            .production(
                "value",
                Expr::alt([Expr::token("number"), Expr::rule("procedure")]),
            )
            .production(
                "procedure",
                Expr::seq([
                    Expr::literal("{"),
                    Expr::star(Expr::rule("value")),
                    Expr::literal("}"),
                ]),
            )
            .build()
            .expect("the grammar should be valid");

        // 同一份文法的声明形态。`Value` 树正是 `postscript.json` 解析后得到的东西，
        // 这里手搭，于是 core 的测试不必经过 `json` 层。
        let parsed = GrammarSpec::from_value(&obj(&[(
            "productions",
            Value::List(vec![
                obj(&[
                    ("name", Value::text("program")),
                    ("rhs", unary("star", unary("rule", Value::text("value")))),
                ]),
                obj(&[
                    ("name", Value::text("value")),
                    (
                        "rhs",
                        unary(
                            "alt",
                            Value::List(vec![
                                unary("token", Value::text("number")),
                                unary("rule", Value::text("procedure")),
                            ]),
                        ),
                    ),
                ]),
                obj(&[
                    ("name", Value::text("procedure")),
                    (
                        "rhs",
                        unary(
                            "seq",
                            Value::List(vec![
                                unary("literal", Value::text("{")),
                                unary("star", unary("rule", Value::text("value"))),
                                unary("literal", Value::text("}")),
                            ]),
                        ),
                    ),
                ]),
            ]),
        )]))
        .expect("the declaration should read as a GrammarSpec")
        .parse_grammar()
        .expect("the grammar should be valid");

        assert_eq!(built.to_bnf(), parsed.to_bnf());
        assert_eq!(built.literals(), parsed.literals());
        assert_eq!(built.productions().len(), parsed.productions().len());
    }

    #[test]
    fn rejects_unknown_nonterminal_when_explicit() {
        let error = Grammar::builder()
            .production("program", Expr::rule("missing"))
            .build()
            .expect_err("should fail");
        assert!(matches!(error, GrammarError::UnknownSymbol { ref name, .. } if name == "missing"));
    }

    #[test]
    fn rejects_unknown_token_class() {
        let grammar = spec(&[("program", Expr::star(Expr::token("mystery")))])
            .parse_grammar()
            .expect("the grammar itself should be valid");
        let rules = Rules::from_rules(vec![crate::core::rules::Rule::regex("number", "\\d+")])
            .expect("the rules should be valid");
        let error = grammar.validate(&rules).expect_err("should fail");
        assert!(matches!(
            error,
            GrammarError::UnknownTokenClass { ref name, .. } if name == "mystery"
        ));
    }

    #[test]
    fn collects_literal_terminals() {
        let grammar = spec(&[(
            "program",
            Expr::seq([
                Expr::literal("("),
                Expr::token("number"),
                Expr::literal(")"),
            ]),
        )])
        .parse_grammar()
        .expect("the grammar should be valid");
        assert_eq!(grammar.literals(), vec!["(", ")"]);
    }

    #[test]
    fn serializes_back_to_objects() {
        let spec = spec(&[("program", Expr::star(Expr::token("number")))]);
        let grammar = spec.parse_grammar().expect("the grammar should be valid");
        let value = spec.to_value();
        assert_eq!(
            GrammarSpec::from_value(&value).expect("should read back"),
            spec,
            "the declaration should round-trip through value → spec → value"
        );
        let productions = value.as_map().and_then(|map| map.get("productions"));
        let rhs = productions
            .and_then(|value| value.as_list())
            .and_then(|items| items.first())
            .and_then(|item| item.as_map())
            .and_then(|map| map.get("rhs"))
            .expect("the right-hand side should be written");
        assert!(
            rhs.as_map().is_some_and(|map| map.contains_key("star")),
            "the right-hand side is a single-key object: {rhs}"
        );
        assert_eq!(grammar.to_bnf(), "program ::= @number* ;\n");
    }
}
