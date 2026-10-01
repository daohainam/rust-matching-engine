//! Keeps the course honest: chapter 9 shows `examples/pipeline.rs` in full,
//! and CI compiles and runs that file. This test fails if the copy in the
//! chapter ever differs from the real program.

#[test]
fn chapter_9_shows_the_pipeline_example_verbatim() {
    // Git may check files out with CRLF line endings on Windows.
    let normalize = |text: &str| text.replace("\r\n", "\n");
    let chapter = normalize(include_str!("../docs/09-write-ahead-log.md"));
    let example = normalize(include_str!("../examples/pipeline.rs"));
    assert!(
        chapter.contains(&example),
        "docs/09-write-ahead-log.md is out of date: paste examples/pipeline.rs into section 9.7"
    );
}
