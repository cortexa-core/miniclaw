use serde::Deserialize;
use std::path::{Path, PathBuf};

/// A loaded, validated skill
pub struct Skill {
    pub name: String,
    #[allow(dead_code)] // used for display and future relevance matching
    pub description: String,
    #[allow(dead_code)] // used for future per-message relevance scoring
    pub tags: Vec<String>,
    pub priority: u32,
    pub always: bool,
    pub content: String,
    #[allow(dead_code)] // used for debugging and future hot-reload
    pub source_path: PathBuf,
}

/// YAML frontmatter parsed from SKILL.md files
#[derive(Deserialize)]
struct SkillFrontmatter {
    name: String,
    description: String,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default = "default_priority")]
    priority: u32,
    #[serde(default)]
    always: bool,
    #[serde(default)]
    requires: SkillRequires,
}

#[derive(Deserialize, Default)]
struct SkillRequires {
    #[serde(default)]
    tools: Vec<String>,
    #[serde(default)]
    env: Vec<String>,
}

fn default_priority() -> u32 { 50 }

/// Manages skill loading, gating, and selection
pub struct SkillManager {
    skills: Vec<Skill>,
}

impl SkillManager {
    /// Load skills from directory, apply gating against available tools and env vars
    pub fn load(skills_dir: &Path, available_tools: &[String]) -> Self {
        let mut skills = Vec::new();

        let entries = match std::fs::read_dir(skills_dir) {
            Ok(e) => e,
            Err(_) => return Self { skills },
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if !path.extension().is_some_and(|ext| ext == "md") {
                continue;
            }

            match Self::load_skill(&path, available_tools) {
                Ok(Some(skill)) => {
                    tracing::info!("Loaded skill: {} (priority={}, always={})",
                        skill.name, skill.priority, skill.always);
                    skills.push(skill);
                }
                Ok(None) => {
                    // Gated out — already logged
                }
                Err(e) => {
                    tracing::warn!("Failed to load skill {}: {e}", path.display());
                }
            }
        }

        // Sort: always first, then by priority descending
        skills.sort_by(|a, b| {
            b.always.cmp(&a.always)
                .then(b.priority.cmp(&a.priority))
        });

        tracing::info!("Loaded {} skills from {}", skills.len(), skills_dir.display());
        Self { skills }
    }

    /// Select skills to inject into the system prompt, respecting budget.
    /// Skills are scored dynamically based on relevance to the user's input.
    /// `always` skills are injected first regardless of input.
    pub fn select_for_prompt(&self, max_bytes: usize, user_input: &str) -> String {
        if self.skills.is_empty() {
            return String::new();
        }

        // Score each skill: always skills first, then by dynamic relevance
        let mut scored: Vec<(&Skill, f32)> = self
            .skills
            .iter()
            .map(|skill| {
                let score = if skill.always {
                    f32::MAX // always skills go first
                } else {
                    Self::relevance_score(skill, user_input)
                };
                (skill, score)
            })
            .filter(|(skill, score)| skill.always || *score > 0.0)
            .collect();

        // Sort by score descending
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        let mut parts = Vec::new();
        let mut remaining = max_bytes;

        // Reserve up to half the budget for `always` skills
        let always_budget = max_bytes / 2;
        let mut always_used = 0;

        for (skill, score) in &scored {
            let entry = format!("### {}\n\n{}", skill.name, skill.content);
            let entry_bytes = entry.len();

            if skill.always {
                if always_used + entry_bytes <= always_budget {
                    parts.push(entry);
                    always_used += entry_bytes;
                    remaining -= entry_bytes;
                }
            } else if entry_bytes <= remaining {
                tracing::debug!(
                    "Skill '{}' selected (relevance={:.1}, priority={})",
                    skill.name, score, skill.priority
                );
                parts.push(entry);
                remaining -= entry_bytes;
            }
        }

        if parts.is_empty() {
            return String::new();
        }

        parts.join("\n\n---\n\n")
    }

