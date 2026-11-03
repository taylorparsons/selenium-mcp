use crate::error::AppError;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use thirtyfour::By;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Target {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub css: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub xpath: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub semantic: Option<String>,
}

// Modeled as a struct-of-optionals (same idiom as Target's css/xpath), not an
// enum: FlowStep is internally tagged (`tag = "action"`), which makes
// serde_yaml buffer each step as `Content` before matching the tag. Any enum
// nested inside that buffered Content fails to deserialize — "untagged and
// internally tagged enums do not support enum input" — regardless of the
// nested enum's own tagging mode. A struct sidesteps the bug entirely while
// producing the identical one-key-map wire shape design.md specifies
// (`that: { text_visible: "Welcome" }`).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(deny_unknown_fields)]
pub struct AssertCondition {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub element_visible: Option<Target>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_visible: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url_matches: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title_matches: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub element_count: Option<ElementCountCondition>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ElementCountCondition {
    pub target: Target,
    pub count: usize,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MaskRect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

pub enum ResolvedCondition<'a> {
    ElementVisible(&'a Target),
    TextVisible(&'a str),
    UrlMatches(&'a str),
    TitleMatches(&'a str),
    ElementCount { target: &'a Target, count: usize },
}

impl AssertCondition {
    pub fn resolve(&self) -> Result<ResolvedCondition<'_>, AppError> {
        let mut matches = Vec::new();
        if let Some(t) = &self.element_visible {
            matches.push(ResolvedCondition::ElementVisible(t));
        }
        if let Some(t) = &self.text_visible {
            matches.push(ResolvedCondition::TextVisible(t));
        }
        if let Some(t) = &self.url_matches {
            matches.push(ResolvedCondition::UrlMatches(t));
        }
        if let Some(t) = &self.title_matches {
            matches.push(ResolvedCondition::TitleMatches(t));
        }
        if let Some(ec) = &self.element_count {
            matches.push(ResolvedCondition::ElementCount {
                target: &ec.target,
                count: ec.count,
            });
        }
        match matches.len() {
            1 => Ok(matches.into_iter().next().unwrap()),
            0 => Err(AppError::InvalidTarget(
                "assert_that condition must set exactly one of element_visible/text_visible/url_matches/title_matches/element_count".to_string(),
            )),
            _ => Err(AppError::InvalidTarget(
                "assert_that condition must set exactly one condition, not multiple".to_string(),
            )),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum FlowStep {
    Navigate {
        url: String,
    },
    Click {
        target: Target,
    },
    TypeText {
        target: Target,
        text: String,
        #[serde(default)]
        clear_first: bool,
        #[serde(default)]
        secret: bool,
    },
    AssertThat {
        that: AssertCondition,
    },
    AssertVisual {
        baseline: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        threshold: Option<f64>,
        #[serde(default)]
        masks: Vec<MaskRect>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Flow {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(default)]
    pub env: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_epoch: Option<u64>,
    /// Overrides run_flow's default fixed window size (design.md §11.2:
    /// visual steps require a fixed viewport "from flow or project
    /// config"). Both unset means the default; set only one and the other
    /// keeps its default rather than resizing to some unrequested value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub viewport_width: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub viewport_height: Option<u32>,
    /// Persistent Chrome profile name (same semantics as
    /// SessionStartArgs::profile). Not exposed via flow_save — set by
    /// hand-editing the saved YAML, same as viewport_width/viewport_height.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    pub steps: Vec<FlowStep>,
}

impl Target {
    pub fn to_by(&self) -> Result<By, AppError> {
        match (&self.css, &self.xpath) {
            (Some(css), None) => Ok(By::Css(css.clone())),
            (None, Some(xpath)) => Ok(By::XPath(xpath.clone())),
            (None, None) => Err(AppError::InvalidTarget(
                "target needs css or xpath".to_string(),
            )),
            (Some(_), Some(_)) => Err(AppError::InvalidTarget(
                "target needs exactly one of css or xpath, not both".to_string(),
            )),
        }
    }
}

pub fn interpolate(
    template: &str,
    base_url: Option<&str>,
    env: &HashMap<String, String>,
) -> Result<String, AppError> {
    let mut result = template.to_string();

    if result.contains("{base_url}") {
        let base_url = base_url.ok_or_else(|| {
            AppError::InvalidTarget("{base_url} used but no base_url is set".to_string())
        })?;
        result = result.replace("{base_url}", base_url);
    }

    while let Some(start) = result.find("{env.") {
        let end = result[start..]
            .find('}')
            .map(|i| start + i)
            .ok_or_else(|| AppError::InvalidTarget("unterminated {env.*} token".to_string()))?;
        let name = &result[start + 5..end];
        let value = env.get(name).ok_or_else(|| {
            AppError::InvalidTarget(format!("required env var {name} was not provided"))
        })?;
        result.replace_range(start..=end, value);
    }

    Ok(result)
}

pub fn mask_if_secret(value: &str, secret: bool) -> String {
    if secret {
        "***".to_string()
    } else {
        value.to_string()
    }
}

pub fn load_flow(path: &Path) -> Result<Flow, AppError> {
    let contents = std::fs::read_to_string(path)?;
    let flow = serde_yaml::from_str(&contents).map_err(|_| {
        AppError::InvalidTarget("failed to parse flow YAML; check the flow schema (contents omitted to protect secrets)".into())
    })?;
    validate_flow(&flow)?;
    Ok(flow)
}

pub fn save_flow(path: &Path, flow: &Flow) -> Result<(), AppError> {
    validate_flow(flow)?;
    let yaml = serde_yaml::to_string(flow).map_err(|e| {
        AppError::InvalidTarget(format!("failed to serialize flow {}: {e}", flow.name))
    })?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, yaml)?;
    Ok(())
}

pub fn validate_flow(flow: &Flow) -> Result<(), AppError> {
    crate::project::validate_managed_name(&flow.name)?;
    if let Some(profile) = &flow.profile {
        crate::project::validate_profile_name(profile)?;
    }
    for step in &flow.steps {
        if let FlowStep::TypeText {
            text, secret: true, ..
        } = step
        {
            let name = secret_env_name(text).ok_or_else(|| AppError::InvalidTarget(
                "secret typing steps must use a single {env.NAME} reference, never literal text".into()
            ))?;
            if !flow.env.iter().any(|n| n == name) {
                return Err(AppError::InvalidTarget(
                    "secret environment references must be declared in the flow env list".into(),
                ));
            }
        }
        if let FlowStep::AssertVisual { baseline, .. } = step {
            crate::project::validate_managed_name(baseline)?;
        }
    }
    Ok(())
}

pub fn valid_env_name(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

pub fn secret_env_name(text: &str) -> Option<&str> {
    let name = text.strip_prefix("{env.")?.strip_suffix('}')?;
    valid_env_name(name).then_some(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_path(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("selenium-mcp-flow-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        dir.join(name)
    }

    #[test]
    fn interpolate_substitutes_base_url() {
        let result = interpolate(
            "{base_url}/login",
            Some("http://localhost:3000"),
            &HashMap::new(),
        )
        .unwrap();
        assert_eq!(result, "http://localhost:3000/login");
    }

    #[test]
    fn interpolate_substitutes_env_vars() {
        let mut env = HashMap::new();
        env.insert("TEST_EMAIL".to_string(), "a@example.com".to_string());
        let result = interpolate("{env.TEST_EMAIL}", None, &env).unwrap();
        assert_eq!(result, "a@example.com");
    }

    #[test]
    fn interpolate_fails_when_base_url_is_used_but_not_set() {
        let result = interpolate("{base_url}/login", None, &HashMap::new());
        assert!(result.is_err());
    }

    #[test]
    fn interpolate_fails_when_env_var_is_missing() {
        let result = interpolate("{env.MISSING}", None, &HashMap::new());
        assert!(result.is_err());
    }

    #[test]
    fn mask_if_secret_masks_when_true() {
        assert_eq!(mask_if_secret("hunter2", true), "***");
    }

    #[test]
    fn mask_if_secret_passes_through_when_false() {
        assert_eq!(mask_if_secret("hunter2", false), "hunter2");
    }

    #[test]
    fn flow_round_trips_through_yaml() {
        let flow = Flow {
            name: "login-happy-path".to_string(),
            description: "Log in and verify the dashboard loads".to_string(),
            tags: vec!["smoke".to_string(), "regression".to_string()],
            base_url: Some("http://localhost:3000".to_string()),
            env: vec!["TEST_EMAIL".to_string(), "TEST_PASSWORD".to_string()],
            created_epoch: Some(1_700_000_000),
            viewport_width: None,
            viewport_height: None,
            profile: None,
            steps: vec![
                FlowStep::Navigate {
                    url: "{base_url}/login".to_string(),
                },
                FlowStep::TypeText {
                    target: Target {
                        css: Some("#email".to_string()),
                        xpath: None,
                        semantic: Some("email input field".to_string()),
                    },
                    text: "{env.TEST_EMAIL}".to_string(),
                    clear_first: false,
                    secret: false,
                },
                FlowStep::TypeText {
                    target: Target {
                        css: Some("#password".to_string()),
                        xpath: None,
                        semantic: None,
                    },
                    text: "{env.TEST_PASSWORD}".to_string(),
                    clear_first: false,
                    secret: true,
                },
                FlowStep::Click {
                    target: Target {
                        css: Some("button[type=submit]".to_string()),
                        xpath: None,
                        semantic: None,
                    },
                },
                FlowStep::AssertThat {
                    that: AssertCondition {
                        text_visible: Some("Welcome".to_string()),
                        ..Default::default()
                    },
                },
            ],
        };

        let path = temp_path("login-happy-path.yaml");
        save_flow(&path, &flow).unwrap();
        let loaded = load_flow(&path).unwrap();

        assert_eq!(loaded.name, flow.name);
        assert_eq!(loaded.steps.len(), 5);
        assert!(matches!(loaded.steps[0], FlowStep::Navigate { .. }));
    }

    #[test]
    fn flow_round_trips_an_assert_visual_step_through_yaml() {
        let flow = Flow {
            name: "visual-check".to_string(),
            description: String::new(),
            tags: vec![],
            base_url: None,
            env: vec![],
            created_epoch: None,
            viewport_width: None,
            viewport_height: None,
            profile: None,
            steps: vec![FlowStep::AssertVisual {
                baseline: "dashboard-after-login".to_string(),
                threshold: Some(0.001),
                masks: vec![MaskRect {
                    x: 900,
                    y: 12,
                    w: 120,
                    h: 24,
                }],
            }],
        };

        let path = temp_path("visual-check.yaml");
        save_flow(&path, &flow).unwrap();
        let loaded = load_flow(&path).unwrap();

        match &loaded.steps[0] {
            FlowStep::AssertVisual {
                baseline,
                threshold,
                masks,
            } => {
                assert_eq!(baseline, "dashboard-after-login");
                assert_eq!(*threshold, Some(0.001));
                assert_eq!(masks.len(), 1);
                assert_eq!(masks[0].x, 900);
            }
            other => panic!("expected AssertVisual, got {other:?}"),
        }
    }

    #[test]
    fn load_flow_rejects_unknown_fields() {
        let path = temp_path("unknown-field.yaml");
        fs::write(
            &path,
            "name: test\nsteps: []\nthis_field_does_not_exist: true\n",
        )
        .unwrap();

        let result = load_flow(&path);

        assert!(result.is_err());
    }
}
