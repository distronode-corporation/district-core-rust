//! Settings: the booking page and its images, the booking assistant, the
//! member's profile and picture, and their notifications.

use district_core::scheduling_admin::{
    AssistantEditor, BrandingEditor, BrandingForm, IMAGE_MAX_BYTES, ImageFile, NOTIFICATIONS,
    NotificationsEditor, ProfileEditor, ProfileForm, SettingsDialog, SettingsEvent,
    SettingsSection, SettingsTab, WriteState, date_format_label, locale_label, notification_groups,
    notification_label, notification_value, set_label, time_format_label, week_start_label,
};
use district_core::{Effect, Model, SchedulingAdminEvent, SchedulingSection};
use district_model::{
    SchedulingAdminData, SchedulingAdminRequest, SchedulingBranding, SchedulingBrandingPatch,
    SchedulingLlmSettings, SchedulingMe, SchedulingOk, SchedulingProfilePatch,
    SchedulingUploadResult, SchedulingUploadTarget,
};

use super::{act, admin, answer, data, done, open, refused, sent};
use crate::support::server_error;

fn section(model: &Model) -> &SettingsSection {
    admin(model).settings.as_ref().expect("settings are open")
}

fn settings(model: &mut Model, event: SettingsEvent) -> Vec<Effect> {
    act(model, SchedulingAdminEvent::Settings(event))
}

fn me() -> SchedulingMe {
    data("district-scheduling-me.json")
}

fn branding() -> SchedulingBranding {
    data("district-scheduling-branding.json")
}

fn llm() -> SchedulingLlmSettings {
    data("district-scheduling-llm.json")
}

fn ok() -> SchedulingAdminData {
    SchedulingAdminData::Done(SchedulingOk { ok: true })
}

fn png(size: usize) -> ImageFile {
    ImageFile {
        mime_type: "image/png".into(),
        bytes: vec![1; size],
    }
}

/// Settings open as `role`, the booking page read.
fn read(role: &str) -> Model {
    let (mut model, effects) = open(role, SchedulingSection::Settings);
    assert_eq!(sent(&effects).len(), 1);
    assert_eq!(sent(&effects)[0].1, SchedulingAdminRequest::Branding);
    answer(
        &mut model,
        sent(&effects)[0].0,
        done(SchedulingAdminData::Branding(branding())),
    );
    model
}

/// `tab` shown, its read answered with `data`.
fn show(model: &mut Model, tab: SettingsTab, data: SchedulingAdminData) {
    let effects = settings(model, SettingsEvent::SelectTab(tab));
    answer(model, sent(&effects)[0].0, done(data));
}

#[test]
fn each_tab_is_read_once_and_says_what_is_set() {
    let mut model = read("viewer");
    let tabs: Vec<&str> = SettingsTab::ALL.iter().map(|t| t.label()).collect();
    assert_eq!(
        tabs,
        [
            "Booking page",
            "Booking assistant",
            "Your profile",
            "Your notifications"
        ]
    );
    assert!(SettingsSection::labels().iter().all(|l| !l.is_empty()));
    let b = branding();
    assert_eq!((set_label(&b.logo_url), set_label("")), ("Set", "Not set"));
    assert_eq!(
        locale_label(&b.fallback_locale, b.supported_locales.as_deref()),
        "English"
    );
    assert_eq!(locale_label(" de ", b.supported_locales.as_deref()), "de");
    assert_eq!(locale_label("de", None), "de");
    assert_eq!(locale_label(" ", None), "Not set");

    show(
        &mut model,
        SettingsTab::Assistant,
        SchedulingAdminData::Llm(llm()),
    );
    assert_eq!(
        section(&model).assistant.as_ref().and_then(|a| a.value()),
        Some(&llm())
    );
    show(
        &mut model,
        SettingsTab::Profile,
        SchedulingAdminData::Me(me()),
    );
    // The notifications read the same profile: no second read.
    assert!(
        settings(
            &mut model,
            SettingsEvent::SelectTab(SettingsTab::Notifications)
        )
        .is_empty()
    );
    assert!(
        settings(
            &mut model,
            SettingsEvent::SelectTab(SettingsTab::BookingPage)
        )
        .is_empty()
    );

    assert_eq!(time_format_label("12h"), "12-hour (2:30 pm)");
    assert_eq!(time_format_label("24h"), "24-hour (14:30)");
    assert_eq!(time_format_label("x"), "x");
    assert_eq!(date_format_label("dmy"), "31/12/2026");
    assert_eq!(date_format_label("mdy"), "12/31/2026");
    assert_eq!(date_format_label("ymd"), "2026-12-31");
    assert_eq!(date_format_label("x"), "x");
    assert_eq!(week_start_label(0), "Sunday");
    assert_eq!(week_start_label(1), "Monday");
    assert_eq!(week_start_label(7), "7");
    assert_eq!(week_start_label(-1), "-1");

    let groups = notification_groups();
    assert_eq!(groups[0].0, "As an attendee");
    assert_eq!(groups[1].2, &NOTIFICATIONS[4..]);
    let values: Vec<bool> = NOTIFICATIONS
        .iter()
        .map(|f| notification_value(f, &me()))
        .collect();
    assert_eq!(values, [true, true, false, true, true, false, true]);
    assert!(!notification_value("notify_unknown", &me()));
    let labels: Vec<&str> = NOTIFICATIONS
        .iter()
        .map(|f| notification_label(f))
        .collect();
    assert!(
        labels.iter().all(|l| !l.starts_with("notify_")),
        "{labels:?}"
    );
    assert_eq!(notification_label("notify_unknown"), "notify_unknown");
}