    /// Compute relevance score for a skill against user input.
    /// Combines tag matching (70% weight) with base priority (30% weight).
    /// Compute relevance score. Returns 0.0 if no tags or description words match,
    /// regardless of base priority. This ensures irrelevant skills are never injected.
    fn relevance_score(skill: &Skill, user_input: &str) -> f32 {
        let input_lower = user_input.to_lowercase();

        // Tag matching: how many skill tags appear in the user's message
        let tag_matches = if skill.tags.is_empty() {
            0
        } else {
            skill.tags
                .iter()
                .filter(|tag| input_lower.contains(&tag.to_lowercase()))
                .count()
        };

        // Description word matching: keywords from description in input
        let desc_matches = skill
            .description
            .split_whitespace()
            .filter(|word| word.len() > 3)
            .filter(|word| input_lower.contains(&word.to_lowercase()))
            .count();

        // No matches at all → score 0, skill won't be selected
        if tag_matches == 0 && desc_matches == 0 {
            return 0.0;
        }

        let tag_score = if skill.tags.is_empty() {
            0.0
        } else {
            (tag_matches as f32 / skill.tags.len() as f32) * 100.0
        };
        let desc_score = desc_matches as f32 * 10.0;
        let relevance = tag_score + desc_score;
        let base = skill.priority as f32;

        // Weighted: relevance matters more, base priority breaks ties
        relevance * 0.7 + base * 0.3
    }

    /// Get the number of loaded skills
    #[allow(dead_code)]
    pub fn count(&self) -> usize {
        self.skills.len()
    }

    /// Get skill names for display
    #[allow(dead_code)]
    pub fn names(&self) -> Vec<&str> {
        self.skills.iter().map(|s| s.name.as_str()).collect()
    }

    fn load_skill(path: &Path, available_tools: &[String]) -> anyhow::Result<Option<Skill>> {
        let raw = std::fs::read_to_string(path)?;

        // Parse YAML frontmatter (between --- delimiters)
        let (frontmatter, body) = Self::parse_frontmatter(&raw)?;
        let meta: SkillFrontmatter = serde_yaml_frontmatter(&frontmatter)?;

        // Gate: check required tools
        for tool in &meta.requires.tools {
            if !available_tools.iter().any(|t| t == tool) {
                tracing::info!("Skill '{}' gated: requires tool '{}' which is not available",
                    meta.name, tool);
                return Ok(None);
            }
        }

        // Gate: check required env vars
        for env_var in &meta.requires.env {
            if std::env::var(env_var).is_err() {
                tracing::info!("Skill '{}' gated: requires env var '{}' which is not set",
                    meta.name, env_var);
                return Ok(None);
            }
        }

        let content = body.trim().to_string();
        if content.is_empty() {
            tracing::warn!("Skill '{}' has empty content body, skipping", meta.name);
            return Ok(None);
        }

        Ok(Some(Skill {
            name: meta.name,
            description: meta.description,
            tags: meta.tags,
            priority: meta.priority.min(100),
            always: meta.always,
            content,
            source_path: path.to_path_buf(),
        }))
    }

    /// Split a markdown file into YAML frontmatter and body
    fn parse_frontmatter(raw: &str) -> anyhow::Result<(String, String)> {
        let trimmed = raw.trim_start();
        if !trimmed.starts_with("---") {
            // No frontmatter — treat entire file as body with auto-generated metadata
            return Err(anyhow::anyhow!("No YAML frontmatter found (must start with ---)"));
        }

        let after_first = &trimmed[3..];
        let end = after_first.find("\n---")
            .ok_or_else(|| anyhow::anyhow!("No closing --- for frontmatter"))?;

        let frontmatter = after_first[..end].trim().to_string();
        let body = after_first[end + 4..].to_string();

        Ok((frontmatter, body))
    }
}

/// Parse YAML-like frontmatter into SkillFrontmatter.
/// We convert simple YAML to TOML since we already depend on the toml crate.
fn serde_yaml_frontmatter(frontmatter: &str) -> anyhow::Result<SkillFrontmatter> {
    let mut toml_lines = Vec::new();
    let mut in_requires = false;

    for line in frontmatter.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        // Detect "requires:" section header
        if trimmed == "requires:" {
            in_requires = true;
            toml_lines.push("[requires]".to_string());
            continue;
        }

        // Indented lines under "requires:" are its fields
        let is_indented = line.starts_with("  ") || line.starts_with('\t');
        if !is_indented && in_requires {
            in_requires = false;
        }

        if let Some((key, value)) = trimmed.split_once(':') {
            let key = key.trim();
            let value = value.trim();
            if value.is_empty() {
                continue;
            }
            let toml_value = yaml_value_to_toml(value);
            toml_lines.push(format!("{key} = {toml_value}"));
        }
    }

    let toml_str = toml_lines.join("\n");
    let parsed: SkillFrontmatter = toml::from_str(&toml_str)
        .map_err(|e| anyhow::anyhow!("Failed to parse skill frontmatter: {e}\nConverted TOML:\n{toml_str}"))?;
    Ok(parsed)
}

