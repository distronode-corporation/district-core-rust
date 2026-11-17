//! Values the wizard writes into the business's own data. They are chosen by
//! the receptionist's language, not the app's, so they are not in the catalog,
//! and they match the web's byte for byte because the web reads them back:
//! step 1's question is how either client finds the "what you do" answer it
//! wrote, and the starter greetings are how both tell an untouched greeting
//! from the owner's own.

/// The language the receptionist speaks, as step 1 offers it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SetupLanguage {
    /// English.
    En,
    /// French.
    Fr,
}

impl SetupLanguage {
    /// Both, in the order step 1 offers them.
    pub const ALL: [Self; 2] = [Self::En, Self::Fr];

    /// `fr-CA` is French and `en-US` English; anything else is `None` (the
    /// web's `localeOfPersonaLanguage`).
    pub fn from_persona(language: Option<&str>) -> Option<Self> {
        let lower = language.unwrap_or_default().to_ascii_lowercase();
        if lower.starts_with("fr") {
            Some(Self::Fr)
        } else if lower.starts_with("en") {
            Some(Self::En)
        } else {
            None
        }
    }

    /// The app's own language, for a workspace whose persona names none.
    pub fn app_default(locale: district_l10n::Locale) -> Self {
        match locale {
            district_l10n::Locale::En => Self::En,
            district_l10n::Locale::FrCa => Self::Fr,
        }
    }

    /// The persona `language` this choice is saved as when nothing is stored.
    pub fn persona_default(self) -> &'static str {
        match self {
            Self::En => "en-US",
            Self::Fr => "fr-CA",
        }
    }

    /// The persona `language` step 3 saves (the web's `personaLanguageToSave`).
    ///
    /// A stored language is kept unless the owner chose the other family: step
    /// 1 offers only English and French, so a receptionist set to German in
    /// settings must not come back as `en-US`. A stored language outside both
    /// families is kept when the owner left step 1 on the app's language, which
    /// is where it opened.
    pub fn to_save(stored: Option<&str>, chosen: Self, app_default: Self) -> String {
        let kept = stored.unwrap_or_default().trim();
        if !kept.is_empty() {
            let family = Self::from_persona(Some(kept));
            if family == Some(chosen) || (family.is_none() && chosen == app_default) {
                return kept.to_owned();
            }
        }
        chosen.persona_default().to_owned()
    }
}

/// The question step 1 stores "what you do" under (`setupCopy[language].business.whatQuestion`).
pub fn what_question(language: SetupLanguage) -> &'static str {
    match language {
        SetupLanguage::En => "What does the business do?",
        SetupLanguage::Fr => "Que fait l'entreprise?",
    }
}

/// Whether `question` is step 1's "what you do" question, in either language.
pub fn is_what_question(question: &str) -> bool {
    SetupLanguage::ALL
        .iter()
        .any(|language| what_question(*language) == question)
}

/// `STARTER_PERSONA_NAME`, for a persona that holds no name.
pub const FALLBACK_NAME: &str = "Ava";

/// `starterGreeting(locale)` in the service's first-run defaults, both languages.
pub const STARTER_GREETINGS: [&str; 2] = [
    "Thanks for calling. This is Ava. How can I help you today?",
    "Merci de votre appel. Ici Ava. Comment puis-je vous aider aujourd'hui?",
];

/// `setupCopy[language].receptionist.greeting` and `greetingNoBusiness`.
pub fn greeting(business: &str, name: &str, language: SetupLanguage) -> String {
    let business = business.trim();
    match (language, business.is_empty()) {
        (SetupLanguage::Fr, false) => {
            format!("Merci d'avoir appelé {business}, ici {name}. Comment puis-je vous aider?")
        }
        (SetupLanguage::Fr, true) => {
            format!("Merci d'avoir appelé, ici {name}. Comment puis-je vous aider?")
        }
        (SetupLanguage::En, false) => {
            format!("Thanks for calling {business}, this is {name}. How can I help?")
        }
        (SetupLanguage::En, true) => format!("Thanks for calling, this is {name}. How can I help?"),
    }
}

/// The name a workspace is provisioned with when its buyer gave no business name.
pub fn is_default_workspace_name(name: &str) -> bool {
    name.trim().ends_with("'s Communications Workspace")
}

