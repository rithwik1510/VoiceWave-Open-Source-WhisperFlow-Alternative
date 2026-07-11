//! DictationProfile module (plan 010, Phase 1).
//!
//! `polish_profile` is the persisted, single authority for how a dictation is
//! shaped. The deterministic bundle fields (`format_profile`,
//! `active_domain_packs`, `code_mode`, `pro_post_processing_enabled`) become
//! *derived defaults with tracked user overrides*: selecting a profile stamps
//! the bundle in ONE atomic settings write (fixing the old five-sequential-
//! writes torn-config risk), editing any derived field afterwards marks the
//! profile `customized`, and reselecting the profile resets the overrides.
//!
//! Migration: a settings file WITHOUT `polishProfile` is a legacy config. We
//! derive the closest profile from the old FormatProfile x DomainPack bundle
//! and keep the user's existing field values untouched (as overrides), so
//! day-one output is byte-identical to before the migration.

use super::{
    CodeCasingStyle, CodeModeSettings, DomainPackId, FormatProfile, VoiceWaveSettings,
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// The five polish profiles (plan 010). Serialized as the fixed interface
/// strings `"standard" | "coding" | "writing" | "casual" | "literal"` shared
/// with the React frontend and the Python polish worker.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, Default)]
#[serde(rename_all = "camelCase")]
pub enum PolishProfile {
    #[default]
    Standard,
    Coding,
    Writing,
    Casual,
    Literal,
}

impl PolishProfile {
    /// The wire string for this profile — identical for settings JSON, the
    /// worker IPC `profile` field, and history's `selectedProfile`.
    pub fn as_str(self) -> &'static str {
        match self {
            PolishProfile::Standard => "standard",
            PolishProfile::Coding => "coding",
            PolishProfile::Writing => "writing",
            PolishProfile::Casual => "casual",
            PolishProfile::Literal => "literal",
        }
    }

    /// Strict parse of the wire string. Returns `None` for anything outside
    /// the fixed contract set so `set_dictation_profile` can reject it.
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim() {
            "standard" => Some(PolishProfile::Standard),
            "coding" => Some(PolishProfile::Coding),
            "writing" => Some(PolishProfile::Writing),
            "casual" => Some(PolishProfile::Casual),
            "literal" => Some(PolishProfile::Literal),
            _ => None,
        }
    }
}

/// How the final text reaches the target app for a profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InsertPath {
    /// Deterministic text inserts immediately; LLM polish (if enabled) runs
    /// async afterwards as a pill Copy-offer. Standard's shipped contract.
    Immediate,
    /// Release waits (bounded) for a single validated LLM pass; on accept the
    /// polished text inserts, otherwise the deterministic floor does.
    WaitValidated,
    /// Deterministic only, branched BEFORE `finalize_pro_transcript`: the
    /// sanitized ASR baseline plus spoken commands, dictionary stabilization,
    /// and punctuation/capitalization. Never calls the model.
    LiteralImmediate,
}

/// The deterministic default bundle a profile stamps onto settings. These are
/// the values `set_dictation_profile` writes and the baseline that
/// `customized` detection compares against — the runtime always reads the
/// (possibly overridden) settings fields themselves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileBundle {
    pub format_profile: FormatProfile,
    pub domain_packs: Vec<DomainPackId>,
    pub code_mode: CodeModeSettings,
    pub pro_post_processing_enabled: bool,
}

/// Resolved, single-source policy for one profile against the current
/// settings. React renders this; the dictation flow branches on it. Neither
/// reconstructs the mapping independently.
#[derive(Debug, Clone)]
pub struct EffectivePolicy {
    pub profile: PolishProfile,
    /// LLM prompt id sent to the polish worker (`profile` IPC field).
    pub prompt_id: &'static str,
    pub insert_path: InsertPath,
    /// True when any derived bundle field differs from the profile defaults.
    pub customized: bool,
}

