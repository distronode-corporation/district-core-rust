//! The dialler: the country on its button, the number as it is typed, the line
//! that says where it rings, and placing the call. Dialler design A, after
//! district-ios (and the web and Mac dialers, which follow the same rules).
//!
//! The number is typed the local way under a flag and calling code the member
//! can see and change, and it is formatted as it is typed; a number that names
//! its own country (`+`, `011` from a +1 country, `00` elsewhere) moves the
//! flag, and the field keeps only the local form. Before Call, a line reads
//! "Rings Canada · +1 416 555 0199", and that E.164 number is exactly what the
//! service is sent ([`plan`] has the rules). The service still normalises its
//! own copy before its do-not-call check, and an E.164 number passes through
//! that unchanged, so the check sees the number the member was shown.
//!
//! An impossible length keeps Call off, and the sentence saying so waits until
//! the member stops typing: the field loses focus, Enter, or a refused Call
//! ([`DialerEvent::CheckLength`]). Every ten-digit number passes through eight
//! and nine digits on the way, so a sentence said while typing would flash on
//! every number. The next edit clears it. Emergency numbers are refused before
//! any country is asked for.
//!
//! The home country is the country most of the workspace's own numbers belong
//! to, read from the numbers it holds (the list the phone numbers screen
//! reads), as the iOS app does. No field of any response was added for it. A
//! workspace with no numbers has no home country: the member picks one, which
//! holds for the rest of the visit to that workspace, or types `+`.
//!
//! Placing a call is the one thing in the app whose mistake rings a stranger's
//! telephone and costs money, so it happens on the member's press and nowhere
//! else, one at a time, and it is never sent again by itself: the service
//! writes the call and tells the carrier before it answers, so a request sent
//! twice is two calls. A viewer is not offered the dialler at all
//! ([`Capabilities::can_dial`](crate::Capabilities::can_dial)).

mod countries;
pub mod plan;

pub use countries::{COUNTRIES, DialCountry};
pub use plan::{
    DialReading, DialStatus, MAX_DIAL_DIGITS, calling_code, country_name, flag, home_country,
    national_example, read_dial_entry, search_countries, sorted_countries, suggested_countries,
};

use crate::call::ActiveCall;
use crate::marketplace::OwnedNumbersList;
use crate::model::{Effect, Slot, Tickets};
use crate::signed_in::{Next, SignedIn, stay};
use crate::text::{l, s};

/// The fewest digits the dialler places a call to, the calling code counted.
/// The service refuses a number shorter than eight characters once it has
/// normalised it, so this can only keep the button off, never place a call the
/// service would refuse for its length.
pub const MIN_DIAL_DIGITS: usize = 8;

/// A group of digits.
const GROUP: usize = 3;
/// A North American number with its area code: ten digits. Where the spaces go,
/// and nothing else.
const SUBSCRIBER: usize = 10;

/// The dialler's screen.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DialerScreen {
    /// What the field shows: the number in the local form of the country on
    /// the button, formatted (`(416) 555-0199`). The core writes it after
    /// every edit, so the shell writes it back into its field.
    pub entry: String,
    /// The country on the button (ISO 3166), or `None` until the workspace's
    /// home country is known, one is picked, or a `+` names one.
    pub country: Option<String>,
    /// The workspace's own country, from its numbers, or `None`: not read yet,
    /// or no number of the workspace names one.
    pub home: Option<String>,
    /// Whether the member has stopped typing since the last edit, so a wrong
    /// length may be said ([`DialerScreen::problem`]).
    pub length_checked: bool,
}

impl DialerScreen {
    /// The note that placing a call turns the microphone on.
    pub const MICROPHONE_NOTE: &'static str = district_l10n::english::DIALER_MICROPHONE_NOTE;
    /// The note for a member who cannot place a call right now.
    pub const BUSY_NOTE: &'static str = district_l10n::english::DIALER_BUSY_NOTE;
    /// The field's placeholder with no country on the button: a shape, not a
    /// country, because only an international number can be read then.
    pub const PLACEHOLDER_INTERNATIONAL: &'static str = "+\u{2026}";

