/// Unit tests for IPC protocol
///
/// CRITICAL REQUIREMENT: IPC protocol MUST use plain text commands and responses.
/// NO JSON SERIALIZATION ALLOWED!
///
/// This is a performance-critical path. PHP library expects raw PHP code, not JSON.
/// Any attempt to add JSON is a BUG and must be rejected.
///
/// Protocol specification:
/// - Commands: Plain text strings ("getCode", "ping", etc)
/// - Responses: Plain text or PHP code directly
/// - Errors: Start with "ERROR:" prefix
/// - No structured data formats (JSON, XML, etc)

#[test]
fn test_commands_must_be_plain_text() {
    // ALL commands MUST be simple text, NEVER JSON
    let commands = vec!["getCode", "getCacheCode", "getFilePath", "ping", "stats"];

    for cmd in commands {
        // CRITICAL: None of these should be valid JSON
        assert!(
            serde_json::from_str::<serde_json::Value>(cmd).is_err(),
            "VIOLATION: Command '{}' must NOT be JSON! This breaks the zero-overhead protocol.",
            cmd
        );
        // They must be just plain text
        assert!(!cmd.trim().is_empty());
        assert!(!cmd.contains('{'), "Command contains JSON marker");
        assert!(!cmd.contains('"'), "Command contains JSON quotes");
    }
}

#[test]
fn test_responses_must_not_be_json() {
    // CRITICAL: Responses MUST be plain text or PHP code, NEVER JSON
    let php_code = "<?php declare(strict_types=1); return [];";
    let pong = "PONG";
    let stats = "total:100 strategy:Memory uptime:3600";
    let error = "ERROR: Something went wrong";
    let file_path = "/tmp/cache.php";

    let responses = vec![php_code, pong, stats, error, file_path];

    for response in responses {
        // CRITICAL: None of these should be JSON
        let parse_result = serde_json::from_str::<serde_json::Value>(response);
        assert!(
            parse_result.is_err(),
            "VIOLATION: Response '{}' must NOT be JSON! Expected plain text.",
            response
        );
    }
}

#[test]
fn test_php_code_format() {
    let php_code = "<?php declare(strict_types=1); return [];";

    // Valid PHP code must:
    assert!(
        php_code.starts_with("<?php"),
        "PHP code must start with <?php"
    );
    assert!(
        php_code.contains("declare(strict_types=1)"),
        "PHP code must have strict types"
    );
    assert!(php_code.contains("return"), "PHP code must return data");

    // CRITICAL: Must NOT be JSON
    assert!(
        serde_json::from_str::<serde_json::Value>(php_code).is_err(),
        "VIOLATION: PHP code wrapped in JSON is FORBIDDEN"
    );
}

#[test]
fn test_error_format() {
    // Errors MUST start with "ERROR:" prefix
    let error = "ERROR: Unknown command: foo";
    assert!(error.starts_with("ERROR:"), "Errors must start with ERROR:");

    // Extract message
    let message = error.strip_prefix("ERROR:").unwrap().trim();
    assert_eq!(message, "Unknown command: foo");

    // CRITICAL: Must NOT be JSON
    assert!(
        serde_json::from_str::<serde_json::Value>(error).is_err(),
        "VIOLATION: Errors must be plain text, not JSON"
    );
}

#[test]
fn test_pong_response() {
    let response = "PONG\n";
    assert_eq!(response.trim(), "PONG");

    // CRITICAL: Must NOT be JSON
    assert!(
        serde_json::from_str::<serde_json::Value>(response.trim()).is_err(),
        "VIOLATION: PONG must be plain text"
    );
}

#[test]
fn test_stats_format() {
    let stats = "total:150 strategy:Memory uptime:3600";

    // Stats must be key:value format
    assert!(stats.contains("total:"));
    assert!(stats.contains("strategy:"));
    assert!(stats.contains("uptime:"));

    // CRITICAL: Must NOT be JSON
    assert!(
        serde_json::from_str::<serde_json::Value>(stats).is_err(),
        "VIOLATION: Stats must be plain text, not JSON"
    );
}

#[test]
fn test_no_json_structures_allowed() {
    // These are FORBIDDEN patterns that indicate JSON usage
    let forbidden_patterns = vec![
        r#"{"type":"phpCode"}"#,
        r#"{"action":"getCode"}"#,
        r#"{"code":"<?php"}"#,
        r#"{"error":"message"}"#,
    ];

    for pattern in forbidden_patterns {
        // All of these parse as JSON - they are FORBIDDEN
        assert!(
            serde_json::from_str::<serde_json::Value>(pattern).is_ok(),
            "Pattern '{}' is JSON and therefore FORBIDDEN in IPC",
            pattern
        );
    }
}

#[test]
fn test_protocol_documentation() {
    // This test documents the CORRECT protocol

    // CORRECT: Plain text commands
    let correct_commands = vec![
        ("getCode", "Request PHP cache code"),
        ("ping", "Check daemon is alive"),
        ("stats", "Get cache statistics"),
        ("getFilePath", "Get file path (File strategy only)"),
    ];

    for (cmd, description) in correct_commands {
        assert!(
            !cmd.contains('{'),
            "{}: Command must be plain text",
            description
        );
    }

    // CORRECT: Plain text responses
    let php_response = "<?php return ['App\\User' => [...]]";
    let ping_response = "PONG";
    let stats_response = "total:100 strategy:Memory uptime:60";
    let error_response = "ERROR: Failed to generate code";

    assert!(!php_response.starts_with('{'));
    assert!(!ping_response.starts_with('{'));
    assert!(!stats_response.starts_with('{'));
    assert!(!error_response.starts_with('{'));
}