#[test]
fn a_refresh_reads_the_tab_again_and_a_failed_read_says_why() {
    let (mut model, effects) = open("agency", SchedulingSection::Settings);
    answer(&mut model, sent(&effects)[0].0, Err(server_error()));
    assert!(
        section(&model)
            .branding
            .as_ref()
            .and_then(|b| b.failure())
            .is_some()
    );
    // Nothing to edit.
    settings(&mut model, SettingsEvent::OpenBranding);
    assert!(section(&model).dialog.is_none());
    let first = settings(&mut model, SettingsEvent::Refresh);
    let second = settings(&mut model, SettingsEvent::Refresh);
    // The first read is superseded.
    answer(
        &mut model,
        sent(&first)[0].0,
        done(SchedulingAdminData::Branding(branding())),
    );
    assert!(
        section(&model)
            .branding
            .as_ref()
            .is_some_and(|b| b.value().is_none())
    );
    answer(
        &mut model,
        sent(&second)[0].0,
        done(SchedulingAdminData::Branding(branding())),
    );
    assert!(
        section(&model)
            .branding
            .as_ref()
            .is_some_and(|b| b.value().is_some())
    );
    for (tab, request) in [
        (SettingsTab::Assistant, SchedulingAdminRequest::Llm),
        (SettingsTab::Notifications, SchedulingAdminRequest::Me),
    ] {
        settings(&mut model, SettingsEvent::SelectTab(tab));
        let effects = settings(&mut model, SettingsEvent::Refresh);
        assert_eq!(sent(&effects)[0].1, request);
    }
    // Closed: the events do nothing.
    let (mut model, _) = open("agency", SchedulingSection::Overview);
    assert!(settings(&mut model, SettingsEvent::Refresh).is_empty());
}

fn profile(model: &Model) -> &ProfileEditor {
    match section(model).dialog.as_ref() {
        Some(SettingsDialog::Profile(editor)) => editor,
        other => panic!("{other:?}"),
    }
}

