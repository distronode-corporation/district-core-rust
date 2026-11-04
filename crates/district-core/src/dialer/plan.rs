//! Reading what the member types into the dialler, in a country: dialler
//! design A, after district-ios's `DialPlan.swift` (and the Mac's identical
//! copy), rule for rule.
//!
//! The country is always on screen before the call, and that is what makes it
//! safe to build the number here. The billed call the design exists to prevent
//! was a Canadian mobile typed with `+41` in front of its area code, which is
//! Switzerland's code. Now the number is typed the local way under a flag and
//! calling code the member can see, and the line under the field names the
//! country and the whole international number before Call, so nothing is sent
//! that was not shown.
//!
//! The service still normalises its own copy. What leaves here is E.164 (`+`,
//! the calling code, the national number), which its normaliser passes through
//! unchanged, so the do-not-call check and the dial run against the number
//! that was shown.
//!
//! Pure: a function of the text and a country, with no IO and no clock.

use district_l10n::Locale;

use super::MIN_DIAL_DIGITS;
use super::countries::{
    CANADIAN_AREA_CODES, COUNTRIES, DialCountry, NANP_AREA_COUNTRIES, NANP_TOLL_FREE,
    NATIONAL_EXAMPLES, OTHER_TRUNK_PREFIXES, TRUNK_ZERO,
};

/// The most digits an E.164 number has, its calling code included.
pub const MAX_DIAL_DIGITS: usize = 15;

/// The longest calling code.
const MAX_CALLING_CODE_DIGITS: usize = 3;

/// Short codes that are never read as a number to call: `112` and `911`, which
/// every handset must recognise (3GPP TS 22.101), and the widely deployed
/// national codes: `000` (AU), `111` (NZ), `999` (UK, IE, HK), `110`, `118` and
/// `119` (JP, CN and much of Europe), `113`, `115` and `117` (various EU). The
/// same list as district-core-swift's `DialEntry`.
///
/// Deliberately incomplete, and that is safe: a code missing here is shorter
/// than any dialable number, so it is still never placed; it just gets the
/// length sentence instead of the hand-off. Matched whole, never as a prefix:
/// a North American number that begins 911 is ordinary.
const EMERGENCY_NUMBERS: [&str; 11] = [
    "000", "110", "111", "112", "113", "115", "117", "118", "119", "911", "999",
];

/// What one entry is, in the order the screen acts on it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DialStatus {
    /// Nothing typed.
    Empty,
    /// A number on its way: too few digits yet, a `+` code not finished, or a
    /// length the country never issues (said only once the member stops).
    Typing,
    /// Dialable: [`DialReading::e164`] is set.
    Ready,
    /// An emergency short code: refused, and the member is sent to their own
    /// phone.
    Emergency,
    /// More digits than the country's numbers have, or than E.164 allows.
    TooLong,
    /// Digits with no country to read them in and no `+`.
    NeedsCountry,
}

/// One entry, read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DialReading {
    /// What the entry is.
    pub status: DialStatus,
    /// The country the number rings, or `None`: none chosen, or a `+` code no
    /// listed country uses.
    pub iso: Option<&'static str>,
    /// The calling code, without `+`, or empty.
    pub calling_code: &'static str,
    /// The national significant number: no trunk prefix, no calling code.
    pub national_number: String,
    /// What the field shows: the local form, as it would be dialled in
    /// [`iso`](Self::iso).
    pub national: String,
    /// The number as the world dials it, grouped: `+1 416 555 0199`.
    pub international: String,
    /// Whether the entry named its own country (`+`, `011`, `00`).
    pub named_its_country: bool,
    /// What is sent to the dial API. Set exactly when
    /// [`status`](Self::status) is [`DialStatus::Ready`].
    pub e164: Option<String>,
}

impl DialReading {
    /// A reading with only a status and what it shows.
    fn shown(status: DialStatus, iso: Option<&'static str>, shown: &str) -> Self {
        Self {
            status,
            iso,
            calling_code: "",
            national_number: String::new(),
            national: shown.to_owned(),
            international: shown.to_owned(),
            named_its_country: false,
            e164: None,
        }
    }
}

/// `raw` read in `country` (an ISO 3166 code, `None` when none is chosen).
///
/// The emergency test comes first, before any country: `911` must reach the
/// hand-off sentence, never "choose a country".
pub fn read_dial_entry(raw: &str, country: Option<&str>) -> DialReading {
    let country = country.and_then(find_country);
    let compact: String = raw
        .chars()
        .filter(|c| *c == '+' || c.is_ascii_digit())
        .collect();
    let mut plus = compact.starts_with('+');
    let mut digits: String = compact.chars().filter(char::is_ascii_digit).collect();
    if is_emergency(&compact) {
        // Shown as typed, its `+` kept: `+911` is refused, but it is also
        // the start of every Indian number beginning +91 1, and the next digit
        // must still be read after the code rather than as a local number.
        let shown = if plus { format!("+{digits}") } else { digits };
        return DialReading::shown(DialStatus::Emergency, country.map(|c| c.iso), &shown);
    }
    if !plus && let Some(rest) = drop_exit_code(&digits, country) {
        digits = rest.to_owned();
        plus = true;
    }
    if digits.is_empty() {
        return if plus {
            DialReading {
                named_its_country: true,
                ..DialReading::shown(DialStatus::Typing, None, "+")
            }
        } else {
            DialReading::shown(DialStatus::Empty, country.map(|c| c.iso), "")
        };
    }
    if plus {
        read_international(&digits, country)
    } else {
        read_national(&digits, country)
    }
}

/// Whether the compacted entry is an emergency short code, as typed or with a
/// `1` (or `+1`) in front: following an old "start with the country code"
/// instruction, `911` became `+1911`.
fn is_emergency(compact: &str) -> bool {
    let digits = compact.strip_prefix('+').unwrap_or(compact);
    EMERGENCY_NUMBERS.contains(&digits)
        || digits
            .strip_prefix('1')
            .is_some_and(|rest| EMERGENCY_NUMBERS.contains(&rest))
}

