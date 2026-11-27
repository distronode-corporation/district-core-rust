// SPDX-FileCopyrightText: Distronode Corporation
//
// SPDX-License-Identifier: Apache-2.0

//! The business facts the receptionist answers from, read out of the service's
//! JSON into the forms step 1 and step 4 edit, and written back.
//!
//! The limits are the service's (`BUSINESS_FACTS_LIMITS`), so a save refused
//! here is one the route would refuse too; the route validates again.
//!
//! A section present in a save replaces the stored one whole, a list is sent in
//! full, and an explicit `null` clears it; an absent section is left alone.

use serde_json::{Map, Value, json};

/// The service's limits.
pub mod limit {
    /// An address line.
    pub const ADDRESS_LINE: usize = 200;
    /// A city or a region.
    pub const ADDRESS_PART: usize = 100;
    /// A postal code.
    pub const POSTAL_CODE: usize = 20;
    /// Opening periods in one day.
    pub const INTERVALS_PER_DAY: usize = 4;
    /// Services.
    pub const SERVICES: usize = 50;
    /// A service's name.
    pub const SERVICE_NAME: usize = 120;
    /// A service's price.
    pub const SERVICE_PRICE: usize = 40;
    /// Questions.
    pub const FAQS: usize = 50;
    /// A question.
    pub const FAQ_QUESTION: usize = 300;
    /// An answer.
    pub const FAQ_ANSWER: usize = 1000;
}

/// One service, as step 4 edits it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ServiceDraft {
    /// Its name.
    pub name: String,
    /// Its price as free text, read aloud as written; "" for none.
    pub price: String,
    /// Whether callers hear about it.
    pub enabled: bool,
}

/// One question and answer, as step 4 edits it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FaqDraft {
    /// The question.
    pub question: String,
    /// Its answer.
    pub answer: String,
    /// Whether the receptionist answers it.
    pub enabled: bool,
}

/// The business's address, as step 4 edits it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AddressDraft {
    /// The street line.
    pub line1: String,
    /// The city.
    pub city: String,
    /// The province, state or region.
    pub region: String,
    /// The postal code.
    pub postal_code: String,
    /// The two-letter country code.
    pub country: String,
}

impl AddressDraft {
    /// Whether every part is blank.
    pub fn is_empty(&self) -> bool {
        [
            &self.line1,
            &self.city,
            &self.region,
            &self.postal_code,
            &self.country,
        ]
        .iter()
        .all(|part| part.trim().is_empty())
    }
}

/// The days of the week, Monday first, as the service keys them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Weekday {
    /// `mon`.
    Mon,
    /// `tue`.
    Tue,
    /// `wed`.
    Wed,
    /// `thu`.
    Thu,
    /// `fri`.
    Fri,
    /// `sat`.
    Sat,
    /// `sun`.
    Sun,
}

impl Weekday {
    /// Every day, Monday first.
    pub const ALL: [Self; 7] = [
        Self::Mon,
        Self::Tue,
        Self::Wed,
        Self::Thu,
        Self::Fri,
        Self::Sat,
        Self::Sun,
    ];

    /// The day as the service keys it.
    pub fn key(self) -> &'static str {
        match self {
            Self::Mon => "mon",
            Self::Tue => "tue",
            Self::Wed => "wed",
            Self::Thu => "thu",
            Self::Fri => "fri",
            Self::Sat => "sat",
            Self::Sun => "sun",
        }
    }
}

/// Whether a day has hours.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum DayState {
    /// Nothing said: the receptionist says nothing about the day. Not the
    /// same as closed.
    #[default]
    NotSet,
    /// Closed all day.
    Closed,
    /// Open in its periods.
    Open,
}

/// One opening period, in minutes after midnight. `close` may be 1440
/// ("24:00").
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Interval {
    /// When it opens.
    pub open: u16,
    /// When it closes.
    pub close: u16,
}

impl Interval {
    /// Nine to five, the period a day switched to open starts with.
    pub const STANDARD: Self = Self {
        open: 9 * 60,
        close: 17 * 60,
    };

    /// "09:30" is 570 and "24:00" is 1440; anything else is `None`.
    pub fn minutes(text: &str) -> Option<u16> {
        if text == "24:00" {
            return Some(24 * 60);
        }
        let (hours, minutes) = text.split_once(':')?;
        let two_digits = |part: &str| part.len() == 2 && part.chars().all(|c| c.is_ascii_digit());
        if !two_digits(hours) || !two_digits(minutes) {
            return None;
        }
        let (hours, minutes): (u16, u16) = (hours.parse().ok()?, minutes.parse().ok()?);
        (hours < 24 && minutes < 60).then_some(hours * 60 + minutes)
    }

