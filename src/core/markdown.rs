//! Lightweight CommonMark parsing for full-output script results.
//!
//! pulldown-cmark (MIT/Apache-2.0, zero dependencies) converts markdown to a
//! small block model the UI renders with plain GPUI text elements. No
//! WebView, no JS runtime, no extra font infrastructure — the cost scales
//! with the size of the output and the parse runs once per result, not per
//! frame.

use std::ops::Range;

/// Inline emphasis found inside a block's text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InlineKind {
    /// `**bold**` / `__bold__`
    Bold,
    /// `*italic*` / `_italic_`
    Italic,
    /// `` `code` ``
    Code,
    /// `~~strikethrough~~`
    Strikethrough,
    /// `[link](url)`
    Link,
}

/// A styled byte range of [`MdText::text`]. Ranges are sorted,
/// non-overlapping and always on character boundaries (required by the
/// GPUI text APIs that consume them).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InlineMark {
    pub kind: InlineKind,
    pub range: Range<usize>,
}

/// Text plus the inline style ranges within it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MdText {
    pub text: String,
    pub marks: Vec<InlineMark>,
}

/// One rendered block of markdown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MdBlock {
    /// `# heading` (`level` 1..=6).
    Heading { level: u8, text: MdText },
    /// A plain paragraph.
    Paragraph(MdText),
    /// A fenced or indented code block.
    CodeBlock { lang: Option<String>, text: String },
    /// A list item; `number` is `Some` for ordered lists.
    ListItem { number: Option<u64>, text: MdText },
    /// `> quoted text`.
    Blockquote(MdText),
    /// `---` horizontal rule.
    Rule,
    /// Text outside any recognised block; shown as plain text.
    Plain(String),
}

/// Kind of block currently accumulating text in [`State`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BlockKind {
    None,
    Heading(u8),
    Para,
    Item(Option<u64>),
}

/// Accumulator state for a single parse pass.
struct State {
    blocks: Vec<MdBlock>,
    buf: MdText,
    kind: BlockKind,
    in_code: bool,
    code_lang: Option<String>,
    code_text: String,
    /// Innermost lists: `(Some(first_number) if ordered, next_number)`.
    lists: Vec<(Option<u64>, u64)>,
    /// Nesting depth of open blockquotes.
    quotes: u32,
    /// Open inline emphases: `(text cursor when opened, kind)`.
    inline: Vec<(usize, InlineKind)>,
}

impl State {
    fn new() -> Self {
        Self {
            blocks: Vec::new(),
            buf: MdText {
                text: String::new(),
                marks: Vec::new(),
            },
            kind: BlockKind::None,
            in_code: false,
            code_lang: None,
            code_text: String::new(),
            lists: Vec::new(),
            quotes: 0,
            inline: Vec::new(),
        }
    }

    fn cursor(&self) -> usize {
        self.buf.text.len()
    }

    fn push_text(&mut self, s: &str) {
        self.buf.text.push_str(s);
    }

    fn open_inline(&mut self, kind: InlineKind) {
        self.inline.push((self.cursor(), kind));
    }

    fn close_inline(&mut self) {
        if let Some((start, kind)) = self.inline.pop() {
            let end = self.cursor();
            if end > start {
                self.buf.marks.push(InlineMark {
                    kind,
                    range: start..end,
                });
            }
        }
    }

    /// Emit the accumulated buffer as a block, if it has any content.
    fn flush_block(&mut self) {
        if self.buf.text.trim().is_empty() && self.buf.marks.is_empty() {
            self.buf = MdText::default();
            self.kind = BlockKind::None;
            self.inline.clear();
            return;
        }
        let text = std::mem::take(&mut self.buf);
        // A paragraph nested in a blockquote is rendered as a quote block.
        let block = match self.kind {
            BlockKind::Heading(level) => MdBlock::Heading { level, text },
            BlockKind::Para if self.quotes > 0 => MdBlock::Blockquote(text),
            BlockKind::Para => MdBlock::Paragraph(text),
            BlockKind::Item(number) => MdBlock::ListItem { number, text },
            BlockKind::None => MdBlock::Plain(text.text),
        };
        self.blocks.push(block);
        self.kind = BlockKind::None;
        self.inline.clear();
    }
}

