//! Event types and what hangs off one: its hosts, its booking questions, the
//! times it offers and the test email.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::envelope::nullish;

/// The location types the catalog lets an event type be given, in its order.
/// Seven: the scheduler accepts `zoom` too, and the catalog withholds it, so it
/// is not offered here.
pub const SCHEDULING_LOCATION_TYPES: [&str; 7] = [
    "google_meet",
    "teams",
    "custom_video",
    "phone",
    "in_person",
    "link",
    "livekit",
];

/// What an event type's `location_value` holds, which depends on its location
/// type.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SchedulingLocationValueKind {
    /// The scheduler makes the meeting link itself: no value to ask for.
    Generated,
    /// A link, the one kind the service checks.
    Url,
    /// A phone number.
    Phone,
    /// A street address.
    Address,
}

/// What `location_type` asks of `location_value`, or `None` for a type this
/// build does not know (shown as it came, never replaced).
pub fn location_value_kind(location_type: &str) -> Option<SchedulingLocationValueKind> {
    Some(match location_type {
        "google_meet" | "teams" | "livekit" => SchedulingLocationValueKind::Generated,
        "custom_video" | "link" => SchedulingLocationValueKind::Url,
        "phone" => SchedulingLocationValueKind::Phone,
        "in_person" => SchedulingLocationValueKind::Address,
        _ => return None,
    })
}

/// An event type: a kind of meeting someone can book.
///
/// A key the scheduler may leave out or send as `null` is an
/// `Option<Option<_>>`: `None` left out, `Some(None)` null. The flat accessors
/// read both as no value.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
pub struct SchedulingEventType {
    /// Its id.
    pub id: String,
    /// Its slug, which every operation on it addresses it by.
    pub slug: String,
    /// Its name.
    pub name: String,
    /// What the booking page says about it.
    #[serde(
        default,
        deserialize_with = "nullish",
        skip_serializing_if = "Option::is_none"
    )]
    pub description: Option<Option<String>>,
    /// How long a meeting lasts.
    pub duration_minutes: i64,
    /// How far apart the offered start times are.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slot_interval_minutes: Option<i64>,
    /// Where it happens: one of [`SCHEDULING_LOCATION_TYPES`], or a type stored
    /// before the list narrowed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location_type: Option<String>,
    /// The phone number, address or link, for a location type that takes one.
    #[serde(
        default,
        deserialize_with = "nullish",
        skip_serializing_if = "Option::is_none"
    )]
    pub location_value: Option<Option<String>>,
    /// `fixed`, `round_robin` or `collective`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub routing_mode: Option<String>,
    /// For round robin: `even`, `soonest` or `priority`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rr_strategy: Option<String>,
    /// Kept free before a meeting.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub buffer_before_minutes: Option<i64>,
    /// Kept free after a meeting.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub buffer_after_minutes: Option<i64>,
    /// How soon before a meeting it can still be booked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_notice_minutes: Option<i64>,
    /// How far ahead it can be booked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_future_days: Option<i64>,
    /// How many bookings one person may hold at once; 0 for no limit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_active_bookings: Option<i64>,
    /// Whether it can be booked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_active: Option<bool>,
    /// Whether the booking page shows the taken times too.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub show_taken_slots: Option<bool>,
    /// Whether it is listed on the booking page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_public: Option<bool>,
    /// When it was made.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    /// The confirmation email's text.
    #[serde(
        default,
        deserialize_with = "nullish",
        skip_serializing_if = "Option::is_none"
    )]
    pub msg_confirmation: Option<Option<String>>,
    /// The cancellation email's text.
    #[serde(
        default,
        deserialize_with = "nullish",
        skip_serializing_if = "Option::is_none"
    )]
    pub msg_cancellation: Option<Option<String>>,
    /// The reschedule email's text.
    #[serde(
        default,
        deserialize_with = "nullish",
        skip_serializing_if = "Option::is_none"
    )]
    pub msg_reschedule: Option<Option<String>>,
    /// The reminder email's text.
    #[serde(
        default,
        deserialize_with = "nullish",
        skip_serializing_if = "Option::is_none"
    )]
    pub msg_reminder: Option<Option<String>>,
    /// The booking page's greeting.
    #[serde(
        default,
        deserialize_with = "nullish",
        skip_serializing_if = "Option::is_none"
    )]
    pub msg_greeting: Option<Option<String>>,
    /// The confirmation email's subject.
    #[serde(
        default,
        deserialize_with = "nullish",
        skip_serializing_if = "Option::is_none"
    )]
    pub subj_confirmation: Option<Option<String>>,
    /// The cancellation email's subject.
    #[serde(
        default,
        deserialize_with = "nullish",
        skip_serializing_if = "Option::is_none"
    )]
    pub subj_cancellation: Option<Option<String>>,
    /// The reschedule email's subject.
    #[serde(
        default,
        deserialize_with = "nullish",
        skip_serializing_if = "Option::is_none"
    )]
    pub subj_reschedule: Option<Option<String>>,
    /// The reminder email's subject.
    #[serde(
        default,
        deserialize_with = "nullish",
        skip_serializing_if = "Option::is_none"
    )]
    pub subj_reminder: Option<Option<String>>,
    /// When reminders go out, in minutes before the meeting.
    #[serde(
        default,
        deserialize_with = "nullish",
        skip_serializing_if = "Option::is_none"
    )]
    pub reminders: Option<Option<Vec<i64>>>,
    /// Whether it was archived.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archived: Option<bool>,
    /// Whether the signed-in member owns it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owned: Option<bool>,
    /// Its owner's name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_name: Option<String>,
    /// Its owner's email address.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_email: Option<String>,
}