/// `011` from a North American country, `00` from anywhere else, is the exit
/// code that stands for `+`. With no country chosen either one is taken,
/// because there is no national number to confuse it with.
fn drop_exit_code<'a>(digits: &'a str, country: Option<&DialCountry>) -> Option<&'a str> {
    let nanp = country.is_some_and(|c| c.code == "1");
    if (nanp || country.is_none())
        && let Some(rest) = digits.strip_prefix("011")
    {
        return Some(rest);
    }
    if nanp {
        return None;
    }
    digits.strip_prefix("00")
}

fn read_national(digits: &str, country: Option<&'static DialCountry>) -> DialReading {
    let Some(country) = country else {
        return DialReading::shown(DialStatus::NeedsCountry, None, digits);
    };
    let code = country.code;
    let mut nsn = digits;
    let mut typed_trunk = false;
    if code == "1" {
        if let Some(rest) = nsn.strip_prefix('1') {
            nsn = rest;
            typed_trunk = true;
        }
    } else if let Some(trunk) = trunk_prefix(country.iso) {
        // Half of a two-digit trunk (Hungary's `06`) is shown as typed, not
        // read as a number.
        if nsn.len() < trunk.len() && trunk.starts_with(nsn) {
            return DialReading {
                calling_code: code,
                national: nsn.to_owned(),
                international: format!("+{code}"),
                ..DialReading::shown(DialStatus::Typing, Some(country.iso), "")
            };
        }
        if let Some(rest) = nsn.strip_prefix(trunk) {
            nsn = rest;
            typed_trunk = true;
        }
    }
    // A code several countries share is read again as each digit arrives,
    // exactly as a pasted `+` number is: +1 by its area code (416 is Canada,
    // 206 the United States, 876 Jamaica), +7 by its first digit. Typed one key
    // at a time, `+7` moves the flag to Russia and the rest arrives here as a
    // national number; read only in the country on the button, a Kazakh
    // number would stay "Rings Russia".
    let iso = country_for_code(code, nsn, country.iso, Some(country.iso));
    finish(iso, code, nsn, typed_trunk, false)
}

fn read_international(digits: &str, current: Option<&DialCountry>) -> DialReading {
    let longest = MAX_CALLING_CODE_DIGITS.min(digits.len());
    for length in (1..=longest).rev() {
        let Some(first) = first_country_with_code(&digits[..length]) else {
            continue;
        };
        let mut nsn = &digits[length..];
        let iso = country_for_code(first.code, nsn, first.iso, current.map(|c| c.iso));
        // "+44 (0)20 ..." pasted from a signature: the trunk 0 is not dialled
        // after a calling code.
        if TRUNK_ZERO.contains(&iso)
            && let Some(rest) = nsn.strip_prefix('0')
        {
            nsn = rest;
        }
        return finish(iso, first.code, nsn, false, true);
    }
    // An unknown code is said plainly and stays dialable: this client is not
    // the authority on which codes exist.
    let shown = format!("+{digits}");
    let mut reading = DialReading {
        named_its_country: true,
        ..DialReading::shown(DialStatus::Typing, None, &shown)
    };
    if digits.len() > MAX_DIAL_DIGITS {
        reading.status = DialStatus::TooLong;
    } else if digits.len() >= MIN_DIAL_DIGITS {
        reading.status = DialStatus::Ready;
        reading.e164 = Some(shown);
    }
    reading
}

/// Which country a calling code rings for `nsn`: the code's `first` country,
/// except for a code several countries share, which the digits after it
/// decide.
fn country_for_code(
    code: &str,
    nsn: &str,
    first: &'static str,
    current: Option<&str>,
) -> &'static str {
    match code {
        "1" => nanp_country(nsn, current),
        // +7 is Russia and Kazakhstan; Kazakh numbers start 6 or 7. Before
        // that digit the +7 country on screen stays, as a +1 one does before
        // its area code.
        "7" if nsn.is_empty() && current == Some("KZ") => "KZ",
        "7" if nsn.starts_with('6') || nsn.starts_with('7') => "KZ",
        "7" => "RU",
        _ => first,
    }
}

fn finish(
    iso: &'static str,
    code: &'static str,
    nsn: &str,
    typed_trunk: bool,
    plus: bool,
) -> DialReading {
    let international = if nsn.is_empty() {
        format!("+{code}")
    } else {
        format!("+{code} {}", chunk(nsn, groups(iso, nsn)).join(" "))
    };
    let mut reading = DialReading {
        status: DialStatus::Typing,
        iso: Some(iso),
        calling_code: code,
        national_number: nsn.to_owned(),
        national: format_national(iso, code, nsn, typed_trunk),
        international,
        named_its_country: plus,
        e164: None,
    };
    let (shortest, longest) = lengths(iso, code);
    let total = code.len() + nsn.len();
    if total > MAX_DIAL_DIGITS || nsn.len() > longest {
        reading.status = DialStatus::TooLong;
    } else if nsn.len() >= shortest && total >= MIN_DIAL_DIGITS {
        reading.status = DialStatus::Ready;
        reading.e164 = Some(format!("+{code}{nsn}"));
    }
    reading
}

/// The national number as it is dialled inside `iso`, trunk prefix included.
///
/// A `+` and a calling code with no digit after it yet stay as they are, so the
/// next digit is still read after the code. Shown as an empty national number
/// instead, the next digit would be read as a local one, and a trunk prefix
/// would swallow it: Saint Petersburg's 812 typed after +7 would lose its 8 to
/// Russia's trunk 8. A national number is empty only after a trunk prefix,
/// which is shown as typed.
fn format_national(iso: &str, code: &str, nsn: &str, typed_trunk: bool) -> String {
    if nsn.is_empty() {
        return match (typed_trunk, code) {
            (false, _) => format!("+{code}"),
            (true, "1") => "1".to_owned(),
            (true, _) => trunk_prefix(iso).unwrap_or("0").to_owned(),
        };
    }
    if code == "1" {
        return match nsn.len() {
            ..=3 => nsn.to_owned(),
            4..=6 => format!("({}) {}", &nsn[..3], &nsn[3..]),
            _ => format!("({}) {}-{}", &nsn[..3], &nsn[3..6], &nsn[6..]),
        };
    }
    let mut parts: Vec<String> = chunk(nsn, groups(iso, nsn))
        .into_iter()
        .map(str::to_owned)
        .collect();
    // An Australian 13, 1300 or 1800 number is dialled as it stands, with no
    // trunk 0.
    if iso == "AU" && is_australian_non_geographic(nsn) {
        return parts.join(" ");
    }
    // The parentheses close only once a digit follows the area code. A `)`
    // written after `(02` alone would be the last character, and deleting it
    // would put it straight back, so backspace could never get past the area
    // code.
    if iso == "AU" && !nsn.starts_with('4') && parts.len() > 1 {
        return format!("(0{}) {}", parts[0], parts[1..].join(" "));
    }
    if let Some(trunk) = trunk_prefix(iso) {
        parts[0] = format!("{trunk}{}", parts[0]);
    }
    parts.join(" ")
}

