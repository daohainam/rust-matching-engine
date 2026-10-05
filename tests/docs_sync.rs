//! Keeps the course honest: chapter 9 shows `examples/pipeline.rs` in full,
//! and CI compiles and runs that file. This test fails if the copy in the
//! chapter ever differs from the real program. The Vietnamese translation
//! carries the same code, so it is held to the same rule.

// Git may check files out with CRLF line endings on Windows.
fn normalize(text: &str) -> String {
    text.replace("\r\n", "\n")
}

/// The body of every ```rust fenced block in a Markdown document.
fn rust_blocks(markdown: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut open: Option<Vec<&str>> = None;
    for line in markdown.lines() {
        match (&mut open, line.trim_end()) {
            (None, "```rust") => open = Some(Vec::new()),
            (Some(body), "```") => {
                blocks.push(body.join("\n"));
                open = None;
            }
            (Some(body), _) => body.push(line),
            (None, _) => {}
        }
    }
    blocks
}

#[test]
fn chapter_9_shows_the_pipeline_example_verbatim() {
    let chapter = normalize(include_str!("../docs/09-write-ahead-log.md"));
    let example = normalize(include_str!("../examples/pipeline.rs"));
    assert!(
        chapter.contains(&example),
        "docs/09-write-ahead-log.md is out of date: paste examples/pipeline.rs into section 9.7"
    );
}

#[test]
fn vietnamese_chapter_9_shows_the_pipeline_example_verbatim() {
    let chapter = normalize(include_str!("../docs/vi/09-write-ahead-log.md"));
    let example = normalize(include_str!("../examples/pipeline.rs"));
    assert!(
        chapter.contains(&example),
        "docs/vi/09-write-ahead-log.md is out of date: paste examples/pipeline.rs into section 9.7"
    );
}

/// `README.md` is compiled as a doctest (see `src/lib.rs`); the Vietnamese
/// README is not, so its Rust examples must be the very same code.
#[test]
fn vietnamese_readme_has_the_same_rust_examples_as_the_english_one() {
    let english = rust_blocks(include_str!("../README.md"));
    let vietnamese = rust_blocks(include_str!("../README.vi.md"));
    assert!(
        !english.is_empty(),
        "README.md should contain Rust examples"
    );
    assert_eq!(
        vietnamese, english,
        "README.vi.md is out of date: its Rust blocks must match README.md exactly"
    );
}
