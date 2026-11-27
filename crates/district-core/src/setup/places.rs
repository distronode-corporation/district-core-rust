// SPDX-FileCopyrightText: Distronode Corporation
//
// SPDX-License-Identifier: Apache-2.0

//! The places step 1 offers and the time zone each implies, as the web's
//! `SETUP_COUNTRIES`, provinces and states hold them.

use district_l10n::Locale;

use crate::dialer::plan::country_name as dialer_country_name;

/// A time zone, and whether its place spans more than one (the owner may
/// want another in step 4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Zone {
    /// The IANA name.
    pub timezone: &'static str,
    /// Whether the place spans more than one zone.
    pub multi_zone: bool,
}

/// A province, territory or state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Subdivision {
    /// Its postal code, for example `QC`.
    pub code: &'static str,
    /// Its English name.
    pub en: &'static str,
    /// Its French name.
    pub fr: &'static str,
    /// The zone it implies.
    pub zone: Zone,
}

impl Subdivision {
    /// Its name in `locale`.
    pub fn name(&self, locale: Locale) -> &'static str {
        match locale {
            Locale::En => self.en,
            Locale::FrCa => self.fr,
        }
    }
}

const fn sub(
    code: &'static str,
    en: &'static str,
    fr: &'static str,
    timezone: &'static str,
    multi_zone: bool,
) -> Subdivision {
    Subdivision {
        code,
        en,
        fr,
        zone: Zone {
            timezone,
            multi_zone,
        },
    }
}

/// Canada's provinces and territories.
pub const PROVINCES: &[Subdivision] = &[
    sub("AB", "Alberta", "Alberta", "America/Edmonton", false),
    sub(
        "BC",
        "British Columbia",
        "Colombie-Britannique",
        "America/Vancouver",
        true,
    ),
    sub("MB", "Manitoba", "Manitoba", "America/Winnipeg", false),
    sub(
        "NB",
        "New Brunswick",
        "Nouveau-Brunswick",
        "America/Moncton",
        false,
    ),
    sub(
        "NL",
        "Newfoundland and Labrador",
        "Terre-Neuve-et-Labrador",
        "America/St_Johns",
        true,
    ),
    sub(
        "NS",
        "Nova Scotia",
        "Nouvelle-Écosse",
        "America/Halifax",
        false,
    ),
    sub(
        "NT",
        "Northwest Territories",
        "Territoires du Nord-Ouest",
        "America/Yellowknife",
        false,
    ),
    sub("NU", "Nunavut", "Nunavut", "America/Iqaluit", true),
    sub("ON", "Ontario", "Ontario", "America/Toronto", true),
    sub(
        "PE",
        "Prince Edward Island",
        "Île-du-Prince-Édouard",
        "America/Halifax",
        false,
    ),
    sub("QC", "Quebec", "Québec", "America/Toronto", true),
    sub("SK", "Saskatchewan", "Saskatchewan", "America/Regina", true),
    sub("YT", "Yukon", "Yukon", "America/Whitehorse", false),
];

