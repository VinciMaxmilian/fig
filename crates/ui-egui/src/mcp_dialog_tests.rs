#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::*;
use serde_json::Value;

const CLI: &str = r"C:\Program Files\FigCraft\figcraft-cli.exe";

#[test]
fn every_client_gets_a_setup_that_starts_the_app_server() {
    let all = snippets(CLI);
    let clients: Vec<_> = all.iter().map(|s| s.client).collect();
    assert_eq!(clients, ["Claude Desktop", "Claude Code", "Claude Code", "Cursor", "Codex"]);
    for s in &all {
        assert!(s.text.contains("--app"), "{}: {}", s.client, s.text);
        assert!(!s.text.contains("--attach"), "no port is baked in: {}", s.text);
    }
}

#[test]
fn the_json_setups_are_valid_json_with_the_path_intact() {
    for s in snippets(CLI).iter().filter(|s| s.text.starts_with('{')) {
        let v: Value = serde_json::from_str(&s.text).unwrap();
        assert_eq!(v["mcpServers"]["figcraft"]["command"], CLI);
        assert_eq!(v["mcpServers"]["figcraft"]["args"], json!(["mcp", "--app"]));
    }
}

#[test]
fn the_codex_setup_is_toml_that_keeps_windows_backslashes() {
    let s = snippets(CLI).into_iter().find(|s| s.client == "Codex").unwrap();
    // A single-quoted TOML string takes backslashes as they are.
    assert_eq!(
        s.text,
        format!(
            "[mcp_servers.figcraft]
command = '{CLI}'
args = [\"mcp\", \"--app\"]"
        )
    );
}

#[test]
fn the_shell_command_quotes_a_path_with_spaces_only() {
    let s = snippets(CLI).into_iter().find(|s| s.place == "terminal").unwrap();
    assert_eq!(s.text, format!("claude mcp add figcraft -- \"{CLI}\" mcp --app"));
    assert_eq!(snippets("figcraft-cli")[1].text, "claude mcp add figcraft -- figcraft-cli mcp --app");
}

#[test]
fn awkward_paths_never_break_the_toml() {
    assert_eq!(toml_literal("a'b"), "\"a'b\"");
    assert_eq!(toml_literal("plain"), "'plain'");
}
