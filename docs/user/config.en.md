# Configuration reference: one JSON document describes a language

For readers who want to analyze their own language without writing Rust. All of the tool's language knowledge
lives in one configuration: lexical rules (`rules`) plus grammar productions (`grammar`). A different
configuration means a different language, and the binary stays the same.

A configuration accepts **structured objects only**: a regex must be wrapped as `{ "regex": "…" }`, and a
right-hand side must be spelled out as `{ "rule": … }` / `{ "token": … }` / `{ "seq": [ … ] }`. String forms (a raw
regex, BNF or EBNF text) have been rejected outright since `version: 2` — there is no hidden "parse another piece of
text" step in a configuration, and what you read is what you get.

> This page only covers **writing a configuration**. To use the crate as a library (actions and values in Rust),
> read `docs/user/api.zh.md`; for a side-by-side comparison of the same grammar in JSON, Rust, and BNF form, read
> `docs/user/grammar.zh.md`.

The companion files:

| file | what it is |
|---|---|
| `examples/postscript.json` | the **builtin configuration**: a PostScript subset, which is what the command line uses by default; also the most complete template |
| `examples/minimal.json` | the smallest usable configuration, showing that "another configuration analyzes another language" |
| `schema/config.schema.json` | the **schema of the configuration format** (Draft 2020-12), independent of any language |

---

## 1. A complete configuration

```json
{
  "$schema": "./schema/config.schema.json",
  "version": 2,
  "rules": [
    { "name": "space",  "pattern": { "plus": { "chars": "\\s" } }, "skip": true },
    { "name": "comment", "pattern": { "regex": "%[^\\n]*" }, "skip": true },
    { "name": "number", "pattern": { "plus": { "chars": "0-9" } }, "priority": 10 },
    { "name": "name",   "pattern": { "regex": "/[^\\s()]*" }, "priority": 20 }
  ],
  "grammar": {
    "start": "program",
    "productions": [
      { "name": "program", "rhs": { "star": { "rule": "value" } } },
      { "name": "value", "rhs": { "alt": [
        { "token": "number" },
        { "token": "name" },
        { "rule": "procedure" }
      ] } },
      { "name": "procedure", "rhs": { "seq": [
        { "literal": "{" },
        { "star": { "rule": "value" } },
        { "literal": "}" }
      ] } }
    ]
  }
}
```

The top level has four keys only:

| key | required | meaning |
|---|---|---|
| `version` | no | configuration format version, currently only `2`; omitted means `2` |
| `rules` | no | array of lexical rules; omitted means an empty table (then only literal terminals are available) |
| `grammar` | no | the grammar section; when omitted, assembly reports `empty grammar: at least one production is required` |
| `$schema` | no | schema path for your editor; ignored when loading, preserved verbatim when writing back |

**Unknown keys are always an error**, matching the schema's `additionalProperties: false`. A typo such as
`productons` does not silently do nothing; it reports ``unknown field `productons`, expected one of: …``.

---

## 2. `rules`: the lexical side

One object per rule, with four keys only:

| key | required | meaning |
|---|---|---|
| `name` | yes | rule name; becomes the token's `kind`, and is referenced from the grammar as `{ "token": "…" }` |
| `pattern` | yes | the match pattern, an object with exactly one key (see §3) |
| `skip` | no | when true, consumes source but produces no token (whitespace, comments); default `false` |
| `priority` | no | priority when competing at the same length, higher wins; default `0` |

Matching is always **anchored** at the current scan position (as if the regex carried `\A`), so a `pattern` never
needs its own `^`.

The decision order is fixed, in three steps:

1. **Longest match** wins first;
2. at equal length, the larger **`priority`** wins;
3. still tied, the **earlier declaration** wins.

`priority` only matters at **equal length**; its purpose is to let a "more specific token" rule beat a general one.
In the builtin configuration `immediate`(30) > `name`(20) > `number`(10) > `boolean`(5) > `operator`(0):
`1.5` matches both `number` and `operator` over its whole length, and `number` wins; `true` goes to `boolean` for
the same reason. When lengths differ, priority plays no part — `/foo` is taken whole by `name`, `//x` whole by
`immediate`.

---

## 3. `pattern`: where the regex comes from

`pattern` is an object with **exactly one key**. One key is one operator; compose by nesting:

| you want to match | spelling | compiled to |
|---|---|---|
| a piece of raw regex | `{ "regex": "\\s+" }` | `\s+` |
| a left brace | `{ "literal": "{" }` | `\{` |
| decimal digits | `{ "chars": "0-9" }` | `[0-9]` |
| anything but whitespace | `{ "not_chars": "\\s" }` | `[^\s]` |
| an identifier | `{ "seq": [{ "chars": "a-zA-Z_" }, { "star": { "chars": "a-zA-Z0-9_" } }] }` | `[a-zA-Z_](?:[a-zA-Z0-9_])*` |
| one of two keywords | `{ "alt": [{ "literal": "if" }, { "literal": "else" }] }` | `if\|else` |
| zero or more | `{ "star": … }` / `{ "plus": … }` / `{ "opt": … }` | `(?:…)*` / `(?:…)+` / `(?:…)?` |