#[test]
fn any_member_saves_their_own_checked_profile() {
    let mut model = read("viewer");
    // Not read yet: nothing to edit.
    settings(&mut model, SettingsEvent::OpenProfile);
    settings(&mut model, SettingsEvent::OpenNotifications);
    assert!(section(&model).dialog.is_none());
    show(
        &mut model,
        SettingsTab::Profile,
        SchedulingAdminData::Me(me()),
    );
    settings(&mut model, SettingsEvent::OpenProfile);
    let editor = profile(&model);
    assert_eq!(editor.form.name, "Contract Member");
    assert_eq!(
        editor.avatar_url(),
        Some("https://book.example.com/media/avatars/contract.png")
    );
    assert!(ProfileEditor::labels().iter().all(|l| !l.is_empty()));
    let zones = editor.timezone_options();
    assert!(zones.iter().any(|z| z == "America/Toronto"));
    assert!(zones.windows(2).all(|w| w[0] <= w[1]));

    let valid = profile(&model).form.clone();
    for (form, issue) in [
        (
            ProfileForm {
                name: " ".into(),
                ..valid.clone()
            },
            "Give your name.",
        ),
        (
            ProfileForm {
                name: "x".repeat(201),
                ..valid.clone()
            },
            "That name is longer than 200 characters.",
        ),
        (
            ProfileForm {
                timezone: " ".into(),
                ..valid.clone()
            },
            "Choose your timezone.",
        ),
        (
            ProfileForm {
                time_format: "ampm".into(),
                ..valid.clone()
            },
            "Choose 12-hour or 24-hour.",
        ),
        (
            ProfileForm {
                week_start: 7,
                ..valid.clone()
            },
            "Choose the day your week starts on.",
        ),
        (
            ProfileForm {
                date_format: "dd".into(),
                ..valid.clone()
            },
            "Choose how dates are written.",
        ),
    ] {
        settings(&mut model, SettingsEvent::EditProfile(form));
        assert_eq!(profile(&model).rejected, None);
        assert!(settings(&mut model, SettingsEvent::SaveProfile).is_empty());
        assert_eq!(profile(&model).rejected, Some(issue));
    }
    // A zone this build does not list is offered first.
    settings(
        &mut model,
        SettingsEvent::EditProfile(ProfileForm {
            name: " Ann ".into(),
            timezone: "Mars/Olympus".into(),
            ..valid.clone()
        }),
    );
    assert_eq!(profile(&model).timezone_options()[0], "Mars/Olympus");
    let effects = settings(&mut model, SettingsEvent::SaveProfile);
    assert_eq!(
        sent(&effects)[0].1,
        SchedulingAdminRequest::PatchMe(SchedulingProfilePatch {
            name: Some("Ann".into()),
            timezone: Some("Mars/Olympus".into()),
            time_format: Some("24h".into()),
            week_start: Some(1),
            date_format: Some("ymd".into()),
            ..SchedulingProfilePatch::default()
        })
    );
    // One write at a time.
    assert!(settings(&mut model, SettingsEvent::SaveProfile).is_empty());
    answer(&mut model, sent(&effects)[0].0, refused("unavailable"));
    assert!(profile(&model).write.failure().is_some());
    settings(&mut model, SettingsEvent::EditProfile(valid.clone()));
    assert_eq!(profile(&model).write, WriteState::Idle);
    let effects = settings(&mut model, SettingsEvent::SaveProfile);
    let saved = SchedulingMe {
        name: "Ann".into(),
        ..me()
    };
    answer(
        &mut model,
        sent(&effects)[0].0,
        done(SchedulingAdminData::Me(saved.clone())),
    );
    assert_eq!(
        profile(&model).write,
        WriteState::Done("Profile saved".into())
    );
    assert_eq!(
        section(&model).profile.as_ref().and_then(|p| p.value()),
        Some(&saved)
    );
    // An answer that is not a profile is not taken for one.
    let effects = settings(&mut model, SettingsEvent::SaveProfile);
    answer(&mut model, sent(&effects)[0].0, done(ok()));
    assert!(profile(&model).write.failure().is_some());
}