/// The states of the United States, and the District of Columbia.
pub const STATES: &[Subdivision] = &[
    sub("AL", "Alabama", "Alabama", "America/Chicago", false),
    sub("AK", "Alaska", "Alaska", "America/Anchorage", true),
    sub("AZ", "Arizona", "Arizona", "America/Phoenix", false),
    sub("AR", "Arkansas", "Arkansas", "America/Chicago", false),
    sub(
        "CA",
        "California",
        "Californie",
        "America/Los_Angeles",
        false,
    ),
    sub("CO", "Colorado", "Colorado", "America/Denver", false),
    sub(
        "CT",
        "Connecticut",
        "Connecticut",
        "America/New_York",
        false,
    ),
    sub("DE", "Delaware", "Delaware", "America/New_York", false),
    sub(
        "DC",
        "District of Columbia",
        "District de Columbia",
        "America/New_York",
        false,
    ),
    sub("FL", "Florida", "Floride", "America/New_York", true),
    sub("GA", "Georgia", "Géorgie", "America/New_York", false),
    sub("HI", "Hawaii", "Hawaï", "Pacific/Honolulu", false),
    sub("ID", "Idaho", "Idaho", "America/Boise", true),
    sub("IL", "Illinois", "Illinois", "America/Chicago", false),
    sub(
        "IN",
        "Indiana",
        "Indiana",
        "America/Indiana/Indianapolis",
        true,
    ),
    sub("IA", "Iowa", "Iowa", "America/Chicago", false),
    sub("KS", "Kansas", "Kansas", "America/Chicago", true),
    sub("KY", "Kentucky", "Kentucky", "America/New_York", true),
    sub("LA", "Louisiana", "Louisiane", "America/Chicago", false),
    sub("ME", "Maine", "Maine", "America/New_York", false),
    sub("MD", "Maryland", "Maryland", "America/New_York", false),
    sub(
        "MA",
        "Massachusetts",
        "Massachusetts",
        "America/New_York",
        false,
    ),
    sub("MI", "Michigan", "Michigan", "America/Detroit", true),
    sub("MN", "Minnesota", "Minnesota", "America/Chicago", false),
    sub("MS", "Mississippi", "Mississippi", "America/Chicago", false),
    sub("MO", "Missouri", "Missouri", "America/Chicago", false),
    sub("MT", "Montana", "Montana", "America/Denver", false),
    sub("NE", "Nebraska", "Nebraska", "America/Chicago", true),
    sub("NV", "Nevada", "Nevada", "America/Los_Angeles", false),
    sub(
        "NH",
        "New Hampshire",
        "New Hampshire",
        "America/New_York",
        false,
    ),
    sub("NJ", "New Jersey", "New Jersey", "America/New_York", false),
    sub(
        "NM",
        "New Mexico",
        "Nouveau-Mexique",
        "America/Denver",
        false,
    ),
    sub("NY", "New York", "New York", "America/New_York", false),
    sub(
        "NC",
        "North Carolina",
        "Caroline du Nord",
        "America/New_York",
        false,
    ),
    sub(
        "ND",
        "North Dakota",
        "Dakota du Nord",
        "America/Chicago",
        true,
    ),
    sub("OH", "Ohio", "Ohio", "America/New_York", false),
    sub("OK", "Oklahoma", "Oklahoma", "America/Chicago", false),
    sub("OR", "Oregon", "Oregon", "America/Los_Angeles", true),
    sub(
        "PA",
        "Pennsylvania",
        "Pennsylvanie",
        "America/New_York",
        false,
    ),
    sub(
        "RI",
        "Rhode Island",
        "Rhode Island",
        "America/New_York",
        false,
    ),
    sub(
        "SC",
        "South Carolina",
        "Caroline du Sud",
        "America/New_York",
        false,
    ),
    sub(
        "SD",
        "South Dakota",
        "Dakota du Sud",
        "America/Chicago",
        true,
    ),
    sub("TN", "Tennessee", "Tennessee", "America/Chicago", true),
    sub("TX", "Texas", "Texas", "America/Chicago", true),
    sub("UT", "Utah", "Utah", "America/Denver", false),
    sub("VT", "Vermont", "Vermont", "America/New_York", false),
    sub("VA", "Virginia", "Virginie", "America/New_York", false),
    sub(
        "WA",
        "Washington",
        "Washington",
        "America/Los_Angeles",
        false,
    ),
    sub(
        "WV",
        "West Virginia",
        "Virginie-Occidentale",
        "America/New_York",
        false,
    ),
    sub("WI", "Wisconsin", "Wisconsin", "America/Chicago", false),
    sub("WY", "Wyoming", "Wyoming", "America/Denver", false),
];

