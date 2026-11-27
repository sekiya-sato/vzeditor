mod parser;
mod render;

pub use parser::{parse_markdown, MdBlock};
pub use render::render_markdown_view;

use parser::MdInline;