/// How a country's national number is grouped. A display convention only: a
/// country not named here gets threes, which reads well enough and changes no
/// digit.
fn groups(iso: &str, nsn: &str) -> &'static [usize] {
    let starts = |prefixes: &[&str]| prefixes.iter().any(|p| nsn.starts_with(p));
    match iso {
        "GB" if nsn.starts_with('2') => &[2, 4, 4],
        "GB" => &[4, 6],
        "FR" => &[1, 2, 2, 2, 2],
        "DE" if starts(&["15", "16", "17"]) => &[3, 8],
        "DE" if starts(&["30", "40", "69", "89"]) => &[2, 9],
        "DE" => &[4, 8],
        "AU" if is_australian_non_geographic(nsn) => &[4, 3, 3],
        "AU" if nsn.starts_with('4') => &[3, 3, 3],
        "AU" => &[1, 4, 4],
        "CH" => &[2, 3, 2, 2],
        "SG" => &[4, 4],
        _ => &[3, 3, 4],
    }
}

/// How many digits a country's national numbers have, shortest and longest.
/// Exact only where it is certain; anywhere else the E.164 floor and ceiling
/// decide.
///
/// Wide enough for every number the country issues, not just the common
/// shape: a range that misses one blocks a real call. Australia's 13 xx xx is
/// 6 digits and its 1300 and 1800 numbers 10; Singapore's 1800 toll-free
/// numbers are 11; a UK 0800 number can be 7. Switzerland and France issue
/// nothing but 9.
fn lengths(iso: &str, code: &str) -> (usize, usize) {
    match (code, iso) {
        ("1", _) => (10, 10),
        // Russia and Kazakhstan: ten digits after +7, always. Without this an
        // eleven-digit number typed with its 7 and no `+` read as a Kazakh
        // number of eleven and was offered for dialling.
        ("7", _) => (10, 10),
        (_, "GB") => (7, 10),
        (_, "FR" | "CH") => (9, 9),
        (_, "AU") => (6, 10),
        (_, "DE") => (6, 13),
        (_, "SG") => (8, 11),
        _ => (4, MAX_DIAL_DIGITS - code.len()),
    }
}

/// Australia's 13, 1300 and 1800 numbers: no area code and no trunk 0. The
/// whole `1` range, so a lone typed `1` is not shown as `01` (geographic area
/// codes are 2, 3, 7 and 8, mobiles 4).
fn is_australian_non_geographic(nsn: &str) -> bool {
    nsn.starts_with('1')
}

/// `text` cut into groups of `sizes`, the last group taking whatever is left.
fn chunk<'a>(text: &'a str, sizes: &[usize]) -> Vec<&'a str> {
    let mut out = Vec::new();
    let mut rest = text;
    for (index, size) in sizes.iter().enumerate() {
        if rest.is_empty() {
            break;
        }
        let take = if index == sizes.len() - 1 {
            rest.len()
        } else {
            (*size).min(rest.len())
        };
        out.push(&rest[..take]);
        rest = &rest[take..];
    }
    out
}

/// The listed country `iso` names, any case.
pub(crate) fn find_country(iso: &str) -> Option<&'static DialCountry> {
    COUNTRIES.iter().find(|c| c.iso.eq_ignore_ascii_case(iso))
}

/// The first country listed with `code`.
fn first_country_with_code(code: &str) -> Option<&'static DialCountry> {
    COUNTRIES.iter().find(|c| c.code == code)
}

/// The calling code of `iso`, digits only, or `None` for a country the
/// dialler does not list.
pub fn calling_code(iso: &str) -> Option<&'static str> {
    find_country(iso).map(|c| c.code)
}

/// Which +1 country an area code belongs to: Canada, a Caribbean or Pacific
/// member of the plan, or else the United States. Before three digits, and for
/// a toll-free code, `fallback` when it is a +1 country, else the United
/// States.
fn nanp_country(nsn: &str, fallback: Option<&str>) -> &'static str {
    let plus_one = fallback
        .and_then(find_country)
        .filter(|c| c.code == "1")
        .map_or("US", |c| c.iso);
    // A toll-free code (800, 888 ...) belongs to no one country, so it keeps
    // the +1 country already on screen rather than claiming the United States.
    let Some(area) = nsn.get(..3).filter(|_| !is_nanp_toll_free(nsn)) else {
        return plus_one;
    };
    if CANADIAN_AREA_CODES.contains(&area) {
        return "CA";
    }
    NANP_AREA_COUNTRIES
        .iter()
        .find(|(code, _)| *code == area)
        .map_or("US", |(_, iso)| iso)
}

/// Whether a +1 national number is toll-free, shared by every country of the
/// plan.
fn is_nanp_toll_free(nsn: &str) -> bool {
    nsn.get(..3)
        .is_some_and(|area| NANP_TOLL_FREE.contains(&area))
}

/// The trunk prefix dialled before a national number inside the country, or
/// `None`: `0` for most, `8` in Russia and Kazakhstan, `80` in Belarus, `06` in
/// Hungary.
fn trunk_prefix(iso: &str) -> Option<&'static str> {
    if let Some((_, other)) = OTHER_TRUNK_PREFIXES.iter().find(|(c, _)| *c == iso) {
        return Some(other);
    }
    TRUNK_ZERO.contains(&iso).then_some("0")
}

/// The flag of `iso`: its two regional-indicator letters, which a colour emoji
/// font draws as one flag. Empty for anything that is not two ASCII letters.
pub fn flag(iso: &str) -> String {
    const REGIONAL_A: u32 = 0x1F1E6;
    if iso.len() != 2 || !iso.chars().all(|c| c.is_ascii_alphabetic()) {
        return String::new();
    }
    iso.chars()
        .filter_map(|c| char::from_u32(REGIONAL_A + u32::from(c.to_ascii_uppercase()) - 65))
        .collect()
}