/// Every other country step 1 offers, in the web's order, with its zone.
pub const COUNTRY_ZONES: &[(&str, Zone)] = &[
    (
        "AT",
        Zone {
            timezone: "Europe/Vienna",
            multi_zone: false,
        },
    ),
    (
        "BE",
        Zone {
            timezone: "Europe/Brussels",
            multi_zone: false,
        },
    ),
    (
        "BG",
        Zone {
            timezone: "Europe/Sofia",
            multi_zone: false,
        },
    ),
    (
        "HR",
        Zone {
            timezone: "Europe/Zagreb",
            multi_zone: false,
        },
    ),
    (
        "CY",
        Zone {
            timezone: "Asia/Nicosia",
            multi_zone: false,
        },
    ),
    (
        "CZ",
        Zone {
            timezone: "Europe/Prague",
            multi_zone: false,
        },
    ),
    (
        "DK",
        Zone {
            timezone: "Europe/Copenhagen",
            multi_zone: false,
        },
    ),
    (
        "EE",
        Zone {
            timezone: "Europe/Tallinn",
            multi_zone: false,
        },
    ),
    (
        "FI",
        Zone {
            timezone: "Europe/Helsinki",
            multi_zone: false,
        },
    ),
    (
        "FR",
        Zone {
            timezone: "Europe/Paris",
            multi_zone: false,
        },
    ),
    (
        "DE",
        Zone {
            timezone: "Europe/Berlin",
            multi_zone: false,
        },
    ),
    (
        "GR",
        Zone {
            timezone: "Europe/Athens",
            multi_zone: false,
        },
    ),
    (
        "HU",
        Zone {
            timezone: "Europe/Budapest",
            multi_zone: false,
        },
    ),
    (
        "IE",
        Zone {
            timezone: "Europe/Dublin",
            multi_zone: false,
        },
    ),
    (
        "IT",
        Zone {
            timezone: "Europe/Rome",
            multi_zone: false,
        },
    ),
    (
        "LV",
        Zone {
            timezone: "Europe/Riga",
            multi_zone: false,
        },
    ),
    (
        "LT",
        Zone {
            timezone: "Europe/Vilnius",
            multi_zone: false,
        },
    ),
    (
        "LU",
        Zone {
            timezone: "Europe/Luxembourg",
            multi_zone: false,
        },
    ),
    (
        "MT",
        Zone {
            timezone: "Europe/Malta",
            multi_zone: false,
        },
    ),
    (
        "NL",
        Zone {
            timezone: "Europe/Amsterdam",
            multi_zone: false,
        },
    ),
    (
        "PL",
        Zone {
            timezone: "Europe/Warsaw",
            multi_zone: false,
        },
    ),
    (
        "PT",
        Zone {
            timezone: "Europe/Lisbon",
            multi_zone: true,
        },
    ),
    (
        "RO",
        Zone {
            timezone: "Europe/Bucharest",
            multi_zone: false,
        },
    ),
    (
        "SK",
        Zone {
            timezone: "Europe/Bratislava",
            multi_zone: false,
        },
    ),
    (
        "SI",
        Zone {
            timezone: "Europe/Ljubljana",
            multi_zone: false,
        },
    ),
    (
        "ES",
        Zone {
            timezone: "Europe/Madrid",
            multi_zone: true,
        },
    ),
    (
        "SE",
        Zone {
            timezone: "Europe/Stockholm",
            multi_zone: false,
        },
    ),
    (
        "IS",
        Zone {
            timezone: "Atlantic/Reykjavik",
            multi_zone: false,
        },
    ),
    (
        "LI",
        Zone {
            timezone: "Europe/Vaduz",
            multi_zone: false,
        },
    ),
    (
        "NO",
        Zone {
            timezone: "Europe/Oslo",
            multi_zone: false,
        },
    ),
    (
        "GB",
        Zone {
            timezone: "Europe/London",
            multi_zone: false,
        },
    ),
    (
        "CH",
        Zone {
            timezone: "Europe/Zurich",
            multi_zone: false,
        },
    ),
    (
        "AU",
        Zone {
            timezone: "Australia/Sydney",
            multi_zone: true,
        },
    ),
    (
        "NZ",
        Zone {
            timezone: "Pacific/Auckland",
            multi_zone: false,
        },
    ),
    (
        "SG",
        Zone {
            timezone: "Asia/Singapore",
            multi_zone: false,
        },
    ),
    (
        "HK",
        Zone {
            timezone: "Asia/Hong_Kong",
            multi_zone: false,
        },
    ),
    (
        "JP",
        Zone {
            timezone: "Asia/Tokyo",
            multi_zone: false,
        },
    ),
    (
        "IN",
        Zone {
            timezone: "Asia/Kolkata",
            multi_zone: false,
        },
    ),
    (
        "PH",
        Zone {
            timezone: "Asia/Manila",
            multi_zone: false,
        },
    ),
];

/// The countries step 1 offers: Canada and the United States first, then the
/// rest in the web's order.
pub fn countries() -> Vec<&'static str> {
    let mut out = vec!["CA", "US"];
    out.extend(COUNTRY_ZONES.iter().map(|(code, _)| *code));
    out
}

/// A country's provinces or states, or `None` for a country step 1 asks no
/// subdivision of.
pub fn subdivisions(country: &str) -> Option<&'static [Subdivision]> {
    match country {
        "CA" => Some(PROVINCES),
        "US" => Some(STATES),
        _ => None,
    }
}

/// The zone a place implies, or `None` while its province or state is not
/// chosen, or for a country step 1 does not offer.
pub fn zone(country: &str, subdivision: &str) -> Option<Zone> {
    if let Some(list) = subdivisions(country) {
        let code = subdivision.to_ascii_uppercase();
        return list.iter().find(|s| s.code == code).map(|s| s.zone);
    }
    COUNTRY_ZONES
        .iter()
        .find(|(code, _)| *code == country)
        .map(|(_, zone)| *zone)
}