#[test]
fn the_picture_is_checked_uploaded_and_removed() {
    let mut model = read("viewer");
    show(
        &mut model,
        SettingsTab::Profile,
        SchedulingAdminData::Me(me()),
    );
    settings(&mut model, SettingsEvent::OpenProfile);
    for (file, issue) in [
        (png(0), "That image could not be read. Choose another."),
        (
            png(IMAGE_MAX_BYTES + 1),
            "That image is larger than 5 MB. Choose a smaller one.",
        ),
        (
            ImageFile {
                mime_type: "image/tiff".into(),
                bytes: vec![1],
            },
            "That file is not an image we can use. Choose a JPEG, PNG, GIF or WebP.",
        ),
    ] {
        let effects = settings(
            &mut model,
            SettingsEvent::UploadImage {
                target: SchedulingUploadTarget::Avatar,
                file,
            },
        );
        assert!(effects.is_empty());
        assert_eq!(
            profile(&model).avatar.failure().map(|f| f.message.as_str()),
            Some(issue)
        );
    }
    settings(
        &mut model,
        SettingsEvent::ImageUnreadable(SchedulingUploadTarget::Avatar),
    );
    assert!(profile(&model).avatar.failure().is_some());
    assert!(!format!("{:?}", png(3)).contains("[1, 1, 1]"));

    // Published at an address: shown at once.
    for (mime, extension) in [
        ("image/jpeg", "jpg"),
        ("image/gif", "gif"),
        ("image/webp", "webp"),
        ("image/png", "png"),
    ] {
        let effects = settings(
            &mut model,
            SettingsEvent::UploadImage {
                target: SchedulingUploadTarget::Avatar,
                file: ImageFile {
                    mime_type: mime.into(),
                    bytes: vec![1],
                },
            },
        );
        let SchedulingAdminRequest::Upload(upload) = sent(&effects)[0].1.clone() else {
            panic!("{effects:?}");
        };
        assert_eq!(upload.file_name, format!("avatar.{extension}"));
        // One at a time.
        assert!(settings(&mut model, SettingsEvent::SaveProfile).is_empty());
        answer(
            &mut model,
            sent(&effects)[0].0,
            done(SchedulingAdminData::Uploaded(SchedulingUploadResult {
                avatar_url: Some("https://book.example.com/new.png".into()),
                ..SchedulingUploadResult::default()
            })),
        );
    }
    assert_eq!(
        profile(&model).avatar_url(),
        Some("https://book.example.com/new.png")
    );
    assert_eq!(
        profile(&model).avatar,
        WriteState::Done("Profile picture uploaded".into())
    );

    // No address in the answer: the profile is read again, quietly.
    let effects = settings(
        &mut model,
        SettingsEvent::UploadImage {
            target: SchedulingUploadTarget::Avatar,
            file: png(1),
        },
    );
    let reread = answer(
        &mut model,
        sent(&effects)[0].0,
        done(SchedulingAdminData::Uploaded(
            SchedulingUploadResult::default(),
        )),
    );
    assert_eq!(sent(&reread)[0].1, SchedulingAdminRequest::Me);
    answer(&mut model, sent(&reread)[0].0, Err(server_error()));
    assert_eq!(
        profile(&model).avatar_url(),
        Some("https://book.example.com/new.png")
    );

    // Removed: the profile read again.
    let effects = settings(
        &mut model,
        SettingsEvent::RemoveImage(SchedulingUploadTarget::Avatar),
    );
    assert_eq!(sent(&effects)[0].1, SchedulingAdminRequest::DeleteAvatar);
    let reread = answer(&mut model, sent(&effects)[0].0, done(ok()));
    assert_eq!(
        profile(&model).avatar,
        WriteState::Done("Profile picture removed".into())
    );
    let bare = SchedulingMe {
        avatar_url: None,
        ..me()
    };
    answer(
        &mut model,
        sent(&reread)[0].0,
        done(SchedulingAdminData::Me(bare.clone())),
    );
    assert_eq!(profile(&model).avatar_url(), None);
    assert_eq!(profile(&model).me, bare);
    let effects = settings(
        &mut model,
        SettingsEvent::RemoveImage(SchedulingUploadTarget::Avatar),
    );
    answer(&mut model, sent(&effects)[0].0, Err(server_error()));
    assert!(profile(&model).avatar.failure().is_some());
    // The booking page's images are not the profile's.
    assert!(
        settings(
            &mut model,
            SettingsEvent::RemoveImage(SchedulingUploadTarget::Logo)
        )
        .is_empty()
    );
}

fn notifications(model: &Model) -> &NotificationsEditor {
    match section(model).dialog.as_ref() {
        Some(SettingsDialog::Notifications(editor)) => editor,
        other => panic!("{other:?}"),
    }
}