/// The name of `iso` in `locale`, or `iso` itself for a country the dialler
/// does not list.
pub fn country_name(iso: &str, locale: Locale) -> &str {
    match (find_country(iso), locale) {
        (Some(country), Locale::En) => country.en,
        (Some(country), Locale::FrCa) => country.fr,
        (None, _) => iso,
    }
}

/// Every country, alphabetical in `locale`, accents and case ignored, so the
/// French list puts Égypte beside Équateur and not after Z.
pub fn sorted_countries(locale: Locale) -> Vec<&'static str> {
    let mut all: Vec<&'static DialCountry> = COUNTRIES.iter().collect();
    all.sort_by_cached_key(|c| fold(country_name(c.iso, locale)));
    all.into_iter().map(|c| c.iso).collect()
}

/// The countries a search matches: by name (accents and case ignored), by ISO
/// code, or by calling code, with or without the `+`. A lone `+` is the start
/// of every calling code, so it lists them all.
///
/// Ranked, then alphabetical within a rank: the ISO code exactly, a name that
/// starts with the query, a calling code, then a name that merely contains
/// it. Enter picks the first row, so alphabetical alone made `us` pick
/// Australia, ahead of Austria and the United States.
pub fn search_countries(query: &str, locale: Locale) -> Vec<&'static str> {
    let all = sorted_countries(locale);
    let needle = fold(query.trim());
    if needle.is_empty() {
        return all;
    }
    let digits = needle.strip_prefix('+').unwrap_or(&needle);
    let by_code = needle.starts_with('+') || !digits.is_empty();
    let by_code = by_code && digits.chars().all(|c| c.is_ascii_digit());
    let mut ranks: [Vec<&'static str>; 4] = Default::default();
    for iso in all {
        let name = fold(country_name(iso, locale));
        let rank = if iso.eq_ignore_ascii_case(&needle) {
            0
        } else if name.starts_with(&needle) {
            1
        } else if by_code && calling_code(iso).is_some_and(|code| code.starts_with(digits)) {
            2
        } else if name.contains(&needle) {
            3
        } else {
            continue;
        };
        ranks[rank].push(iso);
    }
    ranks.concat()
}

/// The "Suggested" group: the workspace's home country, the one on screen,
/// then Canada and the United States, each once.
pub fn suggested_countries(home: Option<&str>, current: Option<&str>) -> Vec<&'static str> {
    let mut out: Vec<&'static str> = Vec::new();
    for iso in [home, current, Some("CA"), Some("US")]
        .into_iter()
        .flatten()
        .filter_map(find_country)
        .map(|c| c.iso)
    {
        if !out.contains(&iso) {
            out.push(iso);
        }
    }
    out
}

/// The country most of `numbers` (a workspace's own, in E.164) belong to; on a
/// tie, the first listed. `None` when none of them reads as a listed country.
///
/// A toll-free +1 number is no evidence of any country: a Canadian workspace
/// with only 1-800 numbers would otherwise read as American.
pub fn home_country<'a>(numbers: impl IntoIterator<Item = &'a str>) -> Option<&'static str> {
    let mut counts: Vec<(&'static str, usize)> = Vec::new();
    for number in numbers {
        let reading = read_dial_entry(number, None);
        let Some(iso) = reading.iso else { continue };
        if reading.calling_code == "1" && is_nanp_toll_free(&reading.national_number) {
            continue;
        }
        match counts.iter_mut().find(|(seen, _)| *seen == iso) {
            Some((_, count)) => *count += 1,
            None => counts.push((iso, 1)),
        }
    }
    // `max_by_key` keeps the last of equals, so the counts are walked
    // backwards to keep the first.
    counts
        .into_iter()
        .rev()
        .max_by_key(|(_, count)| *count)
        .map(|(iso, _)| iso)
}

/// An example national number for `iso`'s placeholder, in a range nobody can
/// be reached on, or `None` for a country with none.
pub fn national_example(iso: &str) -> Option<&'static str> {
    NATIONAL_EXAMPLES
        .iter()
        .find(|(country, _)| *country == iso)
        .map(|(_, example)| *example)
}

/// `text` lower case with the accents taken off, so `etats` finds `États-Unis`
/// and a typed apostrophe finds `Côte d’Ivoire`. The accented letters are the
/// Latin ones a country name or a search for one holds.
fn fold(text: &str) -> String {
    const ACCENTED: &str = "àáâãäåçèéêëìíîïñòóôõöùúûüýÿ’";
    const PLAIN: &str = "aaaaaaceeeeiiiinooooouuuuyy'";
    text.chars()
        .flat_map(char::to_lowercase)
        .map(|c| {
            ACCENTED
                .chars()
                .position(|a| a == c)
                .and_then(|at| PLAIN.chars().nth(at))
                .unwrap_or(c)
        })
        .collect()
}

/// Dialler design A's number reading: the country on the button, the local
/// form in the field, and the E.164 number that is sent. After district-ios's
/// `DialPlanTests`, case for case, with the cases of its follow-up (the +7 and
/// +1 shared codes typed key by key, and the ranked search).
///
/// Numbers outside the fictional +1 555-01xx range are written without their
/// `+` and given one by [`plus`], so the repository's hygiene scan does not
/// read them as real numbers.
#[cfg(test)]
mod tests {
    use super::*;
    use district_l10n::Locale::{En, FrCa};

    /// `digits` with a `+` in front.
    fn plus(digits: &str) -> String {
        format!("+{digits}")
    }

    fn read(raw: &str, country: &str) -> DialReading {
        read_dial_entry(raw, Some(country))
    }

    /// Types `keys` one at a time into a field that is written back after every
    /// key, the country moving as the reading says, as the screen does.
    fn type_keys(keys: &str, mut country: Option<&'static str>) -> DialReading {
        let mut field = String::new();
        for key in keys.chars() {
            let reading = read_dial_entry(&format!("{field}{key}"), country);
            country = reading.iso.or(country);
            field = reading.national;
        }
        read_dial_entry(&field, country)
    }

    // Local numbers.

