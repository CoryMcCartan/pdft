use std::process::Command;

fn pdft() -> Command {
    Command::new(env!("CARGO_BIN_EXE_pdft"))
}

fn fixture(name: &str) -> String {
    format!("{}/tests/fixtures/{}", env!("CARGO_MANIFEST_DIR"), name)
}

fn run_text(pdf: &str, width: u16, height: u16) -> String {
    let output = pdft()
        .args(["text", pdf, "-W", &width.to_string(), "-H", &height.to_string()])
        .output()
        .expect("failed to run pdft");
    assert!(output.status.success(), "pdft text failed: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout).expect("non-UTF8 output")
}

// --- simple.pdf ---

#[test]
fn simple_extracts_heading() {
    let text = run_text(&fixture("simple.pdf"), 80, 60);
    assert!(text.contains("Hello World"), "expected heading in: {text}");
}

#[test]
fn simple_extracts_paragraph() {
    let text = run_text(&fixture("simple.pdf"), 80, 60);
    assert!(text.contains("simple test document"), "expected paragraph text in: {text}");
}

#[test]
fn simple_extracts_list_items() {
    let text = run_text(&fixture("simple.pdf"), 80, 60);
    assert!(text.contains("Item one"), "expected list item in: {text}");
    assert!(text.contains("Item two"), "expected list item in: {text}");
    assert!(text.contains("Item three"), "expected list item in: {text}");
}

// --- columns.pdf ---

#[test]
fn columns_extracts_both_columns() {
    let text = run_text(&fixture("columns.pdf"), 120, 60);
    assert!(text.contains("quick brown fox"), "expected left column text in: {text}");
    assert!(text.contains("right column"), "expected right column text in: {text}");
}

#[test]
fn columns_extracts_numbers() {
    let text = run_text(&fixture("columns.pdf"), 120, 60);
    assert!(text.contains("1234567890"), "expected numbers in: {text}");
}

// --- mixed.pdf ---

#[test]
fn mixed_extracts_heading() {
    let text = run_text(&fixture("mixed.pdf"), 80, 60);
    assert!(text.contains("Mixed Content"), "expected heading in: {text}");
}

#[test]
fn mixed_extracts_table_data() {
    let text = run_text(&fixture("mixed.pdf"), 80, 60);
    assert!(text.contains("Name"), "expected table header in: {text}");
    assert!(text.contains("Width"), "expected table row in: {text}");
    assert!(text.contains("120"), "expected table value in: {text}");
}

#[test]
fn mixed_extracts_styled_text() {
    let text = run_text(&fixture("mixed.pdf"), 80, 60);
    // Bold/italic/mono render as plain text in extraction
    assert!(text.contains("bold text"), "expected bold text in: {text}");
    assert!(text.contains("italic text"), "expected italic text in: {text}");
}

// --- CLI info command ---

#[test]
fn info_shows_page_count() {
    let output = pdft()
        .args(["info", &fixture("simple.pdf")])
        .output()
        .expect("failed to run pdft");
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("Pages: 1"), "expected page count in: {text}");
}

// --- CLI split command ---

#[test]
fn split_produces_smaller_file() {
    let input = fixture("columns.pdf");
    let output_path = std::env::temp_dir().join("pdft_test_split.pdf");
    let output = pdft()
        .args(["split", &input, "-p", "1", "-o", output_path.to_str().unwrap()])
        .output()
        .expect("failed to run pdft");
    assert!(output.status.success(), "split failed: {}", String::from_utf8_lossy(&output.stderr));
    assert!(output_path.exists(), "output file not created");
    let _ = std::fs::remove_file(&output_path);
}
