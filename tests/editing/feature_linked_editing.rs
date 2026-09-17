use super::*;

use expect_test::expect;

// ── LSP-spec same-text invariant ────────────────────────────────────────────

/// Check the LSP requirement that all linked ranges cover the same text.
async fn check_invariant(
    s: &mut TestServer,
    path: &str,
    src: &str,
    line: u32,
    character: u32,
) -> String {
    s.open(path, src).await;
    let resp = s.linked_editing_range(path, line, character).await;
    assert_linked_editing_ranges_share_text(&resp, src);
    render_linked_editing_range(&resp)
}

/// Request linked editing at the fixture cursor and verify its range invariant.
async fn check_response(s: &mut TestServer, src: &str) -> String {
    let opened = s.open_fixture(src).await;
    let cursor = opened.cursor().clone();
    let source = opened
        .fixture
        .files
        .iter()
        .find(|file| file.path == cursor.path)
        .expect("cursor file should be part of its fixture");
    let response = s
        .linked_editing_range(&cursor.path, cursor.line, cursor.character)
        .await;
    assert_linked_editing_ranges_share_text(&response, &source.text);
    render_linked_editing_range(&response)
}

#[tokio::test]
async fn linked_ranges_cover_same_text_across_fixtures() {
    let mut s = TestServer::new().await;
    s.validate_syntax(false);
    // Function declaration and calls.
    let fn_out = check_invariant(
        &mut s,
        "fn.php",
        "<?php\nfunction greet() {}\ngreet();\ngreet();\n",
        1,
        12,
    )
    .await;
    // Method declaration and same-class call.
    let mut s = TestServer::new().await;
    s.validate_syntax(false);
    let method_out = check_invariant(
        &mut s,
        "method.php",
        "<?php\nclass Calc {\n    public function add(): void {}\n    public function self_call(): void { $this->add(); }\n}\n",
        2,
        22,
    )
    .await;
    // Variable declaration and uses.
    let mut s = TestServer::new().await;
    s.validate_syntax(false);
    let var_out = check_invariant(
        &mut s,
        "var.php",
        "<?php\nfunction f(): void {\n    $foo = 1;\n    echo $foo;\n    $foo += 2;\n}\n",
        2,
        6,
    )
    .await;
    // Unicode identifier.
    let mut s = TestServer::new().await;
    s.validate_syntax(false);
    let cjk_out = check_invariant(
        &mut s,
        "cjk.php",
        "<?php\nfunction 名前() {}\n名前();\n",
        1,
        10,
    )
    .await;

    expect![[r#"
        1:9-1:14
        2:0-2:5
        3:0-3:5
        pattern: [a-zA-Z_\u00A0-\uFFFF][a-zA-Z0-9_\u00A0-\uFFFF]*
        ---
        2:20-2:23
        3:47-3:50
        pattern: [a-zA-Z_\u00A0-\uFFFF][a-zA-Z0-9_\u00A0-\uFFFF]*
        ---
        2:4-2:8
        3:9-3:13
        4:4-4:8
        pattern: \$[a-zA-Z_\u00A0-\uFFFF][a-zA-Z0-9_\u00A0-\uFFFF]*
        ---
        1:9-1:11
        2:0-2:2
        pattern: [a-zA-Z_\u00A0-\uFFFF][a-zA-Z0-9_\u00A0-\uFFFF]*"#]]
    .assert_eq(&[fn_out, method_out, var_out, cjk_out].join("\n---\n"));
}

// ── basic shape: declaration only ───────────────────────────────────────────

#[tokio::test]
async fn class_with_only_declaration_yields_one_range() {
    let mut s = TestServer::new().await;
    s.validate_syntax(false);
    let out = check_response(&mut s, "<?php\nclass Lin$0kedClass {}\n").await;
    expect![[r#"
        1:6-1:17
        pattern: [a-zA-Z_\u00A0-\uFFFF][a-zA-Z0-9_\u00A0-\uFFFF]*"#]]
    .assert_eq(&out);
}

// ── functions ───────────────────────────────────────────────────────────────

#[tokio::test]
async fn function_decl_links_to_all_call_sites() {
    let mut s = TestServer::new().await;
    s.validate_syntax(false);
    let out = check_response(
        &mut s,
        r#"<?php
function gre$0et() {}
greet();
greet();
"#,
    )
    .await;
    expect![[r#"
        1:9-1:14
        2:0-2:5
        3:0-3:5
        pattern: [a-zA-Z_\u00A0-\uFFFF][a-zA-Z0-9_\u00A0-\uFFFF]*"#]]
    .assert_eq(&out);
}

#[tokio::test]
async fn function_call_links_back_to_decl() {
    let mut s = TestServer::new().await;
    s.validate_syntax(false);
    let out = check_response(
        &mut s,
        r#"<?php
function greet() {}
gr$0eet();
"#,
    )
    .await;
    expect![[r#"
        1:9-1:14
        2:0-2:5
        pattern: [a-zA-Z_\u00A0-\uFFFF][a-zA-Z0-9_\u00A0-\uFFFF]*"#]]
    .assert_eq(&out);
}

#[tokio::test]
async fn namespaced_function_decl_links_to_all_call_sites() {
    let mut s = TestServer::new().await;
    s.validate_syntax(false);
    let out = check_response(
        &mut s,
        r#"<?php
namespace App;
function greet() {}
gr$0eet();
"#,
    )
    .await;
    expect![[r#"
        2:9-2:14
        3:0-3:5
        pattern: [a-zA-Z_\u00A0-\uFFFF][a-zA-Z0-9_\u00A0-\uFFFF]*"#]]
    .assert_eq(&out);
}

// ── classes & members ──────────────────────────────────────────────────────

#[tokio::test]
async fn class_decl_and_new_expression() {
    let mut s = TestServer::new().await;
    s.validate_syntax(false);
    let out = check_response(&mut s, "<?php\nclass F$0oo {}\n$x = new Foo();\n").await;
    expect![[r#"
        1:6-1:9
        2:9-2:12
        pattern: [a-zA-Z_\u00A0-\uFFFF][a-zA-Z0-9_\u00A0-\uFFFF]*"#]]
    .assert_eq(&out);
}

#[tokio::test]
async fn method_decl_and_call() {
    let mut s = TestServer::new().await;
    s.validate_syntax(false);
    let out = check_response(
        &mut s,
        r#"<?php
class Calc {
    public function ad$0d(): void {}
}
$c = new Calc();
$c->add();
"#,
    )
    .await;
    expect![[r#"
        2:20-2:23
        5:4-5:7
        pattern: [a-zA-Z_\u00A0-\uFFFF][a-zA-Z0-9_\u00A0-\uFFFF]*"#]]
    .assert_eq(&out);
}

// ── variables ──────────────────────────────────────────────────────────────

#[tokio::test]
async fn variable_in_scope_links_all_occurrences_with_dollar_pattern() {
    let mut s = TestServer::new().await;
    s.validate_syntax(false);
    let out = check_response(
        &mut s,
        r#"<?php
function f(): void {
    $fo$0o = 1;
    echo $foo;
    $foo += 2;
}
"#,
    )
    .await;
    expect![[r#"
        2:4-2:8
        3:9-3:13
        4:4-4:8
        pattern: \$[a-zA-Z_\u00A0-\uFFFF][a-zA-Z0-9_\u00A0-\uFFFF]*"#]]
    .assert_eq(&out);
}

#[tokio::test]
async fn variable_does_not_cross_function_scope() {
    let mut s = TestServer::new().await;
    s.validate_syntax(false);
    let out = check_response(
        &mut s,
        r#"<?php
function f() { $x$0 = 1; }
function g() { $x = 2; }
"#,
    )
    .await;
    expect![[r#"
        1:15-1:17
        pattern: \$[a-zA-Z_\u00A0-\uFFFF][a-zA-Z0-9_\u00A0-\uFFFF]*"#]]
    .assert_eq(&out);
}

#[tokio::test]
async fn cursor_on_dollar_sign_still_finds_variable() {
    let mut s = TestServer::new().await;
    s.validate_syntax(false);
    let out = check_response(&mut s, "<?php\nfunction f() { $0$x = 1; echo $x; }\n").await;
    expect![[r#"
        1:15-1:17
        1:28-1:30
        pattern: \$[a-zA-Z_\u00A0-\uFFFF][a-zA-Z0-9_\u00A0-\uFFFF]*"#]]
    .assert_eq(&out);
}

// ── positions that should NOT trigger linked editing ────────────────────────

#[tokio::test]
async fn whitespace_returns_no_linked_editing() {
    let mut s = TestServer::new().await;
    s.validate_syntax(false);
    let out = check_response(&mut s, "<?php\nclass Foo {} $0  $x = 1;\n").await;
    expect!["<no linked editing>"].assert_eq(&out);
}

#[tokio::test]
async fn unknown_word_returns_no_linked_editing() {
    let mut s = TestServer::new().await;
    s.validate_syntax(false);
    let out = check_response(&mut s, "<?php\necho 'nob$0ody';\n").await;
    expect!["<no linked editing>"].assert_eq(&out);
}

#[tokio::test]
async fn comment_word_matching_class_name_does_not_link() {
    // Comments are not editable symbol references.
    let mut s = TestServer::new().await;
    s.validate_syntax(false);
    let out = check_response(
        &mut s,
        "<?php\n// uses Fo$0o here\nclass Foo {}\n$x = new Foo();\n",
    )
    .await;
    expect!["<no linked editing>"].assert_eq(&out);
}

#[tokio::test]
async fn class_modifier_keyword_matching_method_name_does_not_link() {
    // A class modifier is not a method reference.
    let mut s = TestServer::new().await;
    s.validate_syntax(false);
    let out = check_response(&mut s,
            "<?php\nclass Registry {\n    public function final(): void {}\n}\nfina$0l class Locked {}\n",
        )
        .await;
    expect!["<no linked editing>"].assert_eq(&out);
}

#[tokio::test]
async fn string_literal_word_matching_function_name_does_not_link() {
    // String contents are not editable symbol references.
    let mut s = TestServer::new().await;
    s.validate_syntax(false);
    let out = check_response(
        &mut s,
        "<?php\nfunction greet() {}\n$x = 'gr$0eet';\ngreet();\n",
    )
    .await;
    expect!["<no linked editing>"].assert_eq(&out);
}

// ── word pattern correctness ────────────────────────────────────────────────

#[tokio::test]
async fn non_variable_pattern_disallows_dollar_sign() {
    // Class names use the identifier pattern, without a dollar prefix.
    let mut s = TestServer::new().await;
    s.validate_syntax(false);
    let out = check_response(&mut s, "<?php\nclass Fo$0o {}\n").await;
    expect![[r#"
        1:6-1:9
        pattern: [a-zA-Z_\u00A0-\uFFFF][a-zA-Z0-9_\u00A0-\uFFFF]*"#]]
    .assert_eq(&out);
}

#[tokio::test]
async fn variable_pattern_requires_dollar_sign() {
    // Variable names require the dollar-prefixed pattern.
    let mut s = TestServer::new().await;
    s.validate_syntax(false);
    let out = check_response(&mut s, "<?php\nfunction f() { $x$0 = 1; }\n").await;
    expect![[r#"
        1:15-1:17
        pattern: \$[a-zA-Z_\u00A0-\uFFFF][a-zA-Z0-9_\u00A0-\uFFFF]*"#]]
    .assert_eq(&out);
}

// ── unicode identifier support ─────────────────────────────────────────────

#[tokio::test]
async fn method_in_one_class_does_not_link_unrelated_class_with_same_name() {
    // Same-named methods are linked only within their owning class.
    let mut s = TestServer::new().await;
    s.validate_syntax(false);
    let out = check_response(
        &mut s,
        r#"<?php
class A {
    public function ba$0r(): void {}
}
class B {
    public function bar(): void {}
}
"#,
    )
    .await;
    expect![[r#"
        2:20-2:23
        pattern: [a-zA-Z_\u00A0-\uFFFF][a-zA-Z0-9_\u00A0-\uFFFF]*"#]]
    .assert_eq(&out);
}

#[tokio::test]
async fn class_name_itself_still_links_globally() {
    // A class declaration links to its class-name uses.
    let mut s = TestServer::new().await;
    s.validate_syntax(false);
    let out = check_response(&mut s, "<?php\nclass Fo$0o {}\n$x = new Foo();\n").await;
    expect![[r#"
        1:6-1:9
        2:9-2:12
        pattern: [a-zA-Z_\u00A0-\uFFFF][a-zA-Z0-9_\u00A0-\uFFFF]*"#]]
    .assert_eq(&out);
}

#[tokio::test]
async fn cjk_identifier_links_correctly() {
    // CJK identifiers use the same identifier pattern.
    let mut s = TestServer::new().await;
    s.validate_syntax(false);
    let out = check_response(&mut s, "<?php\nfunction 名$0前() {}\n名前();\n").await;
    expect![[r#"
        1:9-1:11
        2:0-2:2
        pattern: [a-zA-Z_\u00A0-\uFFFF][a-zA-Z0-9_\u00A0-\uFFFF]*"#]]
    .assert_eq(&out);
}

#[tokio::test]
async fn utf8_identifier_links_correctly() {
    let mut s = TestServer::new().await;
    s.validate_syntax(false);
    let out = check_response(&mut s, "<?php\nfunction caf$0é() {}\ncafé();\n").await;
    expect![[r#"
        1:9-1:13
        2:0-2:4
        pattern: [a-zA-Z_\u00A0-\uFFFF][a-zA-Z0-9_\u00A0-\uFFFF]*"#]]
    .assert_eq(&out);
}