/// Parse markdown text into blocks. Never panics on malformed input;
/// anything unrecognised degrades to plain text.
pub fn parse(input: &str) -> Vec<MdBlock> {
    use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};

    let options = Options::ENABLE_STRIKETHROUGH;
    let mut st = State::new();

    for event in Parser::new_ext(input, options) {
        match event {
            Event::Start(tag) => match tag {
                Tag::Heading { level, .. } => {
                    st.flush_block();
                    st.kind = BlockKind::Heading(level as u8);
                }
                Tag::Paragraph => {
                    st.flush_block();
                    st.kind = BlockKind::Para;
                }
                Tag::BlockQuote(_) => {
                    st.flush_block();
                    st.quotes += 1;
                }
                Tag::List(start) => {
                    st.flush_block();
                    st.lists.push((start, start.unwrap_or(1)));
                }
                Tag::Item => {
                    st.flush_block();
                    let number = st
                        .lists
                        .last()
                        .and_then(|(ordered, next)| ordered.map(|_| *next));
                    if let Some(ctx) = st.lists.last_mut() {
                        ctx.1 += 1;
                    }
                    st.kind = BlockKind::Item(number);
                }
                Tag::CodeBlock(kind) => {
                    st.flush_block();
                    st.in_code = true;
                    st.code_lang = match kind {
                        CodeBlockKind::Fenced(info) => {
                            let info = info.trim();
                            (!info.is_empty()).then(|| info.to_string())
                        }
                        CodeBlockKind::Indented => None,
                    };
                    st.code_text.clear();
                }
                Tag::Strong => st.open_inline(InlineKind::Bold),
                Tag::Emphasis => st.open_inline(InlineKind::Italic),
                Tag::Strikethrough => st.open_inline(InlineKind::Strikethrough),
                Tag::Link { .. } => st.open_inline(InlineKind::Link),
                // Tables, images, etc.: their text content flows through
                // as plain text, no special rendering.
                _ => {}
            },
            Event::End(tag_end) => match tag_end {
                TagEnd::Heading(_) | TagEnd::Paragraph | TagEnd::Item => st.flush_block(),
                TagEnd::BlockQuote(_) => {
                    st.flush_block();
                    st.quotes = st.quotes.saturating_sub(1);
                }
                TagEnd::List(_) => {
                    st.flush_block();
                    st.lists.pop();
                }
                TagEnd::CodeBlock => {
                    st.in_code = false;
                    if st.code_text.ends_with('\n') {
                        st.code_text.pop();
                    }
                    if !st.code_text.trim().is_empty() {
                        st.blocks.push(MdBlock::CodeBlock {
                            lang: st.code_lang.take(),
                            text: std::mem::take(&mut st.code_text),
                        });
                    }
                }
                TagEnd::Strong
                | TagEnd::Emphasis
                | TagEnd::Strikethrough
                | TagEnd::Link
                | TagEnd::Image => st.close_inline(),
                _ => {}
            },
            Event::Text(text) => {
                if st.in_code {
                    st.code_text.push_str(&text);
                } else {
                    st.push_text(&text);
                }
            }
            Event::Code(text) => {
                let start = st.cursor();
                st.push_text(&text);
                st.buf.marks.push(InlineMark {
                    kind: InlineKind::Code,
                    range: start..st.cursor(),
                });
            }
            // Soft breaks stay line breaks: plain multi-line script output
            // must not collapse into one wrapped paragraph.
            Event::SoftBreak | Event::HardBreak => {
                if st.in_code {
                    st.code_text.push('\n');
                } else {
                    st.push_text("\n");
                }
            }
            Event::Rule => {
                st.flush_block();
                st.blocks.push(MdBlock::Rule);
            }
            // Raw HTML and everything else is skipped, never rendered.
            _ => {}
        }
    }

    st.flush_block();
    normalize_marks(&mut st.blocks);
    st.blocks
}