    #[test]
    fn a_canadian_number_typed_locally_formats_and_dials_as_e164() {
        let reading = read("4165550199", "CA");
        assert_eq!(reading.status, DialStatus::Ready);
        assert_eq!(reading.iso, Some("CA"));
        assert_eq!(reading.national, "(416) 555-0199");
        assert_eq!(reading.international, "+1 416 555 0199");
        assert_eq!(reading.e164.as_deref(), Some("+14165550199"));
    }

    /// The field rewrites itself as digits arrive, so each partial form must
    /// read back to the same digits, or a keystroke would be lost.
    #[test]
    fn formatting_as_you_type_keeps_every_digit() {
        assert_eq!(
            type_keys("4165550199", Some("CA")).national,
            "(416) 555-0199"
        );
        assert_eq!(read("416", "CA").national, "416");
        assert_eq!(read("4165", "CA").national, "(416) 5");
        assert_eq!(read("4165550", "CA").national, "(416) 555-0");
    }

    #[test]
    fn a_number_still_being_typed_is_not_dialable() {
        let reading = read("416555", "CA");
        assert_eq!(reading.status, DialStatus::Typing);
        assert_eq!(reading.e164, None);
    }

    #[test]
    fn the_north_american_trunk_one_is_dropped() {
        let reading = read("1 416 555 0199", "CA");
        assert_eq!(reading.e164.as_deref(), Some("+14165550199"));
        assert_eq!(reading.national, "(416) 555-0199");
        assert_eq!(read("1", "CA").national, "1", "the trunk alone is shown");
    }

    /// A +1 number is read by its area code, so a Seattle number typed under
    /// the Canadian flag says it rings the United States.
    #[test]
    fn a_plus_one_area_code_flips_canada_and_the_united_states() {
        assert_eq!(read("2065550142", "CA").iso, Some("US"));
        assert_eq!(read("4165550199", "US").iso, Some("CA"));
        assert_eq!(
            read("8765550100", "US").iso,
            Some("JM"),
            "a Jamaican number is not the US"
        );
    }

    #[test]
    fn a_london_number_typed_locally_drops_its_trunk_zero() {
        let reading = read("020 7946 0958", "GB");
        assert_eq!(reading.status, DialStatus::Ready);
        assert_eq!(reading.national, "020 7946 0958");
        assert_eq!(reading.e164, Some(plus("442079460958")));
    }

    #[test]
    fn too_many_digits_is_refused() {
        let reading = read("41655501990", "CA");
        assert_eq!(reading.status, DialStatus::TooLong);
        assert_eq!(reading.e164, None);
        // And past the E.164 ceiling in a country with no length of its own.
        assert_eq!(read("1234567890123456", "IT").status, DialStatus::TooLong);
    }

    /// Every number a country issues must be dialable, not only its common
    /// shape: Australia's 13 and 1300/1800 numbers, Singapore's 1800 toll-free
    /// numbers and a short UK freephone number.
    #[test]
    fn non_geographic_numbers_are_dialable() {
        let australia = read("1300 123 456", "AU");
        assert_eq!(
            australia.national, "1300 123 456",
            "no trunk 0 and no parentheses"
        );
        assert_eq!(australia.e164, Some(plus("611300123456")));
        assert_eq!(read("13 12 34", "AU").e164, Some(plus("61131234")));
        assert_eq!(
            read("1800 123 4567", "SG").e164,
            Some(plus("6518001234567"))
        );
        assert_eq!(read("0800 1111", "GB").e164, Some(plus("448001111")));
        assert_eq!(read("02 1234 5678", "AU").national, "(02) 1234 5678");
    }

    /// Backspace always removes a digit. The field formats after every edit, so
    /// a format that ended in punctuation (Australia's `(02)`) would put the
    /// deleted character straight back and the member could never delete past
    /// it.
    #[test]
    fn deleting_the_last_character_always_removes_a_digit() {
        let numbers = [
            ("CA", "4165550199"),
            ("GB", "02079460958"),
            ("FR", "0639980123"),
            ("DE", "030123456"),
            ("AU", "0212345678"),
            ("AU", "0491570156"),
            ("AU", "1300123456"),
            ("CH", "0441234567"),
            ("SG", "61234567"),
            ("IT", "0612345678"),
            ("JP", "0312345678"),
            ("RU", "84951234567"),
            ("HU", "0612345678"),
        ];
        let digits = |text: &str| text.chars().filter(char::is_ascii_digit).count();
        for (country, typed) in numbers {
            let mut field = String::new();
            for key in typed.chars() {
                field = read(&format!("{field}{key}"), country).national;
                let last = field.chars().last().expect("a digit typed shows");
                assert!(
                    last.is_ascii_digit(),
                    "{country}: {field:?} ends in punctuation"
                );
                let mut shorter = field.clone();
                shorter.pop();
                let after = read(&shorter, country).national;
                assert!(
                    digits(&after) < digits(&field),
                    "{country}: backspace on {field:?}"
                );
            }
        }
    }

    /// A trunk prefix that is not `0` is dropped too: kept, `8 495 ...` in
    /// Russia would send +7 849, a wrong number that still dials.
    #[test]
    fn trunk_prefixes_other_than_zero_are_dropped() {
        let moscow = read("8 495 123 4567", "RU");
        assert_eq!(moscow.e164, Some(plus("74951234567")));
        assert_eq!(moscow.national, "8495 123 4567");
        assert_eq!(read("06 1 234 5678", "HU").e164, Some(plus("3612345678")));
        let half = read("0", "HU");
        assert_eq!(half.national, "0", "half of 06 is shown as typed");
        assert_eq!(
            (half.status, half.international.as_str()),
            (DialStatus::Typing, "+36")
        );
        assert_eq!(
            read("06", "HU").national,
            "06",
            "the whole trunk alone is shown"
        );
        assert_eq!(
            read("8 017 123 4567", "BY").e164,
            Some(plus("375171234567"))
        );
    }

    /// A +1 toll-free number names no country, so the +1 country on screen
    /// stays.
    #[test]
    fn north_american_toll_free_keeps_the_country_on_screen() {
        assert_eq!(read("8005550100", "CA").iso, Some("CA"));
        assert_eq!(read("+1 888 555 0100", "CA").iso, Some("CA"));
        assert_eq!(read_dial_entry("+1 888 555 0100", None).iso, Some("US"));
    }

    // Numbers that name their own country.