/// A key left out and a key sent as `null` read alike.
fn flat(value: &Option<Option<String>>) -> Option<&str> {
    value.as_ref().and_then(|value| value.as_deref())
}

impl SchedulingEventType {
    /// The description, whether it was left out or `null` being no value.
    pub fn description(&self) -> Option<&str> {
        flat(&self.description)
    }

    /// The location's value, the same way.
    pub fn location_value(&self) -> Option<&str> {
        flat(&self.location_value)
    }

    /// The reminders, the same way.
    pub fn reminders(&self) -> &[i64] {
        self.reminders
            .as_ref()
            .and_then(|value| value.as_deref())
            .unwrap_or_default()
    }

    /// One of the eight email texts by its wire key (`msg_confirmation`,
    /// `subj_reminder`, ...), the same way; `None` for any other key.
    pub fn message(&self, key: &str) -> Option<&str> {
        flat(match key {
            "msg_confirmation" => &self.msg_confirmation,
            "msg_cancellation" => &self.msg_cancellation,
            "msg_reschedule" => &self.msg_reschedule,
            "msg_reminder" => &self.msg_reminder,
            "msg_greeting" => &self.msg_greeting,
            "subj_confirmation" => &self.subj_confirmation,
            "subj_cancellation" => &self.subj_cancellation,
            "subj_reschedule" => &self.subj_reschedule,
            "subj_reminder" => &self.subj_reminder,
            _ => return None,
        })
    }
}

/// One host of an event type.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
pub struct SchedulingHost {
    /// The host's scheduler user id.
    pub user_id: String,
    /// Their name.
    pub name: String,
    /// Their email address.
    pub email: String,
    /// Their picture.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avatar_url: Option<String>,
    /// `required`, `rotation` or `optional`.
    pub role: String,
    /// Their place in the rotation, lowest first.
    pub priority: i64,
    /// Whether they were archived. Still on the event type: the scheduler keeps
    /// the link.
    pub archived: bool,
}