#[test]
fn notifications_are_switched_and_saved_together() {
    let mut model = read("viewer");
    show(
        &mut model,
        SettingsTab::Notifications,
        SchedulingAdminData::Me(me()),
    );
    settings(&mut model, SettingsEvent::OpenNotifications);
    assert!(NotificationsEditor::labels().iter().all(|l| !l.is_empty()));
    assert!(!notifications(&model).value("notify_reschedule"));
    settings(
        &mut model,
        SettingsEvent::SetNotification {
            field: "notify_reschedule".into(),
            on: true,
        },
    );
    let effects = settings(&mut model, SettingsEvent::SaveNotifications);
    assert_eq!(
        sent(&effects)[0].1,
        SchedulingAdminRequest::PatchMe(SchedulingProfilePatch {
            notify_confirmation: Some(true),
            notify_cancellation: Some(true),
            notify_reschedule: Some(true),
            notify_reminder: Some(true),
            notify_host_booking: Some(true),
            notify_host_cancel: Some(false),
            notify_host_reschedule: Some(true),
            ..SchedulingProfilePatch::default()
        })
    );
    assert!(settings(&mut model, SettingsEvent::SaveNotifications).is_empty());
    answer(&mut model, sent(&effects)[0].0, Err(server_error()));
    assert!(notifications(&model).write.failure().is_some());
    settings(
        &mut model,
        SettingsEvent::SetNotification {
            field: "notify_reminder".into(),
            on: false,
        },
    );
    assert_eq!(notifications(&model).write, WriteState::Idle);
    let effects = settings(&mut model, SettingsEvent::SaveNotifications);
    answer(
        &mut model,
        sent(&effects)[0].0,
        done(SchedulingAdminData::Me(me())),
    );
    assert_eq!(
        notifications(&model).write,
        WriteState::Done("Notifications saved".into())
    );
    let effects = settings(&mut model, SettingsEvent::SaveNotifications);
    answer(&mut model, sent(&effects)[0].0, done(ok()));
    assert!(notifications(&model).write.failure().is_some());
    // Other dialogs' events do nothing here.
    assert!(settings(&mut model, SettingsEvent::SaveProfile).is_empty());
}

fn brand(model: &Model) -> &BrandingEditor {
    match section(model).dialog.as_ref() {
        Some(SettingsDialog::Branding(editor)) => editor,
        other => panic!("{other:?}"),
    }
}

#[test]
fn the_booking_page_is_checked_and_saved_by_a_manager() {
    let mut model = read("viewer");
    settings(&mut model, SettingsEvent::OpenBranding);
    assert!(section(&model).dialog.is_none());

    let mut model = read("agency");
    settings(&mut model, SettingsEvent::OpenBranding);
    assert!(BrandingEditor::labels().iter().all(|l| !l.is_empty()));
    assert_eq!(brand(&model).locale_options().len(), 2);
    let valid = brand(&model).form.clone();
    let between = |min: i64, max: i64| format!("Give a whole number between {min} and {max}.");
    for (form, issue) in [
        (
            BrandingForm {
                business_name: "x".repeat(201),
                ..valid.clone()
            },
            "That name is longer than 200 characters.".to_owned(),
        ),
        (
            BrandingForm {
                logo_height: 15,
                ..valid.clone()
            },
            between(16, 64),
        ),
        (
            BrandingForm {
                logo_opacity: 101,
                ..valid.clone()
            },
            between(20, 100),
        ),
        (
            BrandingForm {
                banner_opacity: 19,
                ..valid.clone()
            },
            between(20, 100),
        ),
        (
            BrandingForm {
                privacy_url: format!("https://example.com/{}", "x".repeat(500)),
                ..valid.clone()
            },
            "That link is too long.".to_owned(),
        ),
        (
            BrandingForm {
                terms_url: "example.com/terms".into(),
                ..valid.clone()
            },
            "Give the full address, starting with https://".to_owned(),
        ),
        (
            BrandingForm {
                terms_url: "ftp://example.com/terms".into(),
                ..valid.clone()
            },
            "Give the full address, starting with https://".to_owned(),
        ),
        (
            BrandingForm {
                fallback_locale: " ".into(),
                ..valid.clone()
            },
            "Choose the language the booking page falls back to.".to_owned(),
        ),
    ] {
        settings(&mut model, SettingsEvent::EditBranding(form));
        assert!(settings(&mut model, SettingsEvent::SaveBranding).is_empty());
        assert_eq!(brand(&model).rejected.as_deref(), Some(issue.as_str()));
    }
    settings(
        &mut model,
        SettingsEvent::EditBranding(BrandingForm {
            business_name: " Agency ".into(),
            privacy_url: " http://example.com/privacy ".into(),
            ..valid.clone()
        }),
    );
    let effects = settings(&mut model, SettingsEvent::SaveBranding);
    assert_eq!(
        sent(&effects)[0].1,
        SchedulingAdminRequest::PatchBranding(SchedulingBrandingPatch {
            business_name: "Agency".into(),
            logo_height: 48,
            logo_opacity: 100,
            banner_opacity: 80,
            privacy_url: "http://example.com/privacy".into(),
            terms_url: "https://www.contract.test/terms".into(),
            fallback_locale: "en".into(),
        })
    );
    assert!(settings(&mut model, SettingsEvent::SaveBranding).is_empty());
    answer(&mut model, sent(&effects)[0].0, Err(server_error()));
    assert!(brand(&model).write.failure().is_some());
    settings(&mut model, SettingsEvent::EditBranding(valid.clone()));
    assert_eq!(brand(&model).write, WriteState::Idle);
    let effects = settings(&mut model, SettingsEvent::SaveBranding);
    let saved = SchedulingBranding {
        business_name: "Agency".into(),
        ..branding()
    };
    answer(
        &mut model,
        sent(&effects)[0].0,
        done(SchedulingAdminData::Branding(saved.clone())),
    );
    assert_eq!(
        brand(&model).write,
        WriteState::Done("Booking page saved".into())
    );
    assert_eq!(
        section(&model).branding.as_ref().and_then(|b| b.value()),
        Some(&saved)
    );
    let effects = settings(&mut model, SettingsEvent::SaveBranding);
    answer(&mut model, sent(&effects)[0].0, done(ok()));
    assert!(brand(&model).write.failure().is_some());
}