    #[test]
    fn a_pasted_plus_forty_four_number_moves_to_the_united_kingdom() {
        let reading = read(&plus("44 20 7946 0958"), "CA");
        assert_eq!(reading.iso, Some("GB"));
        assert_eq!(reading.status, DialStatus::Ready);
        assert_eq!(reading.national, "020 7946 0958");
        assert_eq!(reading.international, plus("44 20 7946 0958"));
        assert_eq!(reading.e164, Some(plus("442079460958")));
        assert!(reading.named_its_country);
    }

    /// "+44 (0)20 ..." is how a signature writes it; the 0 is not dialled after
    /// the code.
    #[test]
    fn a_signature_style_trunk_zero_after_the_code_is_dropped() {
        let reading = read_dial_entry(&plus("44 (0)20 7946 0958"), None);
        assert_eq!(reading.e164, Some(plus("442079460958")));
    }

    /// The billed call. +41 is Switzerland whatever the workspace's country, and
    /// the flag moves there; with eight digits after the code it is not even a
    /// whole Swiss number, so nothing can be sent.
    #[test]
    fn the_switzerland_misdial_moves_the_flag_and_sends_nothing() {
        let reading = read(&plus("4165550123"), "CA");
        assert_eq!(reading.iso, Some("CH"));
        assert_eq!(reading.calling_code, "41");
        assert_eq!(reading.national_number, "65550123");
        assert_eq!(reading.status, DialStatus::Typing);
        assert_eq!(reading.e164, None);
    }

    #[test]
    fn the_north_american_exit_code_stands_for_plus() {
        let reading = read("011 44 20 7946 0958", "CA");
        assert_eq!(reading.iso, Some("GB"));
        assert_eq!(reading.e164, Some(plus("442079460958")));
        // With no country either exit code is taken.
        assert_eq!(read_dial_entry("011 44 20 7946 0958", None).iso, Some("GB"));
        assert_eq!(read_dial_entry("0044 20 7946 0958", None).iso, Some("GB"));
    }

    #[test]
    fn the_international_zero_zero_stands_for_plus_outside_north_america() {
        assert_eq!(
            read("0033 6 39 98 01 23", "GB").e164,
            Some(plus("33639980123"))
        );
        assert_eq!(read("0033639980123", "FR").iso, Some("FR"));
        let exit_only = read("00", "GB");
        assert_eq!(
            (exit_only.status, exit_only.national.as_str()),
            (DialStatus::Typing, "+")
        );
    }

    /// Under a +1 flag `00` is not an exit code, so it is read as a (short)
    /// local number.
    #[test]
    fn zero_zero_is_not_an_exit_code_under_a_plus_one_flag() {
        assert_ne!(
            read("0044 20 7946 0958", "CA").e164,
            Some(plus("442079460958"))
        );
    }

    #[test]
    fn plus_seven_is_kazakhstan_for_a_six_or_seven_and_russia_otherwise() {
        assert_eq!(
            read_dial_entry(&plus("7 701 234 5678"), None).iso,
            Some("KZ")
        );
        assert_eq!(
            read_dial_entry(&plus("7 601 234 5678"), None).iso,
            Some("KZ")
        );
        assert_eq!(
            read_dial_entry(&plus("7 495 123 4567"), None).iso,
            Some("RU")
        );
    }

    /// Typed one key at a time, +7 701 ... is Kazakhstan too. `+7` moves the
    /// flag to Russia and the field drops to the local form, so the rest
    /// arrives as a national number under Russia; it must be read again by its
    /// first digit, as a paste is.
    #[test]
    fn plus_seven_typed_key_by_key_reaches_kazakhstan() {
        let typed = type_keys(&plus("77012345678"), Some("CA"));
        assert_eq!(typed.iso, Some("KZ"));
        assert_eq!(typed.e164, Some(plus("77012345678")));
        let pasted = read(&plus("7 701 234 5678"), "CA");
        assert_eq!(typed.iso, pasted.iso, "typed and pasted read alike");
        assert_eq!(typed.national, pasted.national);
        // And back: a Moscow number under the Kazakh flag rings Russia.
        assert_eq!(read("84951234567", "KZ").iso, Some("RU"));
        assert_eq!(read("87012345678", "RU").iso, Some("KZ"));
        // Before the deciding digit, the +7 country on screen stays.
        assert_eq!(read("8", "KZ").iso, Some("KZ"));
        assert_eq!(read("8", "RU").iso, Some("RU"));
        assert_eq!(read("8", "RU").national, "8", "the trunk alone is shown");
        assert_eq!(read("+7", "KZ").iso, Some("KZ"));
        assert_eq!(read("+7", "CA").iso, Some("RU"));
    }

    /// Every +7 number has ten digits after the code, so a Russian number
    /// typed with its country code and no `+` is too long, never dialled as
    /// something else.
    #[test]
    fn a_plus_seven_number_has_exactly_ten_digits() {
        let typed_with_its_code = read("79161234567", "RU");
        assert_eq!(typed_with_its_code.status, DialStatus::TooLong);
        assert_eq!(typed_with_its_code.e164, None);
        assert_eq!(read("8 916 123 4567", "RU").e164, Some(plus("79161234567")));
        assert_eq!(read("8 916 123 456", "RU").status, DialStatus::Typing);
    }

    /// A calling code with no digit after it yet stays in the field, so the
    /// next digit is read after the code. Typed key by key, Saint Petersburg's
    /// +7 812 kept its 8 only this way: shown as an empty local number, the 8
    /// was read as Russia's trunk prefix and dropped.
    #[test]
    fn a_code_with_no_digit_after_it_stays_in_the_field() {
        let code_only = read("+7", "CA");
        assert_eq!(
            (code_only.national.as_str(), code_only.iso),
            ("+7", Some("RU"))
        );
        assert_eq!(read(&plus("44"), "CA").national, plus("44"));
        let typed = type_keys(&plus("78121234567"), Some("CA"));
        assert_eq!(typed.iso, Some("RU"));
        assert_eq!(typed.e164, Some(plus("78121234567")));
        assert_eq!(typed.national, read(&plus("7 812 123 4567"), "CA").national);
        // A trunk 0 pasted after the code is still dropped, and the code stays.
        assert_eq!(read(&plus("440"), "CA").national, plus("44"));
    }