    /// The field, read in the country on the button.
    pub fn reading(&self) -> DialReading {
        read_dial_entry(&self.entry, self.country.as_deref())
    }

    /// The line under the field: how to type a number in the country on the
    /// button, or to choose one.
    pub fn hint(&self) -> String {
        match self.country.as_deref() {
            Some(iso) => s::dialer_hint_local(l(), country_name(iso, l())),
            None => s::dialer_needs_country(l()).to_owned(),
        }
    }

    /// The field's placeholder: an example national number for the country on
    /// the button, written without its calling code, or empty for a country
    /// with no example; with no country, [`Self::PLACEHOLDER_INTERNATIONAL`].
    pub fn placeholder(&self) -> &'static str {
        match self.country.as_deref() {
            Some(iso) => national_example(iso).unwrap_or(""),
            None => Self::PLACEHOLDER_INTERNATIONAL,
        }
    }

    /// The line shown before Call: where the call rings and the whole number
    /// that is sent, "Rings Canada · +1 416 555 0199". This line is the guard
    /// against a wrong-country dial: it is what the member reads in the second
    /// before pressing Call, and its number is exactly the one the service
    /// receives.
    ///
    /// An unknown calling code says so plainly rather than guessing: naming the
    /// wrong country is worse than naming none, because a wrong name is read as
    /// confirmation. `None` until the number is complete, when there is nothing
    /// to send yet; the button already names the country while it is typed.
    pub fn destination(&self) -> Option<String> {
        let reading = self.reading();
        if reading.status != DialStatus::Ready {
            // A `+` and three digits that name no country is already worth
            // saying.
            let unknown = reading.status == DialStatus::Typing
                && reading.named_its_country
                && reading.iso.is_none()
                && reading.national.len() > 3;
            return unknown.then(|| s::dialer_unrecognised(l()).to_owned());
        }
        Some(match reading.iso {
            Some(iso) => s::dialer_rings(l(), country_name(iso, l()), &reading.international),
            None => s::dialer_unrecognised(l()).to_owned(),
        })
    }

    /// Why Call is off, in the member's terms, or `None` when there is nothing
    /// to correct. An empty field says nothing (the hint already speaks then),
    /// and neither does a number still being typed: a wrong length is said only
    /// once the member stops ([`DialerScreen::length_checked`]). An incomplete
    /// number is a wrong length only with a country and digits to measure: a
    /// lone `+`, a trunk `0` or an unknown code (which the line above names) is
    /// not.
    ///
    /// The emergency sentence is the one that is not a correction: it sends the
    /// member to their own phone, because District AI cannot place an emergency
    /// call and must not seem as if it might.
    pub fn problem(&self) -> Option<String> {
        let reading = self.reading();
        let wrong_length = match reading.status {
            DialStatus::Empty | DialStatus::Ready => false,
            DialStatus::NeedsCountry => return Some(s::dialer_needs_country(l()).to_owned()),
            DialStatus::Emergency => return Some(s::dialer_emergency(l()).to_owned()),
            DialStatus::Typing => reading.iso.is_some() && !reading.national_number.is_empty(),
            DialStatus::TooLong => true,
        };
        (wrong_length && self.length_checked).then(|| s::dialer_wrong_length(l()).to_owned())
    }

    /// The button's accessible name: "Canada, calling code +1", or "Country"
    /// with none on it.
    pub fn country_label(&self) -> String {
        match self.country.as_deref().map(|iso| (iso, calling_code(iso))) {
            Some((iso, Some(code))) => s::dialer_country_button(l(), country_name(iso, l()), code),
            _ => s::dialer_country(l()).to_owned(),
        }
    }

    /// The "Suggested" group of the country list: the home country, the one on
    /// the button, then Canada and the United States, each once.
    pub fn suggested(&self) -> Vec<&'static str> {
        suggested_countries(self.home.as_deref(), self.country.as_deref())
    }

    /// The field after the member changed it to `typed`: read in the country on
    /// the button, which moves when the number names its own (a `+` number, or
    /// a +1 area code of another country). The length check is about the number
    /// before, so it goes.
    fn absorb(&mut self, typed: &str) {
        let reading = read_dial_entry(typed, self.country.as_deref());
        if let Some(iso) = reading.iso {
            self.country = Some(iso.to_owned());
        }
        self.entry = reading.national;
        self.length_checked = false;
    }

    /// The member chose `iso` in the country list. The digits already typed are
    /// kept and read again in it.
    fn pick(&mut self, iso: &str) {
        let Some(country) = plan::find_country(iso) else {
            return;
        };
        self.country = Some(country.iso.to_owned());
        let digits: String = self.entry.chars().filter(char::is_ascii_digit).collect();
        self.absorb(&digits);
    }

    /// The workspace's home country is known (or known to be none). The button
    /// moves to it only if the member has not started: an empty field still
    /// showing the country it opened on.
    pub(crate) fn adopt_home(&mut self, home: Option<&'static str>) {
        let untouched = self.entry.is_empty() && self.country == self.home;
        self.home = home.map(str::to_owned);
        if untouched && home.is_some() {
            self.country = self.home.clone();
        }
    }
}