#[test]
fn the_logo_and_banner_save_as_soon_as_chosen_or_removed() {
    let mut model = read("agency");
    settings(&mut model, SettingsEvent::OpenBranding);
    // The profile's picture is not the booking page's.
    assert!(
        settings(
            &mut model,
            SettingsEvent::UploadImage {
                target: SchedulingUploadTarget::Avatar,
                file: png(1),
            },
        )
        .is_empty()
    );
    settings(
        &mut model,
        SettingsEvent::ImageUnreadable(SchedulingUploadTarget::Logo),
    );
    assert!(brand(&model).logo.failure().is_some());
    settings(
        &mut model,
        SettingsEvent::UploadImage {
            target: SchedulingUploadTarget::Banner,
            file: png(0),
        },
    );
    assert!(brand(&model).banner.failure().is_some());

    let effects = settings(
        &mut model,
        SettingsEvent::UploadImage {
            target: SchedulingUploadTarget::Logo,
            file: png(2),
        },
    );
    let SchedulingAdminRequest::Upload(upload) = sent(&effects)[0].1.clone() else {
        panic!("{effects:?}");
    };
    assert_eq!(
        (upload.target, upload.file_name.as_str()),
        (SchedulingUploadTarget::Logo, "logo.png")
    );
    assert!(brand(&model).busy());
    let reread = answer(
        &mut model,
        sent(&effects)[0].0,
        done(SchedulingAdminData::Uploaded(data(
            "district-scheduling-upload.json",
        ))),
    );
    assert_eq!(brand(&model).logo, WriteState::Done("Logo uploaded".into()));
    assert_eq!(sent(&reread)[0].1, SchedulingAdminRequest::Branding);
    let fresh = SchedulingBranding {
        banner_url: String::new(),
        ..branding()
    };
    answer(
        &mut model,
        sent(&reread)[0].0,
        done(SchedulingAdminData::Branding(fresh.clone())),
    );
    assert_eq!(brand(&model).branding, fresh);

    for (target, request, said) in [
        (
            SchedulingUploadTarget::Banner,
            SchedulingAdminRequest::DeleteBanner,
            "Banner removed",
        ),
        (
            SchedulingUploadTarget::Logo,
            SchedulingAdminRequest::DeleteLogo,
            "Logo removed",
        ),
    ] {
        let effects = settings(&mut model, SettingsEvent::RemoveImage(target));
        assert_eq!(sent(&effects)[0].1, request);
        let reread = answer(&mut model, sent(&effects)[0].0, done(ok()));
        let state = match target {
            SchedulingUploadTarget::Logo => &brand(&model).logo,
            _ => &brand(&model).banner,
        };
        assert_eq!(*state, WriteState::Done(said.into()));
        // A failed read again keeps what was shown.
        answer(&mut model, sent(&reread)[0].0, Err(server_error()));
        assert_eq!(brand(&model).branding, fresh);
    }
    let effects = settings(
        &mut model,
        SettingsEvent::RemoveImage(SchedulingUploadTarget::Logo),
    );
    answer(&mut model, sent(&effects)[0].0, refused("unavailable"));
    assert!(brand(&model).logo.failure().is_some());
}

