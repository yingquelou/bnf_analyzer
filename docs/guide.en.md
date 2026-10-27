How the docs are assembled: the Markdown under `docs/` is pulled into rustdoc.

The split follows **the reader**, not the form of the content (both sides contain Rust code):

| reader | rustdoc | files |
|---|---|---|
| people who **use** this library (as a dependency, on the command line, with configuration) | [`user`] | `docs/user/` |
| people who **develop** this library (read the source, change the implementation, run the tests) | [`dev`] | `docs/dev/` |

The user manuals cover the `pub` surface only; the reader does not need to know how it is implemented.
Only the developer manuals talk about modules, lowering, and tests.

This is not just about writing one copy less: `cargo test --doc` runs the Rust examples in these manuals as real tests,
so a renamed API, a changed return type, or a different behavior turns the docs red before it trips up a reader.

All four user manuals read configuration files (`Config`, `--bnf`, `--config`), so they ship with the `json` feature;
the developer manuals only cover core, which is available with the default features.

# Two languages

Every page has a Chinese / English pair of buttons in the bottom right corner. They switch between the two copies of
the **same page**: no navigation, no change of address. The chosen language is kept in the browser's localStorage,
so it still applies on the next page.

Both copies share the same structure (same heading levels, same examples, same tables row for row), so after reading
the Chinese version you can switch over and find the same passage in the same place. Chinese is the default.
