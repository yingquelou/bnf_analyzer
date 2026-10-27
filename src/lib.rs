// 中英双份正文：`docs/intro.zh.md` 与 `docs/intro.en.md` 拼在一起，由 `data-lang` 分成两块，
// 页面末尾的切换器（`docs/theme/switcher.html`）只显示其中一块。手册页的装法见 `src/guide.rs` 的 `manual!`。
#![doc = concat!(
    "<div class=\"lang\" data-lang=\"zh\">\n\n",
    include_str!("../docs/intro.zh.md"),
    "\n</div>\n\n<div class=\"lang\" data-lang=\"en\">\n\n",
    include_str!("../docs/intro.en.md"),
    "\n</div>\n\n",
    include_str!("../docs/theme/switcher.html"),
)]

pub mod core;
pub mod guide;

/// JSON 层：**用本库自己的文法**读写 JSON，不依赖任何外部 JSON 库。
#[cfg(feature = "json")]
pub mod json;