/// The web's `toE164`: an E.164 number, or a ten- or eleven-digit North
/// American one in a `us` or `ca` workspace; `None` for anything else.
pub fn e164(raw: &str, region: Option<&str>) -> Option<String> {
    let compact: String = raw.chars().filter(|c| !" ().-".contains(*c)).collect();
    let digits = |text: &str| text.chars().all(|c| c.is_ascii_digit());
    if let Some(rest) = compact.strip_prefix('+') {
        return ((8..=15).contains(&rest.len()) && digits(rest)).then_some(compact);
    }
    if !matches!(region, Some("us" | "ca")) || !digits(&compact) {
        return None;
    }
    let area_ok = |first: Option<char>| first.is_some_and(|c| ('2'..='9').contains(&c));
    match compact.len() {
        10 if area_ok(compact.chars().next()) => Some(format!("+1{compact}")),
        11 if compact.starts_with('1') && area_ok(compact.chars().nth(1)) => {
            Some(format!("+{compact}"))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use district_l10n::Locale;

    #[test]
    fn a_persona_language_reads_by_its_family() {
        assert_eq!(
            SetupLanguage::from_persona(Some("fr-CA")),
            Some(SetupLanguage::Fr)
        );
        assert_eq!(
            SetupLanguage::from_persona(Some("EN-us")),
            Some(SetupLanguage::En)
        );
        assert_eq!(SetupLanguage::from_persona(Some("de-DE")), None);
        assert_eq!(SetupLanguage::from_persona(None), None);
        assert_eq!(SetupLanguage::app_default(Locale::En), SetupLanguage::En);
        assert_eq!(SetupLanguage::app_default(Locale::FrCa), SetupLanguage::Fr);
    }

    #[test]
    fn a_stored_language_is_kept_unless_the_other_family_was_chosen() {
        use SetupLanguage::{En, Fr};
        assert_eq!(SetupLanguage::to_save(Some("fr-FR"), Fr, En), "fr-FR");
        assert_eq!(SetupLanguage::to_save(Some("fr-FR"), En, En), "en-US");
        assert_eq!(SetupLanguage::to_save(Some("de-DE"), En, En), "de-DE");
        assert_eq!(SetupLanguage::to_save(Some("de-DE"), Fr, En), "fr-CA");
        assert_eq!(SetupLanguage::to_save(Some("  "), Fr, En), "fr-CA");
        assert_eq!(SetupLanguage::to_save(None, En, Fr), "en-US");
    }

    #[test]
    fn the_wire_words_are_the_webs() {
        assert!(is_what_question("What does the business do?"));
        assert!(is_what_question("Que fait l'entreprise?"));
        assert!(!is_what_question("What do you do?"));
        assert_eq!(
            greeting(" Acme ", "Ava", SetupLanguage::En),
            "Thanks for calling Acme, this is Ava. How can I help?"
        );
        assert_eq!(
            greeting("", "Ava", SetupLanguage::En),
            "Thanks for calling, this is Ava. How can I help?"
        );
        assert_eq!(
            greeting("Acme", "Léa", SetupLanguage::Fr),
            "Merci d'avoir appelé Acme, ici Léa. Comment puis-je vous aider?"
        );
        assert_eq!(
            greeting(" ", "Léa", SetupLanguage::Fr),
            "Merci d'avoir appelé, ici Léa. Comment puis-je vous aider?"
        );
        assert!(is_default_workspace_name("Sam's Communications Workspace "));
        assert!(!is_default_workspace_name("Acme Dental"));
    }

    #[test]
    fn a_phone_number_reads_as_e164_or_not_at_all() {
        assert_eq!(
            e164("+1 (416) 555-0142", None).as_deref(),
            Some("+14165550142")
        );
        assert_eq!(e164("+1555", None), None);
        assert_eq!(e164("+1x", Some("us")), None);
        assert_eq!(
            e164("(416) 555-0123", Some("ca")).as_deref(),
            Some("+14165550123")
        );
        assert_eq!(
            e164("1-416-555-0123", Some("us")).as_deref(),
            Some("+14165550123")
        );
        assert_eq!(e164("4165550123", Some("eu")), None);
        assert_eq!(e164("1165550123", Some("us")), None);
        assert_eq!(e164("11165550123", Some("us")), None);
        assert_eq!(e164("21165550123", Some("us")), None);
        assert_eq!(e164("416555012", Some("us")), None);
        assert_eq!(e164("41655501x3", Some("us")), None);
    }
}
