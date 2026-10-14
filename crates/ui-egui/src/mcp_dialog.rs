//! File ▸ MCP…: what an AI assistant needs to design in this app. The app starts its MCP server
//! by itself; `figcraft-cli mcp --app` finds it (the endpoint file in the settings folder), so the
//! configs below never hold a port or a token and keep working across restarts.

use egui::{Context, RichText};
use serde_json::json;

use crate::FigApp;
use crate::i18n::{tr, trf};
use crate::theme::{Tokens, regular, semibold};

/// One client's setup: what to name it, where its config lives, and the text to put there.
#[derive(Debug, PartialEq, Eq)]
pub struct Snippet {
    pub client: &'static str,
    /// The file to put it in, or the shell to run it in.
    pub place: &'static str,
    pub text: String,
}

/// The `mcpServers` JSON shared by Claude Desktop, Claude Code (`.mcp.json`) and Cursor.
fn servers_json(cli: &str) -> String {
    let v = json!({"mcpServers": {"figcraft": {"command": cli, "args": ["mcp", "--app"]}}});
    serde_json::to_string_pretty(&v).unwrap_or_default()
}

/// A TOML literal string (single quotes: Windows paths keep their backslashes).
fn toml_literal(s: &str) -> String {
    if s.contains('\'') || s.contains('\n') { format!("{s:?}") } else { format!("'{s}'") }
}

/// A command-line word, quoted when it has a space.
fn word(s: &str) -> String {
    if s.contains(' ') { format!("\"{s}\"") } else { s.to_string() }
}

/// The setup text for each client, with `cli` the path of `figcraft-cli`.
pub fn snippets(cli: &str) -> Vec<Snippet> {
    vec![
        Snippet { client: "Claude Desktop", place: "claude_desktop_config.json (Settings ▸ Developer ▸ Edit Config)", text: servers_json(cli) },
        Snippet { client: "Claude Code", place: "terminal", text: format!("claude mcp add figcraft -- {} mcp --app", word(cli)) },
        Snippet { client: "Claude Code", place: ".mcp.json", text: servers_json(cli) },
        Snippet { client: "Cursor", place: "~/.cursor/mcp.json", text: servers_json(cli) },
        Snippet {
            client: "Codex",
            place: "~/.codex/config.toml",
            text: format!("[mcp_servers.figcraft]\ncommand = {}\nargs = [\"mcp\", \"--app\"]", toml_literal(cli)),
        },
    ]
}

/// The window, while `ui.mcp` is set.
pub fn show(app: &mut FigApp, ctx: &Context) {
    if !app.ui.mcp {
        return;
    }
    let t = Tokens::get(ctx);
    let running = app.services.mcp_port().is_some();
    let found = app.services.mcp_cli();
    let cli = found.as_ref().map_or_else(|| "figcraft-cli".to_string(), |p| p.to_string_lossy().into_owned());
    let mut open = true;
    let mut copied = None;
    crate::dialogs::window(tr("MCP"), ctx).open(&mut open).show(ctx, |ui| {
        ui.set_width(520.0);
        ui.label(
            RichText::new(tr("AI assistants can design in FigCraft through MCP. FigCraft starts the MCP server by itself while it is open; add FigCraft to your assistant with one of these."))
                .color(t.text_secondary)
                .font(regular(11.0)),
        );
        ui.add_space(6.0);
        let status = if running { tr("The MCP server is running.") } else { tr("The MCP server is off (FigCraft was started with --no-mcp).") };
        ui.label(RichText::new(status).font(semibold(11.0)));
        if found.is_none() {
            ui.add_space(4.0);
            ui.label(RichText::new(tr("figcraft-cli isn't next to FigCraft; the setups below expect it on your PATH.")).color(t.text_secondary).font(regular(11.0)));
        }
        egui::ScrollArea::vertical().max_height(380.0).show(ui, |ui| {
            for s in snippets(&cli) {
                ui.add_space(10.0);
                ui.label(RichText::new(s.client).font(semibold(12.0)));
                ui.label(RichText::new(s.place).color(t.text_secondary).font(regular(11.0)));
                let mut text = s.text.as_str();
                ui.add(egui::TextEdit::multiline(&mut text).code_editor().desired_width(f32::INFINITY).desired_rows(1));
                if crate::widgets::button(ui, tr("Copy"), false, true).clicked() {
                    copied = Some((s.client, s.text.clone()));
                }
            }
        });
    });
    if let Some((client, text)) = copied {
        app.services.clipboard_set_text(&text);
        app.toast(trf("Copied {value}", &[("value", &client)]));
    }
    app.ui.mcp = open;
}

#[cfg(test)]
#[path = "mcp_dialog_tests.rs"]
mod tests;