    /// `minutes` as the service writes it: "09:30", "24:00".
    pub fn text(minutes: u16) -> String {
        format!("{:02}:{:02}", minutes / 60, minutes % 60)
    }
}

/// One day of opening hours, as step 4 edits it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DayDraft {
    /// Whether it has hours.
    pub state: DayState,
    /// Its periods, when open.
    pub intervals: Vec<Interval>,
}

/// The week, one entry per [`Weekday::ALL`] day, in that order.
pub type Week = [DayDraft; 7];

fn text_of<'a>(row: &'a Map<String, Value>, key: &str) -> &'a str {
    row.get(key).and_then(Value::as_str).unwrap_or_default()
}

fn rows<'a>(facts: Option<&'a Map<String, Value>>, key: &str) -> Vec<&'a Map<String, Value>> {
    facts
        .and_then(|facts| facts.get(key))
        .and_then(Value::as_array)
        .map(|rows| rows.iter().filter_map(Value::as_object).collect())
        .unwrap_or_default()
}

/// The stored services.
pub fn services(facts: Option<&Map<String, Value>>) -> Vec<ServiceDraft> {
    rows(facts, "services")
        .into_iter()
        .map(|row| ServiceDraft {
            name: text_of(row, "name").to_owned(),
            price: text_of(row, "price").to_owned(),
            enabled: row.get("enabled") == Some(&Value::Bool(true)),
        })
        .collect()
}

/// The stored questions.
pub fn faqs(facts: Option<&Map<String, Value>>) -> Vec<FaqDraft> {
    rows(facts, "faqs")
        .into_iter()
        .map(|row| FaqDraft {
            question: text_of(row, "question").to_owned(),
            answer: text_of(row, "answer").to_owned(),
            enabled: row.get("enabled") == Some(&Value::Bool(true)),
        })
        .collect()
}

/// The stored address.
pub fn address(facts: Option<&Map<String, Value>>) -> AddressDraft {
    let empty = Map::new();
    let address = facts
        .and_then(|facts| facts.get("address"))
        .and_then(Value::as_object)
        .unwrap_or(&empty);
    AddressDraft {
        line1: text_of(address, "line1").to_owned(),
        city: text_of(address, "city").to_owned(),
        region: text_of(address, "region").to_owned(),
        postal_code: text_of(address, "postalCode").to_owned(),
        country: text_of(address, "country").to_owned(),
    }
}