/// Convert a YAML value to TOML format
fn yaml_value_to_toml(value: &str) -> String {
    // Boolean
    if value == "true" || value == "false" {
        return value.to_string();
    }
    // Integer
    if value.parse::<u64>().is_ok() {
        return value.to_string();
    }
    // Array: [a, b, c] → ["a", "b", "c"] (quote unquoted elements)
    if value.starts_with('[') && value.ends_with(']') {
        let inner = &value[1..value.len() - 1];
        let elements: Vec<String> = inner
            .split(',')
            .map(|e| {
                let e = e.trim();
                if (e.starts_with('"') && e.ends_with('"'))
                    || (e.starts_with('\'') && e.ends_with('\''))
                {
                    // Already quoted — normalize to double quotes
                    format!("\"{}\"", e.trim_matches('"').trim_matches('\''))
                } else if e.parse::<u64>().is_ok() || e == "true" || e == "false" {
                    e.to_string()
                } else {
                    format!("\"{e}\"")
                }
            })
            .collect();
        return format!("[{}]", elements.join(", "));
    }
    // String — quote it
    let unquoted = value.trim_matches('"').trim_matches('\'');
    format!("\"{unquoted}\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_frontmatter() {
        let raw = r#"---
name: test-skill
description: A test skill
tags: [test, demo]
priority: 70
always: false
---

## Test Content

This is the skill body."#;

        let (fm, body) = SkillManager::parse_frontmatter(raw).unwrap();
        assert!(fm.contains("name: test-skill"));
        assert!(body.contains("Test Content"));
        assert!(body.contains("skill body"));
    }

    #[test]
    fn test_parse_frontmatter_no_yaml() {
        let raw = "Just some markdown without frontmatter";
        assert!(SkillManager::parse_frontmatter(raw).is_err());
    }

    #[test]
    fn test_serde_yaml_frontmatter_simple() {
        let fm = r#"name: my-skill
description: Does things
tags: [foo, bar]
priority: 80
always: true"#;

        let parsed = serde_yaml_frontmatter(fm).unwrap();
        assert_eq!(parsed.name, "my-skill");
        assert_eq!(parsed.description, "Does things");
        assert_eq!(parsed.tags, vec!["foo", "bar"]);
        assert_eq!(parsed.priority, 80);
        assert!(parsed.always);
    }

    #[test]
    fn test_serde_yaml_frontmatter_with_requires() {
        let fm = r#"name: smart-home
description: Control devices
requires:
  tools: [shell_exec, system_info]
  env: [HUE_IP]"#;

        let parsed = serde_yaml_frontmatter(fm).unwrap();
        assert_eq!(parsed.name, "smart-home");
        assert_eq!(parsed.requires.tools, vec!["shell_exec", "system_info"]);
        assert_eq!(parsed.requires.env, vec!["HUE_IP"]);
    }

    #[test]
    fn test_skill_loading_and_gating() {
        let dir = tempfile::tempdir().unwrap();

        // Write a skill that should load
        std::fs::write(dir.path().join("valid.md"), r#"---
name: valid-skill
description: A valid skill
priority: 70
---

## Instructions

Do something useful."#).unwrap();

        // Write a skill that requires a missing tool
        std::fs::write(dir.path().join("gated.md"), r#"---
name: gated-skill
description: Needs special tool
requires:
  tools: [nonexistent_tool]
---

## Instructions

Won't load."#).unwrap();

        let available_tools = vec!["get_time".to_string(), "read_file".to_string()];
        let mgr = SkillManager::load(dir.path(), &available_tools);

        assert_eq!(mgr.count(), 1);
        assert_eq!(mgr.names(), vec!["valid-skill"]);
    }

    #[test]
    fn test_skill_budget_selection() {
        let mgr = SkillManager {
            skills: vec![
                Skill {
                    name: "always-skill".into(),
                    description: "".into(),
                    tags: vec![],
                    priority: 90,
                    always: true,
                    content: "Always active instructions.".into(),
                    source_path: PathBuf::new(),
                },
                Skill {
                    name: "high-priority".into(),
                    description: "Relevant testing skill".into(),
                    tags: vec!["test".into(), "query".into()],
                    priority: 80,
                    always: false,
                    content: "High priority content here.".into(),
                    source_path: PathBuf::new(),
                },
                Skill {
                    name: "too-big".into(),
                    description: "Also relevant but huge".into(),
                    tags: vec!["test".into()],
                    priority: 70,
                    always: false,
                    content: "x".repeat(5000),
                    source_path: PathBuf::new(),
                },
            ],
        };

        let result = mgr.select_for_prompt(500, "test query");
        assert!(result.contains("always-skill"));
        assert!(result.contains("high-priority"));
        assert!(!result.contains("too-big"));
    }

    #[test]
    fn test_skill_sort_order() {
        let dir = tempfile::tempdir().unwrap();

        std::fs::write(dir.path().join("low.md"), r#"---
name: low-priority
description: Low
priority: 10
---
Low content."#).unwrap();

        std::fs::write(dir.path().join("high.md"), r#"---
name: high-priority
description: High
priority: 90
---
High content."#).unwrap();

        std::fs::write(dir.path().join("always.md"), r#"---
name: always-on
description: Always
priority: 50
always: true
---
Always content."#).unwrap();

        let mgr = SkillManager::load(dir.path(), &[]);

        // Order should be: always first, then high, then low
        assert_eq!(mgr.skills[0].name, "always-on");
        assert_eq!(mgr.skills[1].name, "high-priority");
        assert_eq!(mgr.skills[2].name, "low-priority");
    }

    #[test]
    fn test_relevance_scoring() {
        let skill = Skill {
            name: "garden".into(),
            description: "Monitor garden sensors".into(),
            tags: vec!["garden".into(), "plants".into(), "soil".into(), "moisture".into()],
            priority: 50,
            always: false,
            content: "Garden instructions.".into(),
            source_path: PathBuf::new(),
        };

        // Relevant input — matches tags
        let score = SkillManager::relevance_score(&skill, "how are my plants doing?");
        assert!(score > 0.0, "Should match 'plants' tag, got {score}");

        // Very relevant — multiple tag matches
        let score2 = SkillManager::relevance_score(&skill, "check soil moisture in the garden");
        assert!(score2 > score, "More matches should score higher: {score2} vs {score}");

        // Irrelevant input — no tag matches → score 0
        let score3 = SkillManager::relevance_score(&skill, "what time is it?");
        assert_eq!(score3, 0.0, "No tag matches should return 0: {score3}");
    }

    #[test]
    fn test_dynamic_selection_by_input() {
        let mgr = SkillManager {
            skills: vec![
                Skill {
                    name: "garden".into(),
                    description: "Garden monitoring".into(),
                    tags: vec!["garden".into(), "plants".into()],
                    priority: 50,
                    always: false,
                    content: "Water the plants when soil is dry.".into(),
                    source_path: PathBuf::new(),
                },
                Skill {
                    name: "system".into(),
                    description: "System health monitoring".into(),
                    tags: vec!["system".into(), "cpu".into(), "disk".into()],
                    priority: 50,
                    always: false,
                    content: "Check disk space and CPU usage.".into(),
                    source_path: PathBuf::new(),
                },
            ],
        };

        // Garden query → garden skill selected, not system
        let result = mgr.select_for_prompt(2000, "how are my plants?");
        assert!(result.contains("garden"), "Should contain garden skill");
        assert!(!result.contains("system"), "Should NOT contain system skill for plant query");

        // System query → system skill selected, not garden
        let result = mgr.select_for_prompt(2000, "check disk space");
        assert!(result.contains("system"), "Should contain system skill");
        assert!(!result.contains("garden"), "Should NOT contain garden skill for disk query");
    }

    #[test]
    fn test_always_skills_always_included() {
        let mgr = SkillManager {
            skills: vec![
                Skill {
                    name: "always-on".into(),
                    description: "Core behavior".into(),
                    tags: vec![],
                    priority: 90,
                    always: true,
                    content: "Always do this.".into(),
                    source_path: PathBuf::new(),
                },
                Skill {
                    name: "niche-skill".into(),
                    description: "Specific domain".into(),
                    tags: vec!["quantum".into(), "physics".into()],
                    priority: 50,
                    always: false,
                    content: "Quantum physics instructions.".into(),
                    source_path: PathBuf::new(),
                },
            ],
        };

        let result = mgr.select_for_prompt(2000, "completely unrelated query");
        assert!(result.contains("always-on"), "Always skills must be included");
        assert!(!result.contains("niche-skill"), "Non-matching skills excluded");
    }

    #[test]
    fn test_env_gating() {
        let dir = tempfile::tempdir().unwrap();

        std::fs::write(dir.path().join("needs-env.md"), r#"---
name: env-skill
description: Needs env
requires:
  env: [MINICLAW_TEST_SKILL_ENV_12345]
---
Content."#).unwrap();

        // Without env var — gated out
        let mgr = SkillManager::load(dir.path(), &[]);
        assert_eq!(mgr.count(), 0);

        // With env var — loads
        std::env::set_var("MINICLAW_TEST_SKILL_ENV_12345", "1");
        let mgr = SkillManager::load(dir.path(), &[]);
        assert_eq!(mgr.count(), 1);
        std::env::remove_var("MINICLAW_TEST_SKILL_ENV_12345");
    }
}
