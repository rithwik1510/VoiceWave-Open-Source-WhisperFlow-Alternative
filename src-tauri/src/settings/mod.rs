use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum DecodeMode {
    #[default]
    Balanced,
    Fast,
    Quality,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum FormatProfile {
    #[default]
    Default,
    Academic,
    Technical,
    Concise,
    CodeDoc,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "kebab-case")]
pub enum DomainPackId {
    Coding,
    Student,
    Productivity,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum AppTargetClass {
    #[default]
    Editor,
    Browser,
    Collab,
    Desktop,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase")]
pub struct AppProfileBehavior {
    pub punctuation_aggressiveness: u8,
    pub sentence_compactness: u8,
    pub auto_list_formatting: bool,
}

impl Default for AppProfileBehavior {
    fn default() -> Self {
        Self {
            punctuation_aggressiveness: 1,
            sentence_compactness: 1,
            auto_list_formatting: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase")]
pub struct AppProfileOverrides {
    pub active_target: AppTargetClass,
    pub editor: AppProfileBehavior,
    pub browser: AppProfileBehavior,
    pub collab: AppProfileBehavior,
    pub desktop: AppProfileBehavior,
}

impl Default for AppProfileOverrides {
    fn default() -> Self {
        Self {
            active_target: AppTargetClass::Editor,
            editor: AppProfileBehavior {
                punctuation_aggressiveness: 2,
                sentence_compactness: 1,
                auto_list_formatting: true,
            },
            browser: AppProfileBehavior {
                punctuation_aggressiveness: 1,
                sentence_compactness: 1,
                auto_list_formatting: false,
            },
            collab: AppProfileBehavior {
                punctuation_aggressiveness: 1,
                sentence_compactness: 2,
                auto_list_formatting: true,
            },
            desktop: AppProfileBehavior::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum CodeCasingStyle {
    #[default]
    Preserve,
    CamelCase,
    SnakeCase,
    PascalCase,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase")]
pub struct CodeModeSettings {
    pub enabled: bool,
    pub spoken_symbols: bool,
    pub preferred_casing: CodeCasingStyle,
    pub wrap_in_fenced_block: bool,
}

impl Default for CodeModeSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            spoken_symbols: true,
            preferred_casing: CodeCasingStyle::Preserve,
            wrap_in_fenced_block: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct VoiceWaveSettings {
    pub input_device: Option<String>,
    pub active_model: String,
    pub show_floating_hud: bool,
    pub vad_threshold: f32,
    pub max_utterance_ms: u64,
    pub release_tail_ms: u64,
    pub decode_mode: DecodeMode,
    pub diagnostics_opt_in: bool,
    pub toggle_hotkey: String,
    pub push_to_talk_hotkey: String,
    pub prefer_clipboard_fallback: bool,
    pub format_profile: FormatProfile,
    pub active_domain_packs: Vec<DomainPackId>,
    pub app_profile_overrides: AppProfileOverrides,
    pub code_mode: CodeModeSettings,
    pub pro_post_processing_enabled: bool,
}

impl Default for VoiceWaveSettings {
    fn default() -> Self {
        Self {
            input_device: None,
            active_model: "fw-small.en".to_string(),
            show_floating_hud: true,
            vad_threshold: 0.014,
            max_utterance_ms: 60_000,
            release_tail_ms: 350,
            decode_mode: DecodeMode::Balanced,
            diagnostics_opt_in: false,
            toggle_hotkey: LOCKED_TOGGLE_HOTKEY.to_string(),
            push_to_talk_hotkey: LOCKED_PUSH_TO_TALK_HOTKEY.to_string(),
            prefer_clipboard_fallback: false,
            format_profile: FormatProfile::Default,
            active_domain_packs: Vec::new(),
            app_profile_overrides: AppProfileOverrides::default(),
            code_mode: CodeModeSettings::default(),
            pro_post_processing_enabled: false,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SettingsError {
    #[error("failed to read settings file: {0}")]
    Read(std::io::Error),
    #[error("failed to write settings file: {0}")]
    Write(std::io::Error),
    #[error("failed to parse settings JSON: {0}")]
    Parse(serde_json::Error),
    #[error("cannot resolve app data directory")]
    AppData,
}

#[derive(Debug, Clone)]
pub struct SettingsStore {
    path: PathBuf,
}

impl SettingsStore {
    pub fn new() -> Result<Self, SettingsError> {
        let proj_dirs =
            ProjectDirs::from("com", "voicewave", "localcore").ok_or(SettingsError::AppData)?;
        let path = proj_dirs.config_dir().join("settings.json");
        Ok(Self { path })
    }

    pub fn from_path(path: impl AsRef<Path>) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
        }
    }

    pub fn load(&self) -> Result<VoiceWaveSettings, SettingsError> {
        if !self.path.exists() {
            return Ok(VoiceWaveSettings::default());
        }
        let raw = fs::read_to_string(&self.path).map_err(SettingsError::Read)?;
        let normalized = raw.trim_start_matches('\u{feff}').trim();
        if normalized.is_empty() {
            return Ok(VoiceWaveSettings::default());
        }
        let mut settings: VoiceWaveSettings =
            serde_json::from_str(normalized).map_err(SettingsError::Parse)?;
        settings.active_model = normalize_active_model_id(&settings.active_model);
        normalize_hotkey_bindings(&mut settings);
        normalize_pro_settings(&mut settings);
        Ok(settings)
    }

    pub fn save(&self, settings: &VoiceWaveSettings) -> Result<(), SettingsError> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(SettingsError::Write)?;
        }
        let raw = serde_json::to_string_pretty(settings).map_err(SettingsError::Parse)?;
        fs::write(&self.path, raw).map_err(SettingsError::Write)?;
        Ok(())
    }
}

fn normalize_active_model_id(active_model: &str) -> String {
    match active_model.trim() {
        "fw-small.en" | "fw-large-v3" => active_model.trim().to_string(),
        "tiny.en" | "base.en" | "small.en" | "medium.en" => "fw-small.en".to_string(),
        _ => "fw-small.en".to_string(),
    }
}

fn normalize_behavior(behavior: &mut AppProfileBehavior) {
    behavior.punctuation_aggressiveness = behavior.punctuation_aggressiveness.min(2);
    behavior.sentence_compactness = behavior.sentence_compactness.min(2);
}

pub fn normalize_pro_settings(settings: &mut VoiceWaveSettings) {
    normalize_behavior(&mut settings.app_profile_overrides.editor);
    normalize_behavior(&mut settings.app_profile_overrides.browser);
    normalize_behavior(&mut settings.app_profile_overrides.collab);
    normalize_behavior(&mut settings.app_profile_overrides.desktop);

    let mut seen = std::collections::HashSet::new();
    settings
        .active_domain_packs
        .retain(|pack| seen.insert(*pack));
}

pub const LOCKED_TOGGLE_HOTKEY: &str = "Ctrl+Alt+X";
pub const LOCKED_PUSH_TO_TALK_HOTKEY: &str = "Ctrl+Windows";

pub fn normalize_hotkey_bindings(settings: &mut VoiceWaveSettings) {
    settings.toggle_hotkey = LOCKED_TOGGLE_HOTKEY.to_string();
    settings.push_to_talk_hotkey = LOCKED_PUSH_TO_TALK_HOTKEY.to_string();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_settings_path() -> PathBuf {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be valid")
            .as_nanos();
        std::env::temp_dir().join(format!("voicewave-settings-{ts}.json"))
    }

    #[test]
    fn load_returns_default_if_missing() {
        let path = temp_settings_path();
        let store = SettingsStore::from_path(path);
        let loaded = store.load().expect("load should succeed");
        assert_eq!(loaded.active_model, "fw-small.en");
    }

    #[test]
    fn save_then_load_round_trip() {
        let path = temp_settings_path();
        let store = SettingsStore::from_path(path.clone());
        let settings = VoiceWaveSettings {
            active_model: "fw-large-v3".to_string(),
            vad_threshold: 0.025,
            max_utterance_ms: 22_000,
            release_tail_ms: 300,
            decode_mode: DecodeMode::Fast,
            diagnostics_opt_in: true,
            toggle_hotkey: "Ctrl+Alt+X".to_string(),
            push_to_talk_hotkey: "Ctrl+Windows".to_string(),
            prefer_clipboard_fallback: true,
            format_profile: FormatProfile::Technical,
            active_domain_packs: vec![DomainPackId::Coding, DomainPackId::Student],
            code_mode: CodeModeSettings {
                enabled: true,
                spoken_symbols: true,
                preferred_casing: CodeCasingStyle::SnakeCase,
                wrap_in_fenced_block: false,
            },
            pro_post_processing_enabled: true,
            ..VoiceWaveSettings::default()
        };

        store.save(&settings).expect("save should succeed");
        let loaded = store.load().expect("load should succeed");

        assert_eq!(loaded.active_model, "fw-large-v3");
        assert!((loaded.vad_threshold - 0.025).abs() < 1e-6);
        assert_eq!(loaded.max_utterance_ms, 22_000);
        assert_eq!(loaded.release_tail_ms, 300);
        assert_eq!(loaded.decode_mode, DecodeMode::Fast);
        assert!(loaded.diagnostics_opt_in);
        assert!(loaded.prefer_clipboard_fallback);
        assert_eq!(loaded.format_profile, FormatProfile::Technical);
        assert!(loaded.pro_post_processing_enabled);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn load_enforces_locked_hotkey_pair() {
        let path = temp_settings_path();
        let store = SettingsStore::from_path(path.clone());
        let raw = r#"{"toggleHotkey":"Ctrl+Shift+Space","pushToTalkHotkey":"Ctrl+Alt+Space"}"#;
        std::fs::write(&path, raw).expect("write should succeed");

        let loaded = store.load().expect("load should succeed");
        assert_eq!(loaded.toggle_hotkey, LOCKED_TOGGLE_HOTKEY);
        assert_eq!(loaded.push_to_talk_hotkey, LOCKED_PUSH_TO_TALK_HOTKEY);

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn load_accepts_utf8_bom_prefixed_json() {
        let path = temp_settings_path();
        let store = SettingsStore::from_path(path.clone());
        let raw = "\u{feff}{\"activeModel\":\"tiny.en\"}";
        std::fs::write(&path, raw).expect("write should succeed");

        let loaded = store.load().expect("load should succeed");
        assert_eq!(loaded.active_model, "fw-small.en");

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn load_returns_default_for_whitespace_only_json() {
        let path = temp_settings_path();
        let store = SettingsStore::from_path(path.clone());
        std::fs::write(&path, " \n\t  ").expect("write should succeed");

        let loaded = store.load().expect("load should succeed");
        assert_eq!(loaded.active_model, "fw-small.en");

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn normalize_pro_settings_clamps_behavior_and_deduplicates_domain_packs() {
        let mut settings = VoiceWaveSettings::default();
        settings
            .app_profile_overrides
            .editor
            .punctuation_aggressiveness = 9;
        settings.app_profile_overrides.browser.sentence_compactness = 7;
        settings.active_domain_packs = vec![
            DomainPackId::Coding,
            DomainPackId::Student,
            DomainPackId::Coding,
            DomainPackId::Productivity,
            DomainPackId::Student,
        ];

        normalize_pro_settings(&mut settings);

        assert_eq!(
            settings
                .app_profile_overrides
                .editor
                .punctuation_aggressiveness,
            2
        );
        assert_eq!(
            settings.app_profile_overrides.browser.sentence_compactness,
            2
        );
        assert_eq!(
            settings.active_domain_packs,
            vec![
                DomainPackId::Coding,
                DomainPackId::Student,
                DomainPackId::Productivity
            ]
        );
    }
}
