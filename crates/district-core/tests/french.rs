//! A model built in French: the core writes in the language its configuration
//! names. A binary of its own, because the language is the process's and every
//! other test runs in English.
//!
//! Every entry has its French, so `FR_COMPLETE` is true; what this holds is
//! that French is the arm taken.

use district_api::{ApiError, ErrorDetail};
use district_core::l10n::{self, Locale, api_error_text, strings};
use district_core::{CoreConfig, DialerScreen, FailureText, Model};

fn config(locale: Locale) -> CoreConfig {
    CoreConfig {
        web_base_url: "https://www.distronode.com".to_owned(),
        app_version: "0.0.0-test".to_owned(),
        calls_available: false,
        in_app_purchases: false,
        meta_channels: false,
        locale,
    }
}

#[test]
fn a_french_model_writes_in_french_until_told_otherwise() {
    assert!(
        std::hint::black_box(l10n::FR_COMPLETE),
        "every entry has its French"
    );
    let (mut model, _) = Model::new(config(Locale::FrCa));
    assert_eq!(model.config().locale, Locale::FrCa);
    assert_eq!(l10n::current(), Locale::FrCa);

    // The French arm is the one taken: the sentence for a known code, never the
    // service's English.
    let refusal = ApiError::NotFound(ErrorDetail {
        message: Some("No such thing, in English.".to_owned()),
        code: Some("contact_not_found".to_owned()),
        degraded_regions: Vec::new(),
    });
    let text = FailureText::from_api_error(&refusal);
    assert_eq!(
        Some(text.message),
        api_error_text::<&str>(Locale::FrCa, "contact_not_found", &[])
    );
    assert_eq!(
        FailureText::from_retry_reason(district_api::RetryReason::Offline).message,
        strings::failure_offline(Locale::FrCa)
    );

    // The dialler names the country, and writes its lines, in French.
    let dialer = DialerScreen {
        entry: "(416) 555-0199".to_owned(),
        country: Some("CA".to_owned()),
        ..DialerScreen::default()
    };
    assert_eq!(
        dialer.destination().as_deref(),
        Some("Pays de destination\u{a0}: Canada \u{b7} +1 416 555 0199")
    );
    assert_eq!(
        dialer.hint(),
        "Saisissez le numéro comme vous le composeriez sur place (Canada), ou commencez par + et \
         l'indicatif du pays."
    );
    let us = DialerScreen {
        country: Some("US".to_owned()),
        ..DialerScreen::default()
    };
    assert_eq!(us.country_label(), "États-Unis, indicatif +1");

    the_parity_refusals_are_read_by_their_codes(&mut model);

    model.set_locale(Locale::En);
    assert_eq!(model.config().locale, Locale::En);
    assert_eq!(l10n::current(), Locale::En);
    assert_eq!(
        FailureText::from_api_error(&refusal).message,
        "No such thing, in English."
    );
}

#[test]
fn the_default_configuration_is_english() {
    assert_eq!(Locale::default(), Locale::En);
}

/// The refusals the reply channels, the owner picker, the WhatsApp composer and
/// the Messenger and Instagram window meet are read from the error table in French, by their codes, and keep the
/// service's own English otherwise.
/// Called from the test above, not a test of its own: the language is the
/// process's, and tests run side by side.
fn the_parity_refusals_are_read_by_their_codes(model: &mut Model) {
    for code in [
        "sms_reply_not_customer_channel",
        "invalid_request",
        "invalid_phone_number",
        "assignee_not_member",
        "assignee_is_viewer",
        "whatsapp_window_closed",
        "whatsapp_template_invalid",
        "messenger_window_closed",
    ] {
        let refusal = ApiError::Envelope {
            status: 400,
            code: code.to_owned(),
            detail: ErrorDetail {
                message: Some(format!("The service's English for {code}.")),
                code: Some(code.to_owned()),
                degraded_regions: Vec::new(),
            },
        };
        let french = api_error_text::<&str>(Locale::FrCa, code, &[]);
        assert!(french.is_some(), "{code} is in the table");
        assert_eq!(Some(FailureText::from_api_error(&refusal).message), french);
        model.set_locale(Locale::En);
        assert_eq!(
            FailureText::from_api_error(&refusal).message,
            format!("The service's English for {code}.")
        );
        model.set_locale(Locale::FrCa);
    }
    assert_eq!(
        district_core::reply_channel_label("email"),
        Some("Courriel")
    );
    assert_eq!(
        district_core::WhatsAppComposer::variable_label(1),
        "Variable {{2}}"
    );
    // The Messenger and Instagram window's two lines, the web Inbox's French.
    assert_eq!(
        strings::thread_meta_human_agent(Locale::FrCa),
        "Réponse d'un agent humain (fenêtre de 7\u{a0}jours)"
    );
    assert!(
        strings::thread_meta_window_closed(Locale::FrCa)
            .starts_with("Fenêtre de réponse fermée\u{a0}:")
    );
}
