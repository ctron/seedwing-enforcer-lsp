use ropey::Rope;
use seedwing_policy_engine::lang::parser::SourceSpan;
use tower_lsp::lsp_types::{Position, Range};

pub mod rationale;

pub fn span_to_range(content: &Rope, span: SourceSpan) -> Option<Range> {
    fn convert(content: &Rope, span: SourceSpan) -> Result<Range, ropey::Error> {
        let start_line = content.try_char_to_line(span.start)?;
        let start_pos = span.start - content.try_line_to_char(start_line)?;

        let end_line = content.try_char_to_line(span.end)?;
        let end_pos = span.end - content.try_line_to_char(end_line)?;

        Ok(Range {
            start: Position {
                line: start_line as _,
                character: start_pos as _,
            },
            end: Position {
                line: end_line as _,
                character: end_pos as _,
            },
        })
    }

    convert(content, span).ok()
}