fn assistant(model: &Model) -> &AssistantEditor {
    match section(model).dialog.as_ref() {
        Some(SettingsDialog::Assistant(editor)) => editor,
        other => panic!("{other:?}"),
    }
}

#[test]
fn the_booking_assistant_is_saved_only_when_changed() {
    let mut model = read("viewer");
    show(
        &mut model,
        SettingsTab::Assistant,
        SchedulingAdminData::Llm(llm()),
    );
    settings(&mut model, SettingsEvent::OpenAssistant);
    assert!(section(&model).dialog.is_none());

    let mut model = read("agency");
    settings(&mut model, SettingsEvent::OpenAssistant);
    assert!(section(&model).dialog.is_none());
    show(
        &mut model,
        SettingsTab::Assistant,
        SchedulingAdminData::Llm(llm()),
    );
    settings(&mut model, SettingsEvent::OpenAssistant);
    assert!(AssistantEditor::labels().iter().all(|l| !l.is_empty()));
    assert!(!assistant(&model).is_dirty());
    assert!(settings(&mut model, SettingsEvent::SaveAssistant).is_empty());
    settings(
        &mut model,
        SettingsEvent::EditAssistant {
            enabled: true,
            instructions: "x".repeat(4001),
        },
    );
    assert!(settings(&mut model, SettingsEvent::SaveAssistant).is_empty());
    assert_eq!(
        assistant(&model).rejected,
        Some("That is longer than 4000 characters.")
    );
    settings(
        &mut model,
        SettingsEvent::EditAssistant {
            enabled: false,
            instructions: "Ask for the address.".into(),
        },
    );
    assert_eq!(assistant(&model).rejected, None);
    let effects = settings(&mut model, SettingsEvent::SaveAssistant);
    assert_eq!(
        sent(&effects)[0].1,
        SchedulingAdminRequest::PatchLlm {
            enabled: Some(false),
            extra_instructions: Some("Ask for the address.".into()),
        }
    );
    assert!(settings(&mut model, SettingsEvent::SaveAssistant).is_empty());
    answer(&mut model, sent(&effects)[0].0, Err(server_error()));
    assert!(assistant(&model).write.failure().is_some());
    settings(
        &mut model,
        SettingsEvent::EditAssistant {
            enabled: false,
            instructions: "Ask for the address first.".into(),
        },
    );
    assert_eq!(assistant(&model).write, WriteState::Idle);
    let effects = settings(&mut model, SettingsEvent::SaveAssistant);
    let stored = SchedulingLlmSettings {
        enabled: false,
        extra_instructions: "Ask for the address first.".into(),
    };
    answer(
        &mut model,
        sent(&effects)[0].0,
        done(SchedulingAdminData::Llm(stored.clone())),
    );
    let editor = assistant(&model);
    assert_eq!(
        editor.write,
        WriteState::Done("Booking assistant saved".into())
    );
    assert!(!editor.is_dirty());
    assert_eq!(
        section(&model).assistant.as_ref().and_then(|a| a.value()),
        Some(&stored)
    );
    settings(
        &mut model,
        SettingsEvent::EditAssistant {
            enabled: true,
            instructions: String::new(),
        },
    );
    let effects = settings(&mut model, SettingsEvent::SaveAssistant);
    answer(&mut model, sent(&effects)[0].0, done(ok()));
    assert!(assistant(&model).write.failure().is_some());
}

#[test]
fn answers_for_a_dialog_that_closed_land_nowhere() {
    let mut model = read("agency");
    settings(&mut model, SettingsEvent::OpenBranding);
    let effects = settings(&mut model, SettingsEvent::SaveBranding);
    settings(&mut model, SettingsEvent::CloseDialog);
    answer(
        &mut model,
        sent(&effects)[0].0,
        done(SchedulingAdminData::Branding(branding())),
    );
    assert!(section(&model).dialog.is_none());
    // No dialog: its events do nothing.
    assert!(settings(&mut model, SettingsEvent::SaveBranding).is_empty());
}
