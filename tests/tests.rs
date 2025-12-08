use jsonc::strip_comments;

#[test]
fn test_strip_line_comments() {
    let input = r#"{
// this is a comment
"key": "value"
}"#;
    let stripped = strip_comments(input);
    let parsed: serde_json::Value = serde_json::from_str(&stripped).unwrap();
    assert_eq!(parsed["key"], "value");
}

#[test]
fn test_strip_block_comments() {
    let input = r#"{
/* block comment */
"key": "value"
}"#;
    let stripped = strip_comments(input);
    let parsed: serde_json::Value = serde_json::from_str(&stripped).unwrap();
    assert_eq!(parsed["key"], "value");
}

#[test]
fn test_multiline_block_comment() {
    let input = r#"{
/* 
    multi-line
    block comment 
*/
"key": "value"
}"#;
    let stripped = strip_comments(input);
    let parsed: serde_json::Value = serde_json::from_str(&stripped).unwrap();
    assert_eq!(parsed["key"], "value");
}

#[test]
fn test_preserve_slashes_in_strings() {
    let input = r#"{
"path": "/dev/null",
"url": "https://example.com"
}"#;
    let stripped = strip_comments(input);
    let parsed: serde_json::Value = serde_json::from_str(&stripped).unwrap();
    assert_eq!(parsed["path"], "/dev/null");
    assert_eq!(parsed["url"], "https://example.com");
}

#[test]
fn test_comment_after_value() {
    let input = r#"{
"key": "value" // inline comment
}"#;
    let stripped = strip_comments(input);
    let parsed: serde_json::Value = serde_json::from_str(&stripped).unwrap();
    assert_eq!(parsed["key"], "value");
}

#[test]
fn test_complex_jsonc() {
    let input = r#"{
// lol
// Add your configuration here
"example": "value",
"number": 42,
"enabled": [
"yes",
"no",
"maybe",
"/dev/tvdev"
],
"settings": {
// this is a comment
"option1": true,
"option2": false
// "option3": null
}
}"#;
    let stripped = strip_comments(input);
    let parsed: serde_json::Value = serde_json::from_str(&stripped).unwrap();
    assert_eq!(parsed["example"], "value");
    assert_eq!(parsed["number"], 42);
    assert_eq!(parsed["enabled"][3], "/dev/tvdev");
    assert_eq!(parsed["settings"]["option1"], true);
}