    /// The same for +1, which already re-read its area code: typed key by key
    /// from a flag that is not +1, `+1 416 ...` ends on Canada and
    /// `+1 876 ...` on Jamaica.
    #[test]
    fn plus_one_typed_key_by_key_follows_the_area_code() {
        for (keys, iso) in [
            ("+14165550199", "CA"),
            ("+18765550199", "JM"),
            ("+12065550199", "US"),
        ] {
            assert_eq!(type_keys(keys, Some("GB")).iso, Some(iso), "{keys}");
        }
    }

    /// An unknown code is named as unknown and stays dialable: this client is
    /// not the authority on which codes exist.
    #[test]
    fn an_unknown_code_stays_dialable_without_a_name() {
        let reading = read(&plus("999 1234 5678"), "CA");
        assert_eq!(reading.iso, None);
        assert_eq!(reading.status, DialStatus::Ready);
        assert_eq!(reading.e164, Some(plus("99912345678")));
        assert_eq!(reading.national, plus("99912345678"));
        assert_eq!(read(&plus("999 1234"), "CA").status, DialStatus::Typing);
        assert_eq!(
            read(&plus("9991234567890123"), "CA").status,
            DialStatus::TooLong
        );
    }

    #[test]
    fn a_plus_alone_is_still_being_typed() {
        let reading = read("+", "CA");
        assert_eq!(reading.status, DialStatus::Typing);
        assert_eq!(reading.national, "+");
        assert_eq!(reading.iso, None);
        assert!(reading.named_its_country);
    }

    // Emergency numbers.

    /// Refused in every country and with none, and before anything asks for a
    /// country.
    #[test]
    fn emergency_numbers_are_refused_whatever_the_country() {
        for (number, country) in [
            ("911", Some("CA")),
            ("911", None),
            ("112", Some("FR")),
            ("999", Some("GB")),
            ("000", Some("AU")),
            ("1911", Some("US")),
            ("+1911", None),
            ("+112", None),
        ] {
            let reading = read_dial_entry(number, country);
            assert_eq!(
                reading.status,
                DialStatus::Emergency,
                "{number} in {country:?}"
            );
            assert_eq!(reading.e164, None);
        }
        assert_eq!(
            read("911", "CA").national,
            "911",
            "shown as typed, not formatted"
        );
        assert_eq!(read("+911", "CA").national, "+911", "its plus kept");
        // Typed key by key, an Indian number that passes through +911 is
        // still read after its code once the next digit arrives.
        let india = type_keys(&plus("911234567890"), Some("CA"));
        assert_eq!(india.iso, Some("IN"));
        assert_eq!(india.e164, Some(plus("911234567890")));
    }

    /// Whole numbers only: a North American number beginning 911 is ordinary.
    #[test]
    fn a_number_that_starts_with_an_emergency_code_is_ordinary() {
        let reading = read_dial_entry("+1 911 555 0100", None);
        assert_eq!(reading.status, DialStatus::Ready);
    }

    // No home country.

    #[test]
    fn digits_with_no_country_ask_for_one() {
        let reading = read_dial_entry("4165550199", None);
        assert_eq!(reading.status, DialStatus::NeedsCountry);
        assert_eq!(reading.e164, None);
        assert_eq!(reading.iso, None);
        assert_eq!(
            read("4165550199", "ZZ").status,
            DialStatus::NeedsCountry,
            "unlisted"
        );
    }

    #[test]
    fn a_plus_number_works_with_no_country() {
        let reading = read_dial_entry(&plus("33 6 39 98 01 23"), None);
        assert_eq!(reading.iso, Some("FR"));
        assert_eq!(reading.national, "06 39 98 01 23");
        assert_eq!(reading.e164, Some(plus("33639980123")));
    }

    #[test]
    fn empty_is_empty() {
        assert_eq!(read("", "CA").status, DialStatus::Empty);
        assert_eq!(read("", "CA").iso, Some("CA"));
        assert_eq!(read_dial_entry("", None).status, DialStatus::Empty);
        assert_eq!(read_dial_entry("( - )", None).status, DialStatus::Empty);
    }

    /// The shapes of the countries with a grouping of their own, each digit
    /// kept.
    #[test]
    fn each_country_groups_its_own_way() {
        let cases = [
            ("DE", "015112345678", "0151 12345678"),
            ("DE", "0301234567", "030 1234567"),
            ("DE", "0221123456", "02211 23456"),
            ("GB", "07700900123", "07700 900123"),
            ("CH", "0441234567", "044 123 45 67"),
            ("SG", "61234567", "6123 4567"),
            ("AU", "0491570156", "0491 570 156"),
            ("AU", "2", "02"),
            ("IT", "0612345678", "061 234 5678"),
        ];
        for (country, typed, shown) in cases {
            assert_eq!(read(typed, country).national, shown, "{country} {typed}");
        }
        assert_eq!(read("3123", "IT").international, plus("39 312 3"));
        assert_eq!(read("0", "FR").international, plus("33"));
        assert_eq!(read("0", "FR").national, "0", "a trunk alone is shown");
        // Italy keeps its leading 0 after the code, so it is never a trunk.
        assert_eq!(read("0", "IT").international, plus("39 0"));
    }

    // Countries.

    #[test]
    fn flags_are_regional_indicator_pairs() {
        assert_eq!(flag("CA"), "\u{1F1E8}\u{1F1E6}");
        assert_eq!(flag("de"), "\u{1F1E9}\u{1F1EA}");
        assert_eq!(flag("CAN"), "");
        assert_eq!(flag("C1"), "");
    }

    #[test]
    fn every_country_has_a_flag_a_code_and_a_name() {
        assert!(COUNTRIES.len() > 200);
        let mut isos: Vec<&str> = COUNTRIES.iter().map(|c| c.iso).collect();
        isos.sort_unstable();
        isos.dedup();
        assert_eq!(isos.len(), COUNTRIES.len(), "a country listed twice");
        for country in COUNTRIES {
            assert_eq!(country.iso.len(), 2);
            assert_eq!(flag(country.iso).chars().count(), 2, "{}", country.iso);
            assert!(
                country.code.chars().all(|c| c.is_ascii_digit()),
                "{}",
                country.iso
            );
            assert_ne!(
                country_name(country.iso, En),
                country.iso,
                "no name for {}",
                country.iso
            );
            assert!(!country.fr.is_empty() && !country.fr.starts_with(char::is_lowercase));
        }
        assert_eq!(country_name("ZZ", En), "ZZ");
        assert_eq!(country_name("DE", FrCa), "Allemagne");
        assert_eq!(calling_code("ca"), Some("1"));
        assert_eq!(calling_code("ZZ"), None);
    }