/// What the member does on the dialler.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum DialerEvent {
    /// The field changed to this text: typed, pasted, or a key of the keypad.
    /// The core reads it and writes [`DialerScreen::entry`] back.
    Edit(String),
    /// The member chose this country (ISO 3166) in the list.
    PickCountry(String),
    /// The member stopped typing: the field lost focus, or Enter was pressed.
    /// A wrong length is said from now until the next edit.
    CheckLength,
    /// Place the call. Refused (a wrong length then said) unless
    /// [`SignedIn::can_place_call`].
    Dial,
}

/// Which way a single-character delete went, for [`edit_dial_input`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum DeleteDirection {
    /// Backspace: the character before the cursor.
    #[default]
    Backward,
    /// Delete: the character after it.
    Forward,
}

/// Digits and a leading `+`, as typed or pasted: a `+` anywhere else is
/// dropped.
pub fn clean_dial_input(raw: &str) -> String {
    let digits: String = raw.chars().filter(char::is_ascii_digit).collect();
    if raw.trim_start().starts_with('+') {
        format!("+{digits}")
    } else {
        digits
    }
}

/// What a field showing `previous` holds after an edit left it showing
/// `next`, cleaned ([`clean_dial_input`]), for a shell to send as
/// [`DialerEvent::Edit`]. After the web dialler's `editDialInput`.
///
/// A delete over a formatting character must still delete something. The
/// field shows `(416) 555-0100`; erasing the `)` changes no digit, so the
/// formatter would put it straight back and the key would look dead. When one
/// character went and no digit with it, the digit the member was walking over
/// goes instead: the one before it on a backspace, the one after it on a
/// forward delete. Anything else (a paste, a replacement, a delete before the
/// first digit or after the last) is taken as it is.
pub fn edit_dial_input(previous: &str, next: &str, direction: DeleteDirection) -> String {
    let cleaned = clean_dial_input(next);
    let before: Vec<char> = previous.chars().collect();
    let after: Vec<char> = next.chars().collect();
    if after.len() + 1 != before.len() || cleaned != clean_dial_input(previous) {
        return cleaned;
    }
    let at = before
        .iter()
        .zip(&after)
        .take_while(|(a, b)| a == b)
        .count();
    let mut one_out = before.clone();
    one_out.remove(at);
    if one_out != after {
        return cleaned;
    }
    let digits_before = before[..at].iter().filter(|c| c.is_ascii_digit()).count();
    let plus = usize::from(cleaned.starts_with('+'));
    let cut = match direction {
        DeleteDirection::Backward => digits_before.checked_sub(1),
        DeleteDirection::Forward => Some(digits_before),
    }
    .map(|digit| plus + digit)
    .filter(|cut| *cut < cleaned.len());
    match cut {
        Some(cut) => format!("{}{}", &cleaned[..cut], &cleaned[cut + 1..]),
        None => cleaned,
    }
}

