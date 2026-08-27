use serde::Deserialize;
use std::collections::HashMap;

/// Nombre de OS tal como lo usa Mojang en los version.json ("windows", "osx", "linux").
pub fn current_os_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "osx"
    } else {
        "linux"
    }
}

pub fn current_arch() -> &'static str {
    if cfg!(target_arch = "x86_64") {
        "x86_64"
    } else if cfg!(target_arch = "x86") {
        "x86"
    } else if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        "unknown"
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct OsRule {
    pub name: Option<String>,
    pub arch: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Rule {
    pub action: String,
    #[serde(default)]
    pub os: Option<OsRule>,
    #[serde(default)]
    pub features: Option<HashMap<String, bool>>,
}

/// Evalúa una lista de reglas al estilo del launcher vanilla: sin reglas =
/// permitido; si hay reglas, gana la última que matchee (mismo comportamiento
/// que describe minecraft.wiki para "rules" en el version manifest).
pub fn rules_allow(rules: &[Rule], active_features: &HashMap<String, bool>) -> bool {
    if rules.is_empty() {
        return true;
    }

    let mut allowed = false;
    for rule in rules {
        if rule_matches(rule, active_features) {
            allowed = rule.action == "allow";
        }
    }
    allowed
}

fn rule_matches(rule: &Rule, active_features: &HashMap<String, bool>) -> bool {
    if let Some(os) = &rule.os {
        if let Some(name) = &os.name {
            if name != current_os_name() {
                return false;
            }
        }
        if let Some(arch) = &os.arch {
            if arch != current_arch() {
                return false;
            }
        }
    }

    if let Some(features) = &rule.features {
        for (key, expected) in features {
            let actual = active_features.get(key).copied().unwrap_or(false);
            if actual != *expected {
                return false;
            }
        }
    }

    true
}