/// One question the booking page asks.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
pub struct SchedulingQuestion {
    /// Its id.
    pub id: String,
    /// The event type's id.
    pub event_type_id: String,
    /// The question.
    pub label: String,
    /// `text`, `select` or `checkbox`.
    #[serde(rename = "type")]
    pub kind: String,
    /// The choices of a `select`.
    #[serde(
        default,
        deserialize_with = "nullish",
        skip_serializing_if = "Option::is_none"
    )]
    pub options: Option<Option<Vec<String>>>,
    /// Whether it must be answered.
    pub required: bool,
    /// Its place in the list, from 0.
    pub position: i64,
}

impl SchedulingQuestion {
    /// The choices, whether they were left out or `null` being none.
    pub fn options(&self) -> &[String] {
        self.options
            .as_ref()
            .and_then(|value| value.as_deref())
            .unwrap_or_default()
    }
}

/// One bookable (or taken) time.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
pub struct SchedulingSlot {
    /// When it starts, RFC 3339: the value a reschedule sends back as it came.
    pub start: String,
    /// When it ends.
    pub end: String,
    /// Who could take it.
    #[serde(
        default,
        deserialize_with = "nullish",
        skip_serializing_if = "Option::is_none"
    )]
    pub host_ids: Option<Option<Vec<String>>>,
}

/// `eventTypes.slots`: the times an event type offers in a window.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
pub struct SchedulingSlots {
    /// The free times.
    pub slots: Vec<SchedulingSlot>,
    /// The hosts the slots name, by scheduler user id: each `{name,
    /// avatar_url}`. Kept as it came: a map keyed by data, which no screen here
    /// reads.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hosts: Option<BTreeMap<String, Value>>,
    /// The taken times, only for an event type that shows them.
    #[serde(
        default,
        deserialize_with = "nullish",
        skip_serializing_if = "Option::is_none"
    )]
    pub taken: Option<Option<Vec<SchedulingSlot>>>,
}

/// `eventTypes.testEmail`: the test email went, and to whom.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
pub struct SchedulingTestEmail {
    /// Whether it went.
    pub sent: bool,
    /// The address it went to: the signed-in member's own.
    pub to: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_location_type_says_what_its_value_is() {
        let kinds: Vec<_> = SCHEDULING_LOCATION_TYPES
            .iter()
            .map(|t| location_value_kind(t))
            .collect();
        use SchedulingLocationValueKind::*;
        assert_eq!(
            kinds,
            [
                Some(Generated),
                Some(Generated),
                Some(Url),
                Some(Phone),
                Some(Address),
                Some(Url),
                Some(Generated)
            ]
        );
        assert_eq!(location_value_kind("zoom"), None);
    }

    #[test]
    fn left_out_and_null_read_alike() {
        let row: SchedulingEventType = serde_json::from_str(
            r#"{"id":"e","slug":"s","name":"n","duration_minutes":30,
                "description":null,"msg_greeting":"Hi","reminders":[60]}"#,
        )
        .unwrap();
        assert_eq!(row.description, Some(None));
        assert_eq!(row.description(), None);
        assert_eq!(row.location_value, None);
        assert_eq!(row.location_value(), None);
        assert_eq!(row.reminders(), [60]);
        assert_eq!(row.message("msg_greeting"), Some("Hi"));
        assert_eq!(row.message("msg_reminder"), None);
        assert_eq!(row.message("name"), None);
        for key in [
            "msg_confirmation",
            "msg_cancellation",
            "msg_reschedule",
            "subj_confirmation",
            "subj_cancellation",
            "subj_reschedule",
            "subj_reminder",
        ] {
            assert_eq!(row.message(key), None, "{key}");
        }
        assert_eq!(SchedulingEventType::default().reminders(), [] as [i64; 0]);
        let question = SchedulingQuestion {
            options: Some(Some(vec!["A".into()])),
            ..SchedulingQuestion::default()
        };
        assert_eq!(question.options(), ["A"]);
        assert!(SchedulingQuestion::default().options().is_empty());
    }
}