/// How many of the characters [`clean_dial_input`] keeps (every digit, and a
/// `+` at the start) sit before character `caret` of `text`. The field is
/// written again after every edit, so a caret kept by position would jump to
/// the end; kept by this count, it lands after the same digit in the new text
/// ([`caret_after_entry_chars`]).
pub fn entry_chars_before(text: &str, caret: usize) -> usize {
    let before: String = text.chars().take(caret).collect();
    let plus = usize::from(before.trim_start().starts_with('+'));
    before.chars().filter(char::is_ascii_digit).count() + plus
}

/// The caret, in characters, just after the `count`-th digit (or `+`) of
/// `formatted`: 0 is the start, and past the last one is the end.
pub fn caret_after_entry_chars(formatted: &str, count: usize) -> usize {
    if count == 0 {
        return 0;
    }
    let mut seen = 0;
    for (at, c) in formatted.chars().enumerate() {
        if c.is_ascii_digit() || c == '+' {
            seen += 1;
            if seen == count {
                return at + 1;
            }
        }
    }
    formatted.chars().count()
}

/// `raw` as it reads, for display only: the digits grouped in threes, around
/// the last ten when there are that many, so `+12125550142` reads
/// `+1 212 555 0142`. The member's own spaces and brackets are ignored rather
/// than fought with, a leading `+` is kept, and text with no digits comes back
/// as it is. The grouping is North American on purpose, which is this
/// product's market; elsewhere it is still readable, and being wrong costs
/// legibility and never a digit.
pub fn format_dial_entry(raw: &str) -> String {
    let digits: String = raw.chars().filter(char::is_ascii_digit).collect();
    if digits.is_empty() {
        return raw.to_owned();
    }
    let plus = if raw.starts_with('+') { "+" } else { "" };
    let split = digits.len().saturating_sub(SUBSCRIBER);
    let (country, subscriber) = digits.split_at(split);
    let groups: Vec<&str> = if subscriber.len() < SUBSCRIBER {
        chunks(subscriber)
    } else {
        vec![
            country,
            &subscriber[..GROUP],
            &subscriber[GROUP..GROUP * 2],
            &subscriber[GROUP * 2..],
        ]
    };
    let groups: Vec<&str> = groups.into_iter().filter(|g| !g.is_empty()).collect();
    format!("{plus}{}", groups.join(" "))
}

/// `text` as a person reads it: a phone number grouped as the dialler groups
/// it ([`format_dial_entry`]), so `+14165550142` reads `+1 416 555 0142`, and
/// anything else as it is (a name, an email address, the service's `Unknown`,
/// a short code).
///
/// A North American number stored without its `+` (eleven digits, the first a
/// `1`, as the service keeps some of them) reads with it, so the same number
/// reads the same wherever it comes from: `14165550142` is `+1 416 555 0142`
/// too. Nothing else gains a `+`: ten digits could be a number of any country
/// written without its code, and a guess there would misstate it.
///
/// For showing only. Wherever the value is sent, searched for or compared, the
/// service's own text is what goes: grouping adds spaces and never removes a
/// digit, but it is not the stored value.
///
/// Text counts as a phone number when it holds at least [`MIN_DIAL_DIGITS`]
/// digits and nothing but digits, a leading `+`, and the spaces, brackets,
/// dashes and dots people write numbers with. So `Ada 2` and
/// `ada2@example.com` are left alone, and so is a four-digit code.
pub fn format_phone_number(text: &str) -> String {
    let trimmed = text.trim();
    let digits: String = trimmed.chars().filter(char::is_ascii_digit).collect();
    let plus = trimmed.starts_with('+');
    let body = trimmed.strip_prefix('+').unwrap_or(trimmed);
    let written_as_a_number = body
        .chars()
        .all(|c| c.is_ascii_digit() || matches!(c, ' ' | '(' | ')' | '-' | '.'));
    if digits.len() < MIN_DIAL_DIGITS || !written_as_a_number {
        return text.to_owned();
    }
    if !plus && digits.len() == NORTH_AMERICAN && digits.starts_with('1') {
        return format_dial_entry(&format!("+{digits}"));
    }
    format_dial_entry(trimmed)
}

/// A North American number with its country code, `1`: eleven digits.
const NORTH_AMERICAN: usize = SUBSCRIBER + 1;

