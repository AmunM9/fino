//! The language Fino speaks. By default it follows the OS: on macOS the preferred
//! languages, which include the per-app choice in System Settings → Language & Region;
//! on Windows the display language. Fino ships English and Spanish; anything else gets
//! English, unless Spanish also appears further down the person's list.

use crate::model::LanguageChoice;
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Language {
    En,
    Es,
}

/// The first supported language in the person's ordered list ("es-CO", "en-US", "fr"…).
pub fn from_preferences<S: AsRef<str>>(preferred: impl IntoIterator<Item = S>) -> Language {
    preferred
        .into_iter()
        .find_map(|tag| {
            let primary = tag.as_ref().split(['-', '_']).next()?.to_ascii_lowercase();
            match primary.as_str() {
                "en" => Some(Language::En),
                "es" => Some(Language::Es),
                _ => None,
            }
        })
        .unwrap_or(Language::En)
}

pub fn resolve(choice: LanguageChoice) -> Language {
    match choice {
        LanguageChoice::En => Language::En,
        LanguageChoice::Es => Language::Es,
        LanguageChoice::System => from_preferences(sys_locale::get_locales()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn regional_variants_map_to_their_language() {
        assert_eq!(from_preferences(["es-CO"]), Language::Es);
        assert_eq!(from_preferences(["es_419"]), Language::Es);
        assert_eq!(from_preferences(["en-GB"]), Language::En);
        assert_eq!(from_preferences(["ES"]), Language::Es);
    }

    #[test]
    fn the_first_supported_language_in_the_list_wins() {
        assert_eq!(from_preferences(["fr-FR", "es-ES", "en-US"]), Language::Es);
        assert_eq!(from_preferences(["de-DE", "en-US", "es-ES"]), Language::En);
    }

    #[test]
    fn unsupported_or_missing_languages_fall_back_to_english() {
        assert_eq!(from_preferences(["fr-FR", "pt-BR"]), Language::En);
        assert_eq!(from_preferences(Vec::<String>::new()), Language::En);
    }

    #[test]
    fn an_explicit_choice_ignores_the_system() {
        assert_eq!(resolve(LanguageChoice::En), Language::En);
        assert_eq!(resolve(LanguageChoice::Es), Language::Es);
    }

    #[test]
    fn settings_saved_before_the_option_existed_follow_the_system() {
        let old: crate::model::Settings =
            serde_json::from_str(r#"{"outputMode":"replace"}"#).unwrap();
        assert_eq!(old.language, LanguageChoice::System);
        let english: crate::model::Settings = serde_json::from_str(r#"{"language":"en"}"#).unwrap();
        assert_eq!(english.language, LanguageChoice::En);
    }
}