/// The country a workspace's region suggests: Canada for `ca`, the United
/// States for `us`, none otherwise.
pub fn default_country(region: Option<&str>) -> &'static str {
    match region {
        Some("ca") => "CA",
        Some("us") => "US",
        _ => "",
    }
}

/// The zones a country implies, sorted and each once.
pub fn zones(country: &str) -> Vec<&'static str> {
    let mut out: Vec<&'static str> = match subdivisions(country) {
        Some(list) => list.iter().map(|s| s.zone.timezone).collect(),
        None => COUNTRY_ZONES
            .iter()
            .filter(|(code, _)| *code == country)
            .map(|(_, zone)| zone.timezone)
            .collect(),
    };
    out.sort_unstable();
    out.dedup();
    out
}

/// The zones step 4 offers: the country's own, with a stored zone the list
/// does not hold kept in it, sorted.
pub fn zones_with(country: &str, stored: &str) -> Vec<String> {
    let mut out: Vec<String> = zones(country).into_iter().map(str::to_owned).collect();
    if !stored.is_empty() && !out.iter().any(|zone| zone == stored) {
        out.push(stored.to_owned());
        out.sort_unstable();
    }
    out
}

/// A country's name in `locale`, or its code when it has none.
pub fn country_name(code: &str, locale: Locale) -> String {
    dialer_country_name(code, locale).to_owned()
}

/// The subdivision `code` names in `country`: its own code when the country
/// has a list and it is on it, "" when it is not, and `code` as typed for a
/// country with no list.
pub fn subdivision_code(code: &str, country: &str) -> String {
    match subdivisions(country) {
        Some(list) => {
            let upper = code.to_ascii_uppercase();
            list.iter()
                .find(|s| s.code == upper)
                .map(|s| s.code.to_owned())
                .unwrap_or_default()
        }
        None => code.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The tables are built at compile time, so the helper that builds each
    /// row runs here once at run time too.
    #[test]
    fn a_row_holds_what_it_is_built_from() {
        assert_eq!(
            sub("QC", "Quebec", "Québec", "America/Toronto", true),
            PROVINCES[10]
        );
    }

    #[test]
    fn canada_and_the_united_states_come_first_and_have_subdivisions() {
        let all = countries();
        assert_eq!(&all[..2], ["CA", "US"]);
        assert_eq!(all.len(), 2 + COUNTRY_ZONES.len());
        assert_eq!(subdivisions("CA").map(<[_]>::len), Some(13));
        assert_eq!(subdivisions("US").map(<[_]>::len), Some(51));
        assert!(subdivisions("FR").is_none());
    }

    #[test]
    fn a_place_implies_its_zone() {
        assert_eq!(
            zone("CA", "qc").map(|z| z.timezone),
            Some("America/Toronto")
        );
        assert!(zone("CA", "qc").is_some_and(|z| z.multi_zone));
        assert_eq!(zone("CA", ""), None);
        assert_eq!(
            zone("FR", ""),
            Some(Zone {
                timezone: "Europe/Paris",
                multi_zone: false
            })
        );
        assert_eq!(zone("ZZ", ""), None);
        assert_eq!(default_country(Some("ca")), "CA");
        assert_eq!(default_country(Some("us")), "US");
        assert_eq!(default_country(Some("eu")), "");
        assert_eq!(default_country(None), "");
    }

    #[test]
    fn step_four_offers_the_countrys_zones_and_the_stored_one() {
        let canada = zones("CA");
        assert!(canada.contains(&"America/Toronto"));
        assert!(canada.windows(2).all(|w| w[0] < w[1]));
        assert_eq!(zones("FR"), ["Europe/Paris"]);
        assert!(zones("ZZ").is_empty());
        assert_eq!(zones_with("FR", "Europe/Paris"), ["Europe/Paris"]);
        assert_eq!(
            zones_with("FR", "America/Cayenne"),
            ["America/Cayenne", "Europe/Paris"]
        );
        assert_eq!(zones_with("FR", ""), ["Europe/Paris"]);
    }

    #[test]
    fn names_and_codes_read_in_the_language_shown() {
        let quebec = subdivisions("CA")
            .unwrap()
            .iter()
            .find(|s| s.code == "QC")
            .unwrap();
        assert_eq!(quebec.name(Locale::En), "Quebec");
        assert_eq!(quebec.name(Locale::FrCa), "Québec");
        assert_eq!(country_name("AT", Locale::FrCa), "Autriche");
        assert_eq!(subdivision_code("on", "CA"), "ON");
        assert_eq!(subdivision_code("ZZ", "CA"), "");
        assert_eq!(subdivision_code("Bavaria", "DE"), "Bavaria");
    }
}