/// Sort marks and clip overlaps so they satisfy the invariants of the
/// GPUI text APIs (sorted, non-overlapping, char-boundary aligned).
fn normalize_marks(blocks: &mut [MdBlock]) {
    for block in blocks.iter_mut() {
        let text_opt = match block {
            MdBlock::Heading { text, .. }
            | MdBlock::Paragraph(text)
            | MdBlock::ListItem { text, .. }
            | MdBlock::Blockquote(text) => Some(text),
            _ => None,
        };
        let Some(text) = text_opt else {
            continue;
        };
        text.marks.sort_by(|a, b| {
            a.range
                .start
                .cmp(&b.range.start)
                .then(b.range.end.cmp(&a.range.end))
        });
        let mut i = 1;
        while i < text.marks.len() {
            let prev_end = text.marks[i - 1].range.end;
            if text.marks[i].range.start < prev_end {
                if text.marks[i].range.end <= prev_end {
                    text.marks.remove(i);
                    continue;
                }
                text.marks[i].range.start = prev_end;
            }
            i += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blocks(input: &str) -> Vec<MdBlock> {
        parse(input)
    }

    #[test]
    fn headings_keep_level_and_text() {
        assert_eq!(
            blocks("# Title"),
            vec![MdBlock::Heading {
                level: 1,
                text: MdText {
                    text: "Title".into(),
                    marks: vec![]
                }
            }]
        );
        assert_eq!(
            blocks("### Small"),
            vec![MdBlock::Heading {
                level: 3,
                text: MdText {
                    text: "Small".into(),
                    marks: vec![]
                }
            }]
        );
    }

    fn paragraph<'a>(out: &'a [MdBlock], input: &str) -> &'a MdText {
        assert_eq!(
            out.len(),
            1,
            "expected one block for {input:?}, got: {out:?}"
        );
        match &out[0] {
            MdBlock::Paragraph(text) => text,
            other => panic!("expected a paragraph, got: {other:?}"),
        }
    }

    #[test]
    fn inline_marks_cover_styled_ranges() {
        let out = blocks("a **bold** and *it*");
        let text = paragraph(&out, "a **bold** and *it*");
        assert_eq!(text.text, "a bold and it");
        assert_eq!(
            text.marks,
            vec![
                InlineMark {
                    kind: InlineKind::Bold,
                    range: 2..6
                },
                InlineMark {
                    kind: InlineKind::Italic,
                    range: 11..13
                }
            ]
        );
    }

    #[test]
    fn inline_code_is_marked() {
        let out = blocks("run `ls -la` now");
        let text = paragraph(&out, "run `ls -la` now");
        assert_eq!(text.text, "run ls -la now");
        assert_eq!(
            text.marks,
            vec![InlineMark {
                kind: InlineKind::Code,
                range: 4..10
            }]
        );
    }

    #[test]
    fn fenced_code_block_keeps_language_and_lines() {
        let source = "before\n\n```rust\nfn main() {}\n```\n\nafter";
        let out = blocks(source);
        assert_eq!(out.len(), 3);
        assert_eq!(
            out[1],
            MdBlock::CodeBlock {
                lang: Some("rust".into()),
                text: "fn main() {}".into()
            }
        );
    }

    #[test]
    fn ordered_and_unordered_lists_number_items() {
        let out = blocks("- one\n- two\n\n1. first\n2. second");
        assert_eq!(
            out,
            vec![
                MdBlock::ListItem {
                    number: None,
                    text: MdText {
                        text: "one".into(),
                        marks: vec![]
                    }
                },
                MdBlock::ListItem {
                    number: None,
                    text: MdText {
                        text: "two".into(),
                        marks: vec![]
                    }
                },
                MdBlock::ListItem {
                    number: Some(1),
                    text: MdText {
                        text: "first".into(),
                        marks: vec![]
                    }
                },
                MdBlock::ListItem {
                    number: Some(2),
                    text: MdText {
                        text: "second".into(),
                        marks: vec![]
                    }
                },
            ]
        );
    }

    #[test]
    fn blockquote_wraps_paragraph() {
        let out = blocks("> quoted line");
        assert_eq!(out.len(), 1);
        assert!(matches!(&out[0], MdBlock::Blockquote(t) if t.text == "quoted line"));
    }

    #[test]
    fn horizontal_rule_becomes_rule_block() {
        let out = blocks("text\n\n---\n\nmore");
        assert_eq!(out.len(), 3);
        assert_eq!(out[1], MdBlock::Rule);
    }

    #[test]
    fn plain_multiline_text_keeps_line_breaks() {
        let out = blocks("line one\nline two\nline three");
        assert_eq!(
            out,
            vec![MdBlock::Paragraph(MdText {
                text: "line one\nline two\nline three".into(),
                marks: vec![]
            })]
        );
    }

    #[test]
    fn raw_html_is_dropped() {
        let out = blocks("<div>hidden</div>\n\nvisible");
        assert_eq!(
            out,
            vec![MdBlock::Paragraph(MdText {
                text: "visible".into(),
                marks: vec![]
            })]
        );
    }

    #[test]
    fn malformed_input_never_panics() {
        assert!(blocks("").is_empty());
        let out = blocks("**unclosed bold and [unclosed link");
        assert!(!out.is_empty());
        // No mark may extend past the text of its block.
        for block in &out {
            if let Some(text) = match block {
                MdBlock::Heading { text, .. }
                | MdBlock::Paragraph(text)
                | MdBlock::ListItem { text, .. }
                | MdBlock::Blockquote(text) => Some(text),
                _ => None,
            } {
                for mark in &text.marks {
                    assert!(mark.range.end <= text.text.len());
                    assert!(text.text.is_char_boundary(mark.range.start));
                    assert!(text.text.is_char_boundary(mark.range.end));
                }
            }
        }
    }

    #[test]
    fn nested_emphasis_marks_do_not_overlap() {
        let out = blocks("**bold and *inner* end**");
        let text = paragraph(&out, "**bold and *inner* end**");
        for pair in text.marks.windows(2) {
            assert!(pair[0].range.end <= pair[1].range.start);
        }
    }
}