The conventions:

- **the contents of `chars` / `not_chars` are written in regex character-class syntax** (`0-9`, `a-f` are ranges).
  So `]`, `^`, and `\` must be escaped, for example `{ "not_chars": "\\]\\^" }`. Non-ASCII text and emoji can be
  written directly; the class works on characters.
- **`literal` is escaped for you by `regex::escape`**: writing `{ "literal": "{" }` is enough, no `\{`.
- **only `regex` can express advanced syntax**: look-around, named groups, and inline flags (`(?s)`) must go
  through a raw regex.
- writing two keys in one object (`{ "chars": "0-9", "priority": 10 }`) is not "one more option", it is
  `a pattern object must have exactly one key` — `priority` belongs to the rule, not to the pattern.

---

## 4. `grammar`: the grammar

```json
"grammar": {
  "start": "program",
  "productions": [ { "name": "value", "rhs": { "token": "number" } } ]
}
```

| key | required | meaning |
|---|---|---|
| `start` | no | start symbol; when omitted, the name of the **first** production is used |
| `productions` | yes | array of productions, at least one; an empty one reports `empty grammar` at assembly |

A production has two keys only:

| key | meaning |
|---|---|
| `name` | the left-hand side (the nonterminal name). Letters, digits, `_`, and `-` are allowed (`value-1` is fine); must not be empty |
| `rhs` | the right-hand side, one structured expression (see §5) |

**Declaring the same name several times is legal**, and means several alternatives, expanded in declaration order.

---

## 5. `rhs`: three references and five combinators

### 5.1 References

| you want to reference | spelling | when it is checked |
|---|---|---|
| a nonterminal (another production) | `{ "rule": "value" }` | when the grammar is built; a missing name reports "references undefined non-terminal" |
| a lexical class (`rules[].name`) | `{ "token": "number" }` | when the analyzer is assembled; a missing name reports "unknown token class" |
| a literal terminal | `{ "literal": "{" }` | no declaration needed; the lexical rule is derived when the grammar is built |

The three **never infer one another**: `rule` only looks at productions, `token` only at names in `rules`.
"I defined `number` but digits do not match" is almost always a `{ "rule": "number" }` written by mistake.

A few conventions for literal terminals:

- no need to repeat them in `rules`: `{ "literal": "{" }` derives a lexical rule with priority `1000`, so a keyword
  is not swallowed by an identifier rule at the same length;
- the `kind` of a literal token is the text itself (`"{"`, `"<<"`), which is what you see in the output;
- multi-character literals (`"<<"`, `"ifelse"`) match as a whole and never need to be split into single characters.

### 5.2 Combinators

| form | spelling |
|---|---|
| sequence | `{ "seq": [ … ] }` |
| alternation (several alternatives) | `{ "alt": [ … ] }` |
| zero or more | `{ "star": … }` |
| one or more | `{ "plus": … }` |
| zero or one | `{ "opt": … }` |
| empty (ε) | `{ "seq": [] }` |

**The structure is explicit**: no precedence table, no associativity to remember — which of `seq` and `alt` is
inside which is visible from the nesting. `{ "alt": [] }` is rejected (a branch with no alternatives means
nothing).

**A suffix binds one symbol only**: `( word "," )?` needs `opt` around the `seq`:

```json
{ "opt": { "seq": [ { "rule": "word" }, { "literal": "," } ] } }   // the whole group is optional
{ "seq": [ { "rule": "word" }, { "opt": { "literal": "," } } ] }   // only the comma is optional
```

### 5.3 Repetition is flat in the syntax tree

`star` / `plus` / `opt` are only sugar: the syntax tree gains no wrapper node and `children` stays flat; `--bnf`
shows `*` / `+` / `?`, and the "expected" list in an error only contains symbols you wrote yourself. In other
words, how repetition is implemented is the library's business; you just express the intent with the three
spellings above.

---

## 6. Checking a configuration you just changed

Start with the two cheapest steps to confirm the configuration itself is sound:

```text
$ bnf-analyzer -c my.json --bnf        # 1 does the grammar build? start from the BNF view
$ printf '1 /x { dup }' | bnf-analyzer -c my.json -t --compact   # 2 does the tokenizer split it right?
```

- `--bnf` renders the structured grammar as BNF text (`rule` to a bare name, `token` to `@name`, `literal` to
  quotes). It is a **read-only view**, for humans and review; it cannot be read back as a configuration. It is the
  fastest way to check ordering, repetition, and ε.
- `-t` runs the tokenizer only and does no syntax checking: unbalanced brackets still print tokens, which makes it
  the right tool for tuning rules and priorities on their own.

When both steps look right, run the whole pipeline:

```text
$ printf '1 /x { dup }' | bnf-analyzer -c my.json --compact
```

The command line only prints the syntax tree; it carries no actions. To turn the tree into values (sum, build a
dictionary, evaluate), use the crate as a library, see `docs/user/api.zh.md`.

---

## 7. Versions and migration

| `version` | behavior |
|---|---|
| omitted or `2` | loads normally |
| `1` | rejected, with a step-by-step migration hint (see below) |
| anything else | rejected: `unsupported configuration version N (this crate understands version = 2)` |

The error for `version: 1` looks like this (one line in reality, wrapped here for layout):

```text
configuration version 1 is no longer supported: patterns and right-hand sides must be
structured objects. Wrap a regex as { "regex": … }, rewrite BNF text as { "rule": … } /
{ "token": … } / { "seq": [ … ] }, and set version to 2
```

The version number is checked **before** the fields are parsed, so an old file does not first throw a pile of
"a string was rejected"; it tells you how to migrate first.

---

## 8. Editor support: the schema

`schema/config.schema.json` covers the whole configuration, and the builtin configuration points at it through its
own `$schema` (the builtin one lives under `examples/`, hence `../schema/config.schema.json`):

```json
{
  "$schema": "./schema/config.schema.json",
  "version": 2,
  "rules": [ … ],
  "grammar": { … }
}
```

- VS Code / JetBrains use it to complete key names, show descriptions on hover, and underline typos;
- the runtime and the schema agree: `pattern` / `rhs` are both `type: object` + `maxProperties: 1`, so a string is
  rejected outright, and neither side accepts unknown keys;
- for your own configuration, prefer a relative `$schema` path (IDEs understand it too); it is ignored when loading
  and preserved verbatim when writing back.

---

## 9. Error reference

| error | cause | fix |
|---|---|---|
| `rhs` does not accept a string | the right-hand side is still in the old form | split it into `{ "rule": … }` / `{ "token": … }` / `{ "seq": [ … ] }` |
| `pattern` does not accept a string | the pattern is still in the old form | wrap the regex as `{ "regex": "…" }`, use `{ "literal": "…" }` for fixed text |
| a pattern object must have exactly one key | two operators in one pattern | one key per object; compose with `{ "seq": [ … ] }` |
| an expression object must have exactly one key | two symbols in one right-hand side | use `{ "seq": [ … ] }` for several symbols |
| an expression must be an object | a string nested inside the right-hand side | write `{ "star": { "rule": "value" } }` instead |
| unknown expression key / unknown pattern key | a misspelled key (`regexp`, `not_char`) | follow the key tables in §3 / §5 |
| unknown field `…`, expected one of: … | an extra key at the top level, in a rule, or in `grammar` | remove it, or use one of the keys listed |
| references undefined non-terminal `x` | `rule` is used but no production has that name | add a production, or use `{ "token": "x" }` |
| references undefined token class `x` | `token` points at a name that is not in `rules` | add a rule with that name to `rules` |
| empty grammar: at least one production is required | the `grammar` section is empty or missing | write at least one production |
| start symbol `x` has no production | `start` names something that does not exist | use an existing production name, or drop `start` |
| a keyword is split into an ordinary identifier (no error) | the literal is not in the grammar, and the rule priority is too low | write `{ "literal": "if" }` in the grammar, or raise that rule's `priority` |
| failed to compile regex (`]` written in `chars`) | `chars` is a regex character class and needs escaping | write `"\\]"` |
| configuration version 1 is no longer supported | the file still says `version: 1` | follow the hint, switch to the object form, and bump to `2` |

Errors happen either **before the source is read** (configuration problems) or **while tokenizing and parsing**
(source problems); the two never mix. Source errors carry `line N, column M` and a caret, see
`docs/user/cli.zh.md`.

---

## 10. The builtin PostScript subset

`examples/postscript.json` is the instance behind every rule on this page, so you can take it as a template:

```text
program ::= value* ;
value ::= @number | @boolean | @string | @hexstring | @name | @immediate | @operator
        | procedure | array | dictionary ;
procedure ::= "{" value* "}" ;
array ::= "[" value* "]" ;
dictionary ::= "<<" entry* ">>" ;
entry ::= @name value ;
```

(output of `bnf-analyzer --bnf`; the second line is one long line in reality, wrapped here to fit.)

It covers a subset that is **good enough**: atoms (number, boolean, string, hex string, name, immediate name,
operator), three containers (procedure, array, dictionary), comments, and whitespace. Strings allow one level of
nested parentheses (`(a (b) c)` works, `(a (b (c)) d)` does not) — if you need more, replace it with a custom
scanning rule.

To change this language (add keywords, add syntax), copy `examples/postscript.json`, edit the copy, and point at it
with `-c`; do not edit the builtin one, since it doubles as the baseline for the tests and the docs.