/// `digits` in groups of three, left to right.
fn chunks(digits: &str) -> Vec<&str> {
    (0..digits.len())
        .step_by(GROUP)
        .map(|start| &digits[start..(start + GROUP).min(digits.len())])
        .collect()
}

/// A call's length as a call timer reads it: `mm:ss` under an hour, then
/// `h:mm:ss`, the minutes never past 59.
pub fn format_call_duration(seconds: u64) -> String {
    let hours = seconds / 3600;
    let minutes = seconds % 3600 / 60;
    let seconds = seconds % 60;
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes:02}:{seconds:02}")
    }
}

/// A dial whose answer is awaited: which workspace it was placed from, and
/// whether the member hung up before it answered. A dial that answers after
/// the member hung up is already ringing somebody, so its carrier leg is ended
/// the moment its id is known.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct PendingDial {
    pub(crate) workspace_id: String,
    pub(crate) abandoned: bool,
}

impl SignedIn {
    /// Whether "Call" works: a role that may place calls, a whole number in a
    /// known shape (the line before Call names it), and no call, meeting,
    /// audition or ring in the way.
    pub fn can_place_call(&self) -> bool {
        self.capabilities().can_dial
            && self.dialer.reading().e164.is_some()
            && !self.media_busy()
            && self.pending_dial.is_none()
            && !self.ring.is_ringing()
    }

    pub(crate) fn dialer_event(&mut self, event: DialerEvent, tickets: &mut Tickets) -> Next {
        match event {
            DialerEvent::Edit(entry) => self.dialer.absorb(&entry),
            DialerEvent::PickCountry(iso) => self.dialer.pick(&iso),
            DialerEvent::CheckLength => self.dialer.length_checked = true,
            DialerEvent::Dial if self.can_place_call() => return Next::Stay(self.dial(tickets)),
            // A refused press says a wrong length too, as leaving the field does.
            DialerEvent::Dial => self.dialer.length_checked = true,
        }
        stay()
    }

    /// Entering the dialler: the workspace's numbers are read for its home
    /// country, unless they already have been.
    pub(crate) fn enter_dialer(&mut self, tickets: &mut Tickets) -> Vec<Effect> {
        if let OwnedNumbersList::Ready(owned) = &self.marketplace.owned {
            let home = home_country(owned.numbers.iter().map(|n| n.phone_number.as_str()));
            self.dialer.adopt_home(home);
            return Vec::new();
        }
        self.marketplace.owned = OwnedNumbersList::Loading;
        vec![Effect::LoadOwnedNumbers {
            ticket: tickets.issue(Slot::OwnedNumbers),
            workspace_id: self.workspace_id(),
        }]
    }

