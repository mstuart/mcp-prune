use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Where an MCP server's configuration lives — determines whether `mcp-prune
/// apply` can act on it.
///
/// - `UserConfig` / `ProjectConfig`: removable via `claude mcp remove`.
/// - `Plugin`: parent plugin is currently enabled; disable it with
///   `claude plugin disable <plugin>`.
/// - `Historical`: server only appears in past transcripts — not in any
///   current config or enabled plugin. Nothing to remove.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServerSource {
    UserConfig,
    ProjectConfig,
    Plugin,
    #[default]
    Historical,
}

#[derive(Debug, Default, Clone)]
pub struct Installed {
    pub user: HashSet<String>,
    pub project: HashSet<String>,
    pub enabled_plugins: HashSet<String>,
}

impl Installed {
    pub fn classify(&self, server: &str) -> ServerSource {
        if let Some(rest) = server.strip_prefix("plugin_") {
            let plugin = rest.split_once('_').map(|(p, _)| p).unwrap_or(rest);
            return if self.enabled_plugins.contains(plugin) {
                ServerSource::Plugin
            } else {
                ServerSource::Historical
            };
        }
        if self.user.contains(server) {
            ServerSource::UserConfig
        } else if self.project.contains(server) {
            ServerSource::ProjectConfig
        } else {
            ServerSource::Historical
        }
    }
}

pub fn load() -> Installed {
    let mut out = Installed::default();
    let Some(home) = dirs::home_dir() else {
        return out;
    };
    load_claude_json(&home.join(".claude.json"), &mut out);
    load_settings_json(&home.join(".claude").join("settings.json"), &mut out);
    out
}

fn load_claude_json(path: &Path, out: &mut Installed) {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return;
    };
    let Ok(v) = serde_json::from_str::<Value>(&raw) else {
        return;
    };
    if let Some(servers) = v.get("mcpServers").and_then(Value::as_object) {
        out.user.extend(servers.keys().cloned());
    }
    if let Some(projects) = v.get("projects").and_then(Value::as_object) {
        for (project_path, proj) in projects {
            if let Some(servers) = proj.get("mcpServers").and_then(Value::as_object) {
                out.project.extend(servers.keys().cloned());
            }
            load_project_mcp_json(&PathBuf::from(project_path).join(".mcp.json"), out);
        }
    }
}

fn load_project_mcp_json(path: &Path, out: &mut Installed) {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return;
    };
    let Ok(v) = serde_json::from_str::<Value>(&raw) else {
        return;
    };
    if let Some(servers) = v.get("mcpServers").and_then(Value::as_object) {
        out.project.extend(servers.keys().cloned());
    }
}

fn load_settings_json(path: &Path, out: &mut Installed) {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return;
    };
    let Ok(v) = serde_json::from_str::<Value>(&raw) else {
        return;
    };
    let Some(plugins) = v.get("enabledPlugins").and_then(Value::as_object) else {
        return;
    };
    for (key, val) in plugins {
        if val.as_bool() != Some(true) {
            continue;
        }
        let plugin = key.split_once('@').map(|(p, _)| p).unwrap_or(key);
        out.enabled_plugins.insert(plugin.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn installed() -> Installed {
        let mut i = Installed::default();
        i.user.insert("vexp".into());
        i.user.insert("gitnexus".into());
        i.project.insert("gsd-workflow".into());
        i.project.insert("github".into());
        i.enabled_plugins.insert("playwright".into());
        i.enabled_plugins.insert("claude-mem".into());
        i
    }

    #[test]
    fn classifies_user_server() {
        assert_eq!(installed().classify("vexp"), ServerSource::UserConfig);
    }

    #[test]
    fn classifies_project_server() {
        assert_eq!(
            installed().classify("gsd-workflow"),
            ServerSource::ProjectConfig
        );
    }

    #[test]
    fn classifies_enabled_plugin() {
        // First underscore after "plugin_" splits plugin name from server.
        assert_eq!(
            installed().classify("plugin_playwright_playwright"),
            ServerSource::Plugin
        );
        assert_eq!(
            installed().classify("plugin_claude-mem_mcp-search"),
            ServerSource::Plugin
        );
    }

    #[test]
    fn disabled_plugin_is_historical() {
        // `context7` is not in enabled_plugins → historical even though it
        // matches the plugin_ prefix.
        assert_eq!(
            installed().classify("plugin_context7_context7"),
            ServerSource::Historical
        );
    }

    #[test]
    fn unknown_server_is_historical() {
        assert_eq!(
            installed().classify("Claude_in_Chrome"),
            ServerSource::Historical
        );
        assert_eq!(
            installed().classify("84d6c24c-d787-4f0a-9ca8-24cafad1b7bc"),
            ServerSource::Historical
        );
    }
}