/// Deterministic default bundle per profile (plan 010 table).
pub fn profile_defaults(profile: PolishProfile) -> ProfileBundle {
    match profile {
        // Standard and Literal both carry the shipped default pipeline;
        // Literal's bundle is unused at runtime (it branches pre-finalize)
        // but keeping it at defaults makes switching away predictable.
        PolishProfile::Standard | PolishProfile::Literal => ProfileBundle {
            format_profile: FormatProfile::Default,
            domain_packs: Vec::new(),
            code_mode: CodeModeSettings::default(),
            pro_post_processing_enabled: true,
        },
        // Mirrors the old ProToolsMode "coding" preset so legacy coding
        // configs migrate to Coding with customized == false.
        PolishProfile::Coding => ProfileBundle {
            format_profile: FormatProfile::CodeDoc,
            domain_packs: vec![DomainPackId::Coding],
            code_mode: CodeModeSettings {
                enabled: true,
                spoken_symbols: true,
                preferred_casing: CodeCasingStyle::CamelCase,
                wrap_in_fenced_block: false,
            },
            pro_post_processing_enabled: true,
        },
        // Mirrors the old "writing" preset (Academic bundle).
        PolishProfile::Writing => ProfileBundle {
            format_profile: FormatProfile::Academic,
            domain_packs: vec![DomainPackId::Productivity],
            code_mode: CodeModeSettings::default(),
            pro_post_processing_enabled: true,
        },
        // Concise-ish bundle, light touch: no packs, no auto-structure.
        PolishProfile::Casual => ProfileBundle {
            format_profile: FormatProfile::Concise,
            domain_packs: Vec::new(),
            code_mode: CodeModeSettings::default(),
            pro_post_processing_enabled: true,
        },
    }
}

/// Resolve the runtime policy for `profile` against the current settings.
pub fn resolve_profile(profile: PolishProfile, settings: &VoiceWaveSettings) -> EffectivePolicy {
    EffectivePolicy {
        profile,
        prompt_id: profile.as_str(),
        insert_path: match profile {
            PolishProfile::Standard => InsertPath::Immediate,
            PolishProfile::Coding | PolishProfile::Writing | PolishProfile::Casual => {
                InsertPath::WaitValidated
            }
            PolishProfile::Literal => InsertPath::LiteralImmediate,
        },
        customized: is_customized(profile, settings),
    }
}

/// True when any derived bundle field differs from `profile`'s defaults.
/// Domain packs compare as a set (order and duplicates are presentation
/// noise, not customization).
pub fn is_customized(profile: PolishProfile, settings: &VoiceWaveSettings) -> bool {
    let defaults = profile_defaults(profile);
    let current_packs: HashSet<DomainPackId> =
        settings.active_domain_packs.iter().copied().collect();
    let default_packs: HashSet<DomainPackId> = defaults.domain_packs.iter().copied().collect();
    settings.format_profile != defaults.format_profile
        || current_packs != default_packs
        || settings.code_mode != defaults.code_mode
        || settings.pro_post_processing_enabled != defaults.pro_post_processing_enabled
}

/// Stamp `profile` and its default bundle onto `settings` (resetting any
/// overrides). The caller persists with ONE settings write.
pub fn apply_profile_defaults(profile: PolishProfile, settings: &mut VoiceWaveSettings) {
    let defaults = profile_defaults(profile);
    settings.polish_profile = Some(profile);
    settings.format_profile = defaults.format_profile;
    settings.active_domain_packs = defaults.domain_packs;
    settings.code_mode = defaults.code_mode;
    settings.pro_post_processing_enabled = defaults.pro_post_processing_enabled;
    settings.polish_profile_customized = false;
}

/// Derive the closest profile for a legacy config (no persisted
/// `polishProfile`). Mirrors the old frontend `detectProToolsMode` inference.
/// Concise-lineage bundles (old "study", bare Concise) land on Writing, not
/// Casual: Casual was cut from the v1 selectable lineup after failing the
/// plan-010 distinctness gate (writing<->casual 50-59% near-identical), and
/// the deterministic fields are preserved as overrides either way, so
/// day-one output is unchanged — only the (previously nonexistent) LLM tier
/// differs, and a migration must never land users on an unselectable card.
fn derive_legacy_profile(settings: &VoiceWaveSettings) -> PolishProfile {
    if settings.code_mode.enabled
        || settings.format_profile == FormatProfile::CodeDoc
        || settings.active_domain_packs.contains(&DomainPackId::Coding)
    {
        return PolishProfile::Coding;
    }
    // Old "study" preset lineage (Concise + Student pack). Lands as
    // "Writing - Customized" with the study bundle preserved as overrides.
    if settings
        .active_domain_packs
        .contains(&DomainPackId::Student)
    {
        return PolishProfile::Writing;
    }
    if settings.format_profile == FormatProfile::Academic
        || settings
            .active_domain_packs
            .contains(&DomainPackId::Productivity)
    {
        return PolishProfile::Writing;
    }
    if settings.format_profile == FormatProfile::Concise {
        return PolishProfile::Writing;
    }
    PolishProfile::Standard
}