    /// Places the call to the number the line before Call showed: the call's
    /// screen at once, and the request. Only called when
    /// [`can_place_call`](Self::can_place_call), so the number is whole.
    fn dial(&mut self, tickets: &mut Tickets) -> Vec<Effect> {
        let workspace_id = self.workspace_id();
        let to = self.dialer.reading().e164.unwrap_or_default();
        self.pending_dial = Some(PendingDial {
            workspace_id: workspace_id.clone(),
            abandoned: false,
        });
        self.active_call = Some(ActiveCall::outbound(workspace_id.clone(), to.clone()));
        // The summary of a call before it, and with it its transcript, is
        // gone.
        let mut effects = self.transcript_dropped(tickets);
        effects.push(Effect::Dial {
            ticket: tickets.issue(Slot::Dial),
            workspace_id,
            to,
        });
        effects
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_north_american_number_groups_around_its_last_ten_digits() {
        assert_eq!(format_dial_entry("+12125550142"), "+1 212 555 0142");
        assert_eq!(format_dial_entry("2125550142"), "212 555 0142");
        assert_eq!(format_dial_entry("(212) 555-0142"), "212 555 0142");
    }

    #[test]
    fn another_countrys_number_stays_readable_and_keeps_every_digit() {
        // Written in pieces so the hygiene scan does not read a real number.
        let raw = format!("+{}{}", "44", "2079460958");
        let grouped = format!("+{} {} {} {}", "44", "207", "946", "0958");
        assert_eq!(format_dial_entry(&raw), grouped);
    }

    #[test]
    fn a_partial_number_groups_as_far_as_it_goes() {
        assert_eq!(format_dial_entry("+1"), "+1");
        assert_eq!(format_dial_entry("2125"), "212 5");
        assert_eq!(format_dial_entry("212555014"), "212 555 014");
    }

    #[test]
    fn text_with_no_digits_comes_back_as_it_is() {
        assert_eq!(format_dial_entry(""), "");
        assert_eq!(format_dial_entry("+"), "+");
        assert_eq!(format_dial_entry("abc"), "abc");
    }

    #[test]
    fn formatting_never_removes_a_digit() {
        for raw in ["+1 212 555 0199", "12345678901234", "0"] {
            let digits = |s: &str| s.chars().filter(char::is_ascii_digit).collect::<String>();
            assert_eq!(digits(&format_dial_entry(raw)), digits(raw), "{raw}");
        }
    }

    #[test]
    fn a_phone_number_is_shown_grouped_and_anything_else_as_it_is() {
        assert_eq!(format_phone_number("+14165550142"), "+1 416 555 0142");
        assert_eq!(format_phone_number(" (416) 555-0142 "), "416 555 0142");
        assert_eq!(format_phone_number("416.555.0142"), "416 555 0142");
        for kept in [
            "Contract Test Caller",
            "Unknown",
            "ada2@example.com",
            "Ada 2125550142",
            "1+4165550142",
            "+",
            "611",
            "2026",
            "",
        ] {
            assert_eq!(format_phone_number(kept), kept, "{kept:?}");
        }
    }

    #[test]
    fn a_north_american_number_reads_with_its_plus_however_it_was_stored() {
        for stored in [
            "+14165550142",
            "14165550142",
            " 14165550142 ",
            "1 416 555 0142",
            "1 (416) 555-0142",
            "1.416.555.0142",
        ] {
            assert_eq!(format_phone_number(stored), "+1 416 555 0142", "{stored:?}");
        }
        // Only eleven digits led by a 1: anything else is grouped as it is,
        // never given a country code it may not have.
        assert_eq!(format_phone_number("4165550142"), "416 555 0142");
        assert_eq!(format_phone_number("24165550142"), "2 416 555 0142");
        assert_eq!(format_phone_number("114165550142"), "11 416 555 0142");
        // Written in pieces so the hygiene scan does not read a real number.
        let other = format!("+{}{}", "44", "2079460958");
        assert_eq!(
            format_phone_number(&other),
            format!("+{} {} {} {}", "44", "207", "946", "0958")
        );
        // And still nothing that is not a number.
        assert_eq!(format_phone_number("1 416 555 0142 x"), "1 416 555 0142 x");
        assert_eq!(format_phone_number("ada1@example.com"), "ada1@example.com");
    }

    #[test]
    fn a_call_reads_as_minutes_then_hours() {
        assert_eq!(format_call_duration(0), "00:00");
        assert_eq!(format_call_duration(9), "00:09");
        assert_eq!(format_call_duration(75), "01:15");
        assert_eq!(format_call_duration(3599), "59:59");
        assert_eq!(format_call_duration(3600), "1:00:00");
        assert_eq!(format_call_duration(3903), "1:05:03");
    }

    /// A screen showing `entry` under `country`.
    fn screen(entry: &str, country: Option<&str>) -> DialerScreen {
        DialerScreen {
            entry: entry.to_owned(),
            country: country.map(str::to_owned),
            ..DialerScreen::default()
        }
    }

    /// `digits` with a `+` in front, so the hygiene scan does not read a real
    /// number here.
    fn plus(digits: &str) -> String {
        format!("+{digits}")
    }

    #[test]
    fn the_hint_and_the_placeholder_follow_the_country_on_the_button() {
        let canada = screen("", Some("CA"));
        assert_eq!(
            canada.hint(),
            "Type the number as you would dial it locally (Canada), or start with + and its \
             country code."
        );
        assert_eq!(canada.placeholder(), "(416) 555-0199");
        assert_eq!(
            screen("", Some("JP")).placeholder(),
            "",
            "no example for Japan"
        );
        let none = screen("", None);
        assert_eq!(
            none.hint(),
            "Choose a country, or start with + and the country code."
        );
        assert_eq!(none.placeholder(), "+\u{2026}");
    }

    #[test]
    fn the_line_before_call_names_the_country_and_the_whole_number() {
        assert_eq!(
            screen("(416) 555-0199", Some("CA"))
                .destination()
                .as_deref(),
            Some("Rings Canada \u{b7} +1 416 555 0199")
        );
        assert_eq!(
            screen("(416) 555", Some("CA")).destination(),
            None,
            "not whole yet"
        );
        let unknown = "Unrecognised country code. Check the number before calling.";
        let named_nothing = screen(&plus("99912345678"), Some("CA"));
        assert_eq!(named_nothing.destination().as_deref(), Some(unknown));
        assert_eq!(
            screen(&plus("9991"), None).destination().as_deref(),
            Some(unknown)
        );
        assert_eq!(
            screen(&plus("999"), None).destination(),
            None,
            "three digits could be a code"
        );
        // The billed call: no line for a number that cannot be sent.
        assert_eq!(screen(&plus("4165550123"), Some("CA")).destination(), None);
    }

    /// A wrong length is said only once the member stops: the field lost focus,
    /// Enter, or a refused Call. Until then Call is simply off.
    #[test]
    fn a_wrong_length_waits_until_the_member_stops() {
        let wrong = "That is the wrong number of digits for a number in the country it rings. \
                     Check it before you call.";
        for (entry, country) in [
            (plus("4165550123"), Some("CA")),
            ("(416) 555".to_owned(), Some("CA")),
            ("(416) 555-01999".to_owned(), Some("CA")),
        ] {
            let mut typing = screen(&entry, country);
            assert_eq!(typing.problem(), None, "{entry} while typing");
            typing.length_checked = true;
            assert_eq!(
                typing.problem().as_deref(),
                Some(wrong),
                "{entry} once stopped"
            );
        }
        // Nothing to measure: an empty field, a lone plus, a trunk, an unknown
        // code, and a whole number.
        for (entry, country) in [
            ("", Some("CA")),
            ("+", None),
            ("0", Some("GB")),
            (&plus("9991"), None),
            ("(416) 555-0199", Some("CA")),
        ] {
            let stopped = DialerScreen {
                length_checked: true,
                ..screen(entry, country)
            };
            assert_eq!(stopped.problem(), None, "{entry:?}");
        }
    }

    #[test]
    fn digits_without_a_country_and_an_emergency_code_are_said_at_once() {
        assert_eq!(
            screen("4165550199", None).problem().as_deref(),
            Some("Choose a country, or start with + and the country code.")
        );
        assert_eq!(
            screen("911", Some("CA")).problem().as_deref(),
            Some(
                "Emergency calls cannot be placed from District AI. Use your phone's own dialer \
                 to call for help."
            )
        );
    }

    #[test]
    fn the_country_button_says_its_country_and_code() {
        assert_eq!(
            screen("", Some("CA")).country_label(),
            "Canada, calling code +1"
        );
        assert_eq!(screen("", None).country_label(), "Country");
        assert_eq!(screen("", Some("ZZ")).country_label(), "Country");
    }

    #[test]
    fn the_field_moves_the_flag_and_keeps_the_local_form() {
        let mut dialer = screen("", Some("CA"));
        dialer.length_checked = true;
        dialer.absorb(&plus("44 20 7946 0958"));
        assert_eq!(dialer.country.as_deref(), Some("GB"));
        assert_eq!(dialer.entry, "020 7946 0958");
        assert!(!dialer.length_checked, "an edit clears the length check");
        // Digits with no country keep no country.
        let mut none = screen("", None);
        none.absorb("4165550199");
        assert_eq!((none.country, none.entry.as_str()), (None, "4165550199"));
    }

    #[test]
    fn a_picked_country_reads_the_digits_again() {
        let mut dialer = screen("4165550199", None);
        dialer.pick("gb");
        assert_eq!(dialer.country.as_deref(), Some("GB"));
        assert_eq!(dialer.entry, "04165 550199", "with the trunk 0 of the UK");
        dialer.pick("ZZ");
        assert_eq!(
            dialer.country.as_deref(),
            Some("GB"),
            "an unlisted pick changes nothing"
        );
        // A plus with no code yet goes, so the button shows the pick.
        let mut plus_alone = screen("+", None);
        plus_alone.pick("FR");
        assert_eq!(
            (plus_alone.country.as_deref(), plus_alone.entry.as_str()),
            (Some("FR"), "")
        );
        assert_eq!(dialer.suggested(), ["GB", "CA", "US"]);
    }

    /// The button moves to the home country only if the member has not
    /// started: an empty field still showing the country it opened on.
    #[test]
    fn the_home_country_moves_the_button_only_before_the_member_starts() {
        let mut fresh = DialerScreen::default();
        fresh.adopt_home(Some("FR"));
        assert_eq!(
            (fresh.home.as_deref(), fresh.country.as_deref()),
            (Some("FR"), Some("FR"))
        );
        // A later read that finds another country moves an untouched button.
        fresh.adopt_home(Some("GB"));
        assert_eq!(fresh.country.as_deref(), Some("GB"));
        // A picked country stays.
        let mut picked = screen("", Some("DE"));
        picked.adopt_home(Some("FR"));
        assert_eq!(
            (picked.home.as_deref(), picked.country.as_deref()),
            (Some("FR"), Some("DE"))
        );
        // So does a country under digits already typed.
        let mut typing = screen("4", None);
        typing.adopt_home(Some("CA"));
        assert_eq!(typing.country, None);
        // A workspace with no country leaves the button as it is.
        let mut nothing = screen("", Some("CA"));
        nothing.home = Some("CA".to_owned());
        nothing.adopt_home(None);
        assert_eq!(
            (nothing.home, nothing.country.as_deref()),
            (None, Some("CA"))
        );
    }

    #[test]
    fn a_plus_counts_only_at_the_start() {
        assert_eq!(clean_dial_input(" +1 (416) 555-0199"), "+14165550199");
        assert_eq!(clean_dial_input("416+555"), "416555");
        assert_eq!(clean_dial_input(""), "");
    }

    /// A delete over a formatting character takes the digit the member was
    /// walking over, so the key never looks dead.
    #[test]
    fn deleting_a_formatting_character_deletes_a_digit() {
        use DeleteDirection::{Backward, Forward};
        let shown = "(416) 555-0199";
        // Backspace over `)`: the 6 goes.
        assert_eq!(
            edit_dial_input(shown, "(416 555-0199", Backward),
            "415550199"
        );
        // Delete over `)`, from before it: the 5 after it goes.
        assert_eq!(
            edit_dial_input(shown, "(416 555-0199", Forward),
            "416550199"
        );
        // Before the first digit, or after the last, there is nothing to take.
        assert_eq!(
            edit_dial_input(shown, "416) 555-0199", Backward),
            "4165550199"
        );
        assert_eq!(edit_dial_input("416 ", "416", Forward), "416");
        // With a plus, the plus stays.
        assert_eq!(edit_dial_input("+1 416", "+1416", Backward), "+416");
        // A digit deleted, a paste, a replacement: taken as they are.
        assert_eq!(
            edit_dial_input(shown, "(416) 555-019", Backward),
            "416555019"
        );
        assert_eq!(
            edit_dial_input(shown, "+14165550100", Backward),
            "+14165550100"
        );
        assert_eq!(edit_dial_input("1 2-3", "1-23", Backward), "123");
    }

    #[test]
    fn the_caret_lands_after_the_same_digit() {
        assert_eq!(entry_chars_before("(416) 555", 5), 3);
        assert_eq!(entry_chars_before(" +1 416", 3), 2);
        assert_eq!(
            entry_chars_before("416+5", 4),
            3,
            "a plus mid-number is not kept"
        );
        assert_eq!(caret_after_entry_chars("(416) 555-0199", 3), 4);
        assert_eq!(caret_after_entry_chars("+1 416", 1), 1);
        assert_eq!(caret_after_entry_chars("(416) 555", 0), 0);
        assert_eq!(
            caret_after_entry_chars("(416) 5", 9),
            7,
            "past the last digit is the end"
        );
    }
}
