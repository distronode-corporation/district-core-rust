//! A support request's status label (4.0.2): the catalog's label in both
//! languages, never the help desk's own word, and a label from the category for
//! a status this build does not list. The same rows as District AI for iOS's
//! `SupportStatusLabelTests`.

use district_l10n::{Locale, listed_support_status_label, support_status_label};

const EN: Locale = Locale::En;
const FR: Locale = Locale::FrCa;

#[test]
fn every_listed_status_has_its_label_in_both_languages() {
    let rows = [
        ("Received", "Received", "Reçue"),
        ("Open", "Open", "Ouverte"),
        ("Reopened", "Open", "Ouverte"),
        ("To Do", "Open", "Ouverte"),
        (
            "Waiting for support",
            "Waiting for support",
            "En attente du soutien",
        ),
        (
            "Waiting for customer",
            "Waiting for your reply",
            "En attente de votre réponse",
        ),
        ("In Progress", "In progress", "En cours"),
        ("Work in progress", "In progress", "En cours"),
        ("Pending", "Pending", "En attente"),
        ("Escalated", "Escalated", "Transférée"),
        ("Resolved", "Resolved", "Réglée"),
        ("Done", "Done", "Terminée"),
        ("Completed", "Done", "Terminée"),
        ("Closed", "Closed", "Fermée"),
        ("Canceled", "Canceled", "Annulée"),
        ("Cancelled", "Canceled", "Annulée"),
        ("Declined", "Declined", "Refusée"),
        ("unknown", "Status unknown", "État inconnu"),
    ];
    for (name, english, french) in rows {
        assert_eq!(
            support_status_label(name, "INDETERMINATE", EN),
            english,
            "{name}"
        );
        assert_eq!(
            support_status_label(name, "INDETERMINATE", FR),
            french,
            "{name}"
        );
    }
}

#[test]
fn a_name_is_matched_trimmed_and_without_case() {
    assert_eq!(
        support_status_label("  WAITING FOR SUPPORT \n", "NEW", EN),
        "Waiting for support"
    );
    assert_eq!(support_status_label("\tdone ", "", FR), "Terminée");
}

/// The desk's own word is never shown: the live workflow is localised, so an
/// unlisted name reads as its category's label, in the app's language.
#[test]
fn an_unlisted_name_reads_as_its_category() {
    let rows = [
        ("NEW", "Open", "Ouverte"),
        ("new", "Open", "Ouverte"),
        ("INDETERMINATE", "In progress", "En cours"),
        ("indeterminate", "In progress", "En cours"),
        ("DONE", "Closed", "Fermée"),
        (" done ", "Closed", "Fermée"),
        ("PENDING", "Received", "Reçue"),
    ];
    for (category, english, french) in rows {
        assert_eq!(
            support_status_label("完成", category, EN),
            english,
            "{category}"
        );
        assert_eq!(
            support_status_label("等待客户", category, FR),
            french,
            "{category}"
        );
    }
}

#[test]
fn an_unlisted_name_in_an_unknown_category_is_the_generic_label() {
    assert_eq!(
        support_status_label("Triage", "SOMETHING_NEW", EN),
        "Status unknown"
    );
    assert_eq!(support_status_label("Triage", "", FR), "État inconnu");
    assert_eq!(support_status_label("", "", EN), "Status unknown");
}

#[test]
fn only_a_listed_name_has_a_listed_label() {
    assert_eq!(
        listed_support_status_label(" Completed ", FR),
        Some("Terminée")
    );
    assert_eq!(
        listed_support_status_label("Resolved", EN),
        Some("Resolved")
    );
    assert_eq!(listed_support_status_label("完成", EN), None);
    assert_eq!(listed_support_status_label("", FR), None);
}