/// The stored time zone, or "".
pub fn timezone(facts: Option<&Map<String, Value>>) -> String {
    facts
        .and_then(|facts| facts.get("timezone"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

/// Every weekday of `weekly_hours`, not set where it says nothing.
pub fn hours(weekly_hours: Option<&Value>) -> Week {
    Weekday::ALL.map(|day| {
        let stored = weekly_hours.and_then(|week| week.get(day.key()));
        match stored {
            Some(Value::String(closed)) if closed == "closed" => DayDraft {
                state: DayState::Closed,
                intervals: Vec::new(),
            },
            Some(Value::Array(rows)) if !rows.is_empty() => DayDraft {
                state: DayState::Open,
                intervals: rows
                    .iter()
                    .filter_map(|row| {
                        let open = Interval::minutes(row.get("open")?.as_str()?)?;
                        let close = Interval::minutes(row.get("close")?.as_str()?)?;
                        Some(Interval { open, close })
                    })
                    .collect(),
            },
            _ => DayDraft::default(),
        }
    })
}

/// The "what you do" line: the answer to the question step 1 writes, in either
/// language, or "".
pub fn what(facts: Option<&Map<String, Value>>) -> String {
    rows(facts, "faqs")
        .into_iter()
        .find(|row| super::wire::is_what_question(text_of(row, "question")))
        .map(|row| text_of(row, "answer").to_owned())
        .unwrap_or_default()
}

/// The services section, `null` when there are none.
pub fn services_json(services: &[ServiceDraft]) -> Value {
    if services.is_empty() {
        return Value::Null;
    }
    Value::Array(
        services
            .iter()
            .map(|service| {
                let mut row = Map::new();
                row.insert("name".into(), service.name.trim().into());
                let price = service.price.trim();
                if !price.is_empty() {
                    row.insert("price".into(), price.into());
                }
                row.insert("enabled".into(), service.enabled.into());
                Value::Object(row)
            })
            .collect(),
    )
}

/// The questions section, `null` when there are none.
pub fn faqs_json(faqs: &[FaqDraft]) -> Value {
    if faqs.is_empty() {
        return Value::Null;
    }
    Value::Array(
        faqs.iter()
            .map(|faq| {
                json!({
                    "question": faq.question.trim(),
                    "answer": faq.answer.trim(),
                    "enabled": faq.enabled,
                })
            })
            .collect(),
    )
}

/// The address section. An empty address is a clear (`null`), and a blank
/// optional part is left out, never sent as "".
pub fn address_json(address: &AddressDraft) -> Value {
    if address.is_empty() {
        return Value::Null;
    }
    let mut row = Map::new();
    row.insert("city".into(), address.city.trim().into());
    row.insert(
        "country".into(),
        address.country.trim().to_ascii_uppercase().into(),
    );
    for (key, value) in [
        ("line1", &address.line1),
        ("region", &address.region),
        ("postalCode", &address.postal_code),
    ] {
        if !value.trim().is_empty() {
            row.insert(key.into(), value.trim().into());
        }
    }
    Value::Object(row)
}

/// The hours section. A day not set is absent, which is not closed; no day set
/// at all clears the section.
pub fn hours_json(week: &Week) -> Value {
    let mut out = Map::new();
    for (day, draft) in Weekday::ALL.iter().zip(week) {
        match draft.state {
            DayState::NotSet => {}
            DayState::Closed => {
                out.insert(day.key().into(), "closed".into());
            }
            DayState::Open => {
                let mut periods = draft.intervals.clone();
                periods.sort_by_key(|period| period.open);
                out.insert(
                    day.key().into(),
                    Value::Array(
                        periods
                            .iter()
                            .map(|period| {
                                json!({
                                    "open": Interval::text(period.open),
                                    "close": Interval::text(period.close),
                                })
                            })
                            .collect(),
                    ),
                );
            }
        }
    }
    if out.is_empty() {
        Value::Null
    } else {
        Value::Object(out)
    }
}

/// `incoming` appended to `current` by a case-insensitive `key`, duplicates
/// skipped, up to `max` (the web's `appendUnique`). The stored rows go back as
/// they came, every key they carry, because the section is replaced whole; the
/// incoming ones keep the lookup's `enabled: false`.
pub fn appending(incoming: Vec<Value>, current: &[Value], key: &str, max: usize) -> Vec<Value> {
    let normalise = |row: &Value| {
        row.get(key)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_lowercase()
    };
    let mut seen: Vec<String> = current.iter().map(normalise).collect();
    let mut out = current.to_vec();
    for row in incoming {
        if out.len() >= max {
            break;
        }
        let name = normalise(&row);
        if seen.contains(&name) {
            continue;
        }
        seen.push(name);
        out.push(row);
    }
    out
}

/// Whether a service can be saved.
pub fn service_is_valid(service: &ServiceDraft) -> bool {
    let name = service.name.trim();
    !name.is_empty()
        && name.chars().count() <= limit::SERVICE_NAME
        && service.price.trim().chars().count() <= limit::SERVICE_PRICE
}

/// Whether a question can be saved.
pub fn faq_is_valid(faq: &FaqDraft) -> bool {
    let question = faq.question.trim();
    let answer = faq.answer.trim();
    !question.is_empty()
        && question.chars().count() <= limit::FAQ_QUESTION
        && !answer.is_empty()
        && answer.chars().count() <= limit::FAQ_ANSWER
}

/// Whether an address can be saved: empty, or at least a city and a two-letter
/// country code, each part within its limit.
pub fn address_is_valid(address: &AddressDraft) -> bool {
    if address.is_empty() {
        return true;
    }
    let country = address.country.trim();
    let within = |text: &str, max: usize| text.trim().chars().count() <= max;
    !address.city.trim().is_empty()
        && country.len() == 2
        && country.chars().all(|c| c.is_ascii_alphabetic())
        && within(&address.line1, limit::ADDRESS_LINE)
        && within(&address.city, limit::ADDRESS_PART)
        && within(&address.region, limit::ADDRESS_PART)
        && within(&address.postal_code, limit::POSTAL_CODE)
}

/// Whether a day can be saved: one to four periods, each closing after it
/// opens, none overlapping (`DayHoursSchema`).
pub fn day_is_valid(day: &DayDraft) -> bool {
    if day.state != DayState::Open {
        return true;
    }
    if day.intervals.is_empty() || day.intervals.len() > limit::INTERVALS_PER_DAY {
        return false;
    }
    let mut sorted = day.intervals.clone();
    sorted.sort_by_key(|period| period.open);
    sorted.iter().all(|period| period.close > period.open)
        && sorted.windows(2).all(|pair| pair[1].open >= pair[0].close)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(value: Value) -> Map<String, Value> {
        value.as_object().cloned().unwrap()
    }

    #[test]
    fn the_stored_facts_read_into_the_forms() {
        let stored = facts(json!({
            "timezone": "America/Toronto",
            "address": {"line1": "100 Example Street", "city": "Toronto", "region": "ON",
                        "postalCode": "M5V 1A1", "country": "CA"},
            "services": [{"name": "Cleaning", "price": "$120", "enabled": true},
                         {"name": "Whitening", "enabled": false}, "not a row"],
            "faqs": [{"question": "What does the business do?", "answer": "Teeth.", "enabled": true},
                     {"question": "Parking?", "answer": "Yes.", "enabled": "yes"}],
        }));
        assert_eq!(timezone(Some(&stored)), "America/Toronto");
        assert_eq!(address(Some(&stored)).postal_code, "M5V 1A1");
        assert_eq!(
            services(Some(&stored)),
            [
                ServiceDraft {
                    name: "Cleaning".into(),
                    price: "$120".into(),
                    enabled: true
                },
                ServiceDraft {
                    name: "Whitening".into(),
                    price: String::new(),
                    enabled: false
                },
            ]
        );
        let questions = faqs(Some(&stored));
        assert_eq!(questions.len(), 2);
        assert!(!questions[1].enabled, "only `true` is on");
        assert_eq!(what(Some(&stored)), "Teeth.");
        assert_eq!(what(None), "");
        assert_eq!(timezone(None), "");
        assert!(address(None).is_empty());
        assert!(services(None).is_empty());
    }

    #[test]
    fn hours_read_each_day_as_not_set_closed_or_open() {
        let week = hours(Some(&json!({
            "mon": [{"open": "09:00", "close": "17:00"}, {"open": "bad", "close": "18:00"},
                    {"close": "18:00"}],
            "tue": [],
            "sun": "closed",
            "sat": "later",
        })));
        assert_eq!(week[0].state, DayState::Open);
        assert_eq!(week[0].intervals, [Interval::STANDARD]);
        assert_eq!(week[1], DayDraft::default());
        assert_eq!(week[5], DayDraft::default());
        assert_eq!(week[6].state, DayState::Closed);
        assert!(hours(None).iter().all(|day| *day == DayDraft::default()));
    }

    #[test]
    fn times_are_read_and_written_as_the_service_does() {
        assert_eq!(Interval::minutes("09:30"), Some(570));
        assert_eq!(Interval::minutes("24:00"), Some(1440));
        assert_eq!(Interval::minutes("24:30"), None);
        assert_eq!(Interval::minutes("12:60"), None);
        assert_eq!(Interval::minutes("9:30"), None);
        assert_eq!(Interval::minutes("09:3"), None);
        assert_eq!(Interval::minutes("0930"), None);
        assert_eq!(Interval::minutes("ab:cd"), None);
        assert_eq!(Interval::text(570), "09:30");
        assert_eq!(Interval::text(1440), "24:00");
        let days: Vec<&str> = Weekday::ALL.iter().map(|day| day.key()).collect();
        assert_eq!(days, ["mon", "tue", "wed", "thu", "fri", "sat", "sun"]);
    }

    #[test]
    fn sections_are_written_whole_and_an_empty_one_is_a_clear() {
        assert_eq!(services_json(&[]), Value::Null);
        assert_eq!(
            services_json(&[
                ServiceDraft {
                    name: " Cleaning ".into(),
                    price: " $1 ".into(),
                    enabled: true
                },
                ServiceDraft {
                    name: "Free".into(),
                    price: " ".into(),
                    enabled: false
                },
            ]),
            json!([{"name": "Cleaning", "price": "$1", "enabled": true},
                   {"name": "Free", "enabled": false}])
        );
        assert_eq!(faqs_json(&[]), Value::Null);
        assert_eq!(
            faqs_json(&[FaqDraft {
                question: " Q ".into(),
                answer: " A ".into(),
                enabled: true
            }]),
            json!([{"question": "Q", "answer": "A", "enabled": true}])
        );
        assert_eq!(address_json(&AddressDraft::default()), Value::Null);
        assert_eq!(
            address_json(&AddressDraft {
                city: " Toronto ".into(),
                country: "ca".into(),
                region: "ON".into(),
                ..AddressDraft::default()
            }),
            json!({"city": "Toronto", "country": "CA", "region": "ON"})
        );
        let mut week: Week = Default::default();
        assert_eq!(hours_json(&week), Value::Null);
        week[0] = DayDraft {
            state: DayState::Open,
            intervals: vec![
                Interval {
                    open: 780,
                    close: 1020,
                },
                Interval {
                    open: 540,
                    close: 720,
                },
            ],
        };
        week[6].state = DayState::Closed;
        assert_eq!(
            hours_json(&week),
            json!({"mon": [{"open": "09:00", "close": "12:00"}, {"open": "13:00", "close": "17:00"}],
                   "sun": "closed"})
        );
    }

    #[test]
    fn appending_skips_duplicates_and_stops_at_the_cap() {
        let current = vec![json!({"name": "Cleaning", "kept": 1})];
        let incoming = vec![
            json!({"name": " cleaning "}),
            json!({"name": "Whitening"}),
            json!({"name": "Braces"}),
            json!({"name": "whitening"}),
        ];
        assert_eq!(
            appending(incoming.clone(), &current, "name", 10),
            [
                json!({"name": "Cleaning", "kept": 1}),
                json!({"name": "Whitening"}),
                json!({"name": "Braces"})
            ]
        );
        assert_eq!(appending(incoming, &current, "name", 2).len(), 2);
    }

    #[test]
    fn the_checks_are_the_services() {
        let long = |n: usize| "x".repeat(n);
        assert!(service_is_valid(&ServiceDraft {
            name: "A".into(),
            ..Default::default()
        }));
        assert!(!service_is_valid(&ServiceDraft::default()));
        assert!(!service_is_valid(&ServiceDraft {
            name: long(121),
            ..Default::default()
        }));
        assert!(!service_is_valid(&ServiceDraft {
            name: "A".into(),
            price: long(41),
            enabled: true
        }));
        assert!(faq_is_valid(&FaqDraft {
            question: "Q".into(),
            answer: "A".into(),
            enabled: true
        }));
        assert!(!faq_is_valid(&FaqDraft {
            question: " ".into(),
            answer: "A".into(),
            enabled: true
        }));
        assert!(!faq_is_valid(&FaqDraft {
            question: long(301),
            answer: "A".into(),
            enabled: true
        }));
        assert!(!faq_is_valid(&FaqDraft {
            question: "Q".into(),
            answer: String::new(),
            enabled: true
        }));
        assert!(!faq_is_valid(&FaqDraft {
            question: "Q".into(),
            answer: long(1001),
            enabled: true
        }));
        let place = AddressDraft {
            city: "Toronto".into(),
            country: "CA".into(),
            ..Default::default()
        };
        assert!(address_is_valid(&AddressDraft::default()));
        assert!(address_is_valid(&place));
        assert!(!address_is_valid(&AddressDraft {
            city: String::new(),
            ..place.clone()
        }));
        assert!(!address_is_valid(&AddressDraft {
            country: "CAN".into(),
            ..place.clone()
        }));
        assert!(!address_is_valid(&AddressDraft {
            country: "C1".into(),
            ..place.clone()
        }));
        assert!(!address_is_valid(&AddressDraft {
            line1: long(201),
            ..place.clone()
        }));
        assert!(!address_is_valid(&AddressDraft {
            city: long(101),
            ..place.clone()
        }));
        assert!(!address_is_valid(&AddressDraft {
            region: long(101),
            ..place.clone()
        }));
        assert!(!address_is_valid(&AddressDraft {
            postal_code: long(21),
            ..place
        }));
        let open = |intervals: Vec<Interval>| DayDraft {
            state: DayState::Open,
            intervals,
        };
        assert!(day_is_valid(&DayDraft::default()));
        assert!(day_is_valid(&open(vec![Interval::STANDARD])));
        assert!(!day_is_valid(&open(Vec::new())));
        assert!(!day_is_valid(&open(vec![Interval::STANDARD; 5])));
        assert!(!day_is_valid(&open(vec![Interval {
            open: 600,
            close: 600
        }])));
        assert!(!day_is_valid(&open(vec![
            Interval {
                open: 540,
                close: 720
            },
            Interval {
                open: 700,
                close: 800
            },
        ])));
        assert!(day_is_valid(&open(vec![
            Interval {
                open: 780,
                close: 900
            },
            Interval {
                open: 540,
                close: 780
            },
        ])));
    }
}