/// Normalize the profile state on every settings load/update:
/// - legacy configs (no `polishProfile`) get the derived closest profile
///   while keeping their existing field values as overrides;
/// - the `customized` flag is recomputed so it can never go stale.
pub fn migrate_and_sync_polish_profile(settings: &mut VoiceWaveSettings) {
    if settings.polish_profile.is_none() {
        settings.polish_profile = Some(derive_legacy_profile(settings));
    }
    let profile = settings.polish_profile.unwrap_or_default();
    settings.polish_profile_customized = is_customized(profile, settings);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{AppTargetClass, SettingsStore};

    fn legacy(settings_mutator: impl FnOnce(&mut VoiceWaveSettings)) -> VoiceWaveSettings {
        let mut settings = VoiceWaveSettings {
            polish_profile: None,
            ..VoiceWaveSettings::default()
        };
        settings_mutator(&mut settings);
        settings
    }

    /// Old ProToolsMode "default" preset (App.tsx buildProToolsPreset).
    fn legacy_default_preset() -> VoiceWaveSettings {
        legacy(|s| {
            s.format_profile = FormatProfile::Default;
            s.active_domain_packs = Vec::new();
            s.code_mode = CodeModeSettings::default();
            s.pro_post_processing_enabled = true;
            s.app_profile_overrides.active_target = AppTargetClass::Desktop;
        })
    }

    /// Old ProToolsMode "coding" preset.
    fn legacy_coding_preset() -> VoiceWaveSettings {
        legacy(|s| {
            s.format_profile = FormatProfile::CodeDoc;
            s.active_domain_packs = vec![DomainPackId::Coding];
            s.code_mode = CodeModeSettings {
                enabled: true,
                spoken_symbols: true,
                preferred_casing: CodeCasingStyle::CamelCase,
                wrap_in_fenced_block: false,
            };
            s.pro_post_processing_enabled = true;
        })
    }

    /// Old ProToolsMode "writing" preset.
    fn legacy_writing_preset() -> VoiceWaveSettings {
        legacy(|s| {
            s.format_profile = FormatProfile::Academic;
            s.active_domain_packs = vec![DomainPackId::Productivity];
            s.code_mode = CodeModeSettings::default();
            s.pro_post_processing_enabled = true;
        })
    }

    /// Old ProToolsMode "study" preset.
    fn legacy_study_preset() -> VoiceWaveSettings {
        legacy(|s| {
            s.format_profile = FormatProfile::Concise;
            s.active_domain_packs = vec![DomainPackId::Student, DomainPackId::Productivity];
            s.code_mode = CodeModeSettings::default();
            s.pro_post_processing_enabled = true;
        })
    }

    /// A hand-edited custom mixture no preset ever produced.
    fn legacy_custom_mixture() -> VoiceWaveSettings {
        legacy(|s| {
            s.format_profile = FormatProfile::Technical;
            s.active_domain_packs = vec![DomainPackId::Productivity, DomainPackId::Coding];
            s.code_mode = CodeModeSettings {
                enabled: false,
                spoken_symbols: false,
                preferred_casing: CodeCasingStyle::SnakeCase,
                wrap_in_fenced_block: true,
            };
            s.pro_post_processing_enabled = false;
        })
    }

    #[test]
    fn migration_maps_all_four_legacy_presets() {
        for (mut settings, expected_profile, expected_customized) in [
            (legacy_default_preset(), PolishProfile::Standard, false),
            (legacy_coding_preset(), PolishProfile::Coding, false),
            (legacy_writing_preset(), PolishProfile::Writing, false),
            // Study keeps its heavier bundle as overrides -> customized.
            // Lands on Writing (Casual was cut from the v1 selectable
            // lineup; a migration must never target an unselectable card).
            (legacy_study_preset(), PolishProfile::Writing, true),
        ] {
            let before = settings.clone();
            migrate_and_sync_polish_profile(&mut settings);
            assert_eq!(
                settings.polish_profile,
                Some(expected_profile),
                "profile for {:?}",
                before.format_profile
            );
            assert_eq!(
                settings.polish_profile_customized, expected_customized,
                "customized for {:?}",
                before.format_profile
            );
            // Migration must NOT rewrite the deterministic bundle: day-one
            // output stays byte-identical.
            assert_eq!(settings.format_profile, before.format_profile);
            assert_eq!(settings.active_domain_packs, before.active_domain_packs);
            assert_eq!(settings.code_mode, before.code_mode);
            assert_eq!(
                settings.pro_post_processing_enabled,
                before.pro_post_processing_enabled
            );
        }
    }

    #[test]
    fn migration_maps_custom_mixture_and_preserves_overrides() {
        let mut settings = legacy_custom_mixture();
        let before = settings.clone();
        migrate_and_sync_polish_profile(&mut settings);
        // Coding pack wins the inference (mirrors detectProToolsMode).
        assert_eq!(settings.polish_profile, Some(PolishProfile::Coding));
        assert!(settings.polish_profile_customized);
        assert_eq!(settings.format_profile, before.format_profile);
        assert_eq!(settings.active_domain_packs, before.active_domain_packs);
        assert_eq!(settings.code_mode, before.code_mode);
        assert_eq!(
            settings.pro_post_processing_enabled,
            before.pro_post_processing_enabled
        );
    }

    #[test]
    fn migration_via_settings_store_detects_legacy_file() {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("voicewave-profile-migrate-{ts}.json"));
        // Legacy file: old writing preset, no polishProfile key.
        let raw = r#"{
            "activeModel": "fw-small.en",
            "formatProfile": "academic",
            "activeDomainPacks": ["productivity"],
            "proPostProcessingEnabled": true
        }"#;
        std::fs::write(&path, raw).expect("write legacy settings");
        let store = SettingsStore::from_path(&path);
        let loaded = store.load().expect("load should succeed");
        assert_eq!(loaded.polish_profile, Some(PolishProfile::Writing));
        assert!(!loaded.polish_profile_customized);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn fresh_default_settings_are_standard_and_not_customized() {
        let mut settings = VoiceWaveSettings::default();
        assert_eq!(settings.polish_profile, Some(PolishProfile::Standard));
        migrate_and_sync_polish_profile(&mut settings);
        assert_eq!(settings.polish_profile, Some(PolishProfile::Standard));
        assert!(!settings.polish_profile_customized);
    }

    #[test]
    fn apply_profile_defaults_resets_overrides_atomically() {
        let mut settings = legacy_study_preset();
        migrate_and_sync_polish_profile(&mut settings);
        assert!(settings.polish_profile_customized);

        apply_profile_defaults(PolishProfile::Casual, &mut settings);
        assert_eq!(settings.polish_profile, Some(PolishProfile::Casual));
        assert!(!settings.polish_profile_customized);
        assert_eq!(settings.format_profile, FormatProfile::Concise);
        assert!(settings.active_domain_packs.is_empty());
        assert!(!is_customized(PolishProfile::Casual, &settings));
    }

    #[test]
    fn editing_a_derived_field_marks_customized() {
        let mut settings = VoiceWaveSettings::default();
        apply_profile_defaults(PolishProfile::Writing, &mut settings);
        assert!(!is_customized(PolishProfile::Writing, &settings));

        settings.code_mode.enabled = true;
        migrate_and_sync_polish_profile(&mut settings);
        assert!(settings.polish_profile_customized);
        // The selected profile itself must survive the edit.
        assert_eq!(settings.polish_profile, Some(PolishProfile::Writing));
    }

    #[test]
    fn domain_pack_order_is_not_customization() {
        let mut settings = VoiceWaveSettings::default();
        apply_profile_defaults(PolishProfile::Coding, &mut settings);
        settings.active_domain_packs = vec![DomainPackId::Coding];
        assert!(!is_customized(PolishProfile::Coding, &settings));
    }

    #[test]
    fn resolve_profile_maps_insert_paths() {
        let settings = VoiceWaveSettings::default();
        assert_eq!(
            resolve_profile(PolishProfile::Standard, &settings).insert_path,
            InsertPath::Immediate
        );
        for profile in [
            PolishProfile::Coding,
            PolishProfile::Writing,
            PolishProfile::Casual,
        ] {
            let policy = resolve_profile(profile, &settings);
            assert_eq!(policy.insert_path, InsertPath::WaitValidated);
            assert_eq!(policy.prompt_id, profile.as_str());
        }
        assert_eq!(
            resolve_profile(PolishProfile::Literal, &settings).insert_path,
            InsertPath::LiteralImmediate
        );
    }

    #[test]
    fn wire_strings_round_trip() {
        for profile in [
            PolishProfile::Standard,
            PolishProfile::Coding,
            PolishProfile::Writing,
            PolishProfile::Casual,
            PolishProfile::Literal,
        ] {
            assert_eq!(PolishProfile::parse(profile.as_str()), Some(profile));
            let json = serde_json::to_string(&profile).expect("serialize");
            assert_eq!(json, format!("\"{}\"", profile.as_str()));
            let back: PolishProfile = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(back, profile);
        }
        assert_eq!(PolishProfile::parse("verbatim"), None);
        assert_eq!(PolishProfile::parse(""), None);
    }
}