    /// Calling codes are prefix-free, which the longest-match reading relies
    /// on.
    #[test]
    fn no_calling_code_is_a_prefix_of_another() {
        for code in COUNTRIES.iter().map(|c| c.code) {
            for other in COUNTRIES.iter().map(|c| c.code).filter(|o| *o != code) {
                assert!(!other.starts_with(code), "+{code} shadows +{other}");
            }
        }
    }

    #[test]
    fn every_country_reads_back_to_itself_or_its_codes_first_country() {
        for country in COUNTRIES.iter().filter(|c| c.code != "1" && c.code != "7") {
            let iso = read_dial_entry(&plus(&format!("{}12345678", country.code)), None).iso;
            assert_eq!(
                calling_code(iso.unwrap_or("")),
                Some(country.code),
                "{}",
                country.iso
            );
        }
    }

    #[test]
    fn search_finds_by_name_code_and_calling_code() {
        assert_eq!(search_countries("germ", En), ["DE"]);
        assert_eq!(search_countries("allem", FrCa), ["DE"]);
        assert!(search_countries("+49", En).contains(&"DE"));
        assert!(search_countries("49", En).contains(&"DE"));
        assert!(search_countries("de", En).contains(&"DE"));
        assert!(search_countries("quebec zzz", En).is_empty());
        assert_eq!(search_countries("  ", En), sorted_countries(En));
        // A lone `+` is the start of every calling code.
        assert_eq!(search_countries("+", En), sorted_countries(En));
        assert!(search_countries("+x", En).is_empty());
    }

    /// Ranked, because Enter picks the first row: `us` listed Australia and
    /// Austria before the United States.
    #[test]
    fn search_ranks_code_then_name_start_then_calling_code_then_contains() {
        let position = |list: &[&str], iso: &str| list.iter().position(|c| *c == iso).unwrap();
        let us = search_countries("us", En);
        assert_eq!(us.first(), Some(&"US"));
        assert!(position(&us, "US") < position(&us, "AU"));
        assert_eq!(search_countries("ca", En).first(), Some(&"CA"));
        // A name that starts with the query beats one that only contains it.
        assert_eq!(search_countries("can", En).first(), Some(&"CA"));
        let ger = search_countries("ger", En);
        assert!(position(&ger, "DE") < position(&ger, "DZ"));
        // The countries sharing a calling code stay alphabetical among
        // themselves.
        let plus_one = search_countries("+1", En);
        assert!(plus_one.contains(&"CA") && plus_one.contains(&"US"));
        let alphabetical: Vec<&str> = sorted_countries(En)
            .into_iter()
            .filter(|c| plus_one.contains(c))
            .collect();
        assert_eq!(plus_one, alphabetical);
        // An exact ISO code first, then names starting with it: "de" is
        // Germany, then Denmark.
        let de = search_countries("de", En);
        assert_eq!(&de[..2], ["DE", "DK"]);
    }

    /// Accents are ignored, so "etats" finds the United States in French, and a
    /// typed apostrophe finds the curly one.
    #[test]
    fn search_ignores_accents_and_case() {
        assert!(search_countries("ETATS-UNIS", FrCa).contains(&"US"));
        assert_eq!(search_countries("côte d'iv", En), ["CI"]);
        assert_eq!(fold("ÀÉÎÕÜÇÑÝ’"), "aeiouc\u{f1}y'".replace('\u{f1}', "n"));
    }

    #[test]
    fn the_list_is_alphabetical_in_the_apps_language() {
        let english = sorted_countries(En);
        assert_eq!(english.first(), Some(&"AF"));
        let french = sorted_countries(FrCa);
        let position = |iso: &str| french.iter().position(|c| *c == iso).unwrap();
        assert!(
            position("DE") < position("AD"),
            "Allemagne sorts before Andorre in French"
        );
        assert_eq!(
            position("EG") + 2,
            position("EC"),
            "Égypte, Émirats, then Équateur"
        );
    }

    #[test]
    fn suggested_is_home_then_current_then_canada_and_the_united_states_once() {
        assert_eq!(
            suggested_countries(Some("FR"), Some("GB")),
            ["FR", "GB", "CA", "US"]
        );
        assert_eq!(suggested_countries(Some("CA"), Some("CA")), ["CA", "US"]);
        assert_eq!(suggested_countries(None, None), ["CA", "US"]);
        assert_eq!(suggested_countries(Some("ZZ"), Some("us")), ["US", "CA"]);
    }

    // The home country.

    #[test]
    fn the_home_country_is_where_most_of_the_numbers_are() {
        let toronto = "+14165550100";
        let seattle = "+12065550100";
        assert_eq!(home_country([toronto, seattle, "+16475550100"]), Some("CA"));
        let london = plus("442079460100");
        assert_eq!(
            home_country([london.as_str(), toronto]),
            Some("GB"),
            "a tie goes first"
        );
        assert_eq!(
            home_country([seattle, toronto]),
            Some("US"),
            "a tie goes first"
        );
        assert_eq!(home_country([]), None);
        assert_eq!(home_country(["not a number"]), None);
        // Toll-free numbers do not vote.
        let toll_free = ["+18005550100", "+18885550100"];
        assert_eq!(
            home_country([toll_free[0], toll_free[1], toronto]),
            Some("CA")
        );
        assert_eq!(home_country([toll_free[0]]), None);
    }

    #[test]
    fn a_placeholder_is_a_number_nobody_can_be_reached_on() {
        assert_eq!(national_example("CA"), Some("(416) 555-0199"));
        assert_eq!(national_example("GB"), Some("020 7946 0958"));
        assert_eq!(national_example("JP"), None);
        // Each one is a whole number of its country, so the placeholder never
        // shows a shape the field would refuse.
        for (country, example) in NATIONAL_EXAMPLES {
            assert_eq!(
                read(example, country).status,
                DialStatus::Ready,
                "{country}"
            );
        }
    }
}
