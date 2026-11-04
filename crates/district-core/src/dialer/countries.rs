//! The data the dial plan reads by: every country the dialler lists, with its
//! calling code and its name in English and Canadian French, and the few facts
//! about numbering plans the formatter needs. After district-ios's
//! `DialPlan+Countries.swift`, row for row.
//!
//! One country per calling code outside +1 and +7. Territories that share a
//! code with a larger country (Guernsey and Jersey on +44, the Vatican on +39,
//! Mayotte on +262) are left out rather than listed second, so a pasted number
//! never names a place it cannot tell apart.
//!
//! The names are the Unicode CLDR's (what the iOS app reads from its locale
//! and the web from `Intl.DisplayNames`), read with `Intl.DisplayNames` in
//! `en-CA` and `fr-CA`. They are locale data rather than copy, so they live
//! here and not in a catalog. Two changes from CLDR: a French name starts with
//! a capital, because it heads a row of a list (`Îles Cook`, `La Réunion`), and
//! a non-breaking hyphen is a plain one, so a search typed with `-` finds it.

/// One country of the list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DialCountry {
    /// The ISO 3166-1 alpha-2 code: `CA`.
    pub iso: &'static str,
    /// The calling code, digits only: `1`.
    pub code: &'static str,
    /// The name in English.
    pub en: &'static str,
    /// The name in Canadian French.
    pub fr: &'static str,
}

/// One row of [`COUNTRIES`]: a literal, so the table is data with no code to
/// run.
macro_rules! country {
    ($iso:literal, $code:literal, $en:literal, $fr:literal $(,)?) => {
        DialCountry {
            iso: $iso,
            code: $code,
            en: $en,
            fr: $fr,
        }
    };
}

/// Every country the dialler lists. The first one listed for a calling code is
/// the one a pasted `+` number of that code reads as, except +1 and +7, which
/// are read by the digits after them.
pub static COUNTRIES: &[DialCountry] = &[
    country!("CA", "1", "Canada", "Canada"),
    country!("US", "1", "United States", "États-Unis"),
    country!("AF", "93", "Afghanistan", "Afghanistan"),
    country!("AL", "355", "Albania", "Albanie"),
    country!("DZ", "213", "Algeria", "Algérie"),
    country!("AS", "1", "American Samoa", "Samoa américaines"),
    country!("AD", "376", "Andorra", "Andorre"),
    country!("AO", "244", "Angola", "Angola"),
    country!("AI", "1", "Anguilla", "Anguilla"),
    country!("AG", "1", "Antigua and Barbuda", "Antigua-et-Barbuda"),
    country!("AR", "54", "Argentina", "Argentine"),
    country!("AM", "374", "Armenia", "Arménie"),
    country!("AW", "297", "Aruba", "Aruba"),
    country!("AU", "61", "Australia", "Australie"),
    country!("AT", "43", "Austria", "Autriche"),
    country!("AZ", "994", "Azerbaijan", "Azerbaïdjan"),
    country!("BS", "1", "Bahamas", "Bahamas"),
    country!("BH", "973", "Bahrain", "Bahreïn"),
    country!("BD", "880", "Bangladesh", "Bangladesh"),
    country!("BB", "1", "Barbados", "Barbade"),
    country!("BY", "375", "Belarus", "Bélarus"),
    country!("BE", "32", "Belgium", "Belgique"),
    country!("BZ", "501", "Belize", "Bélize"),
    country!("BJ", "229", "Benin", "Bénin"),
    country!("BM", "1", "Bermuda", "Bermudes"),
    country!("BT", "975", "Bhutan", "Bhoutan"),
    country!("BO", "591", "Bolivia", "Bolivie"),
    country!("BA", "387", "Bosnia and Herzegovina", "Bosnie-Herzégovine"),
    country!("BW", "267", "Botswana", "Botswana"),
    country!("BR", "55", "Brazil", "Brésil"),
    country!(
        "VG",
        "1",
        "British Virgin Islands",
        "Îles Vierges britanniques",
    ),
    country!("BN", "673", "Brunei", "Brunéi"),
    country!("BG", "359", "Bulgaria", "Bulgarie"),
    country!("BF", "226", "Burkina Faso", "Burkina Faso"),
    country!("BI", "257", "Burundi", "Burundi"),
    country!("KH", "855", "Cambodia", "Cambodge"),
    country!("CM", "237", "Cameroon", "Cameroun"),
    country!("CV", "238", "Cape Verde", "Cap-Vert"),
    country!("KY", "1", "Cayman Islands", "Îles Caïmans"),
    country!(
        "CF",
        "236",
        "Central African Republic",
        "République centrafricaine",
    ),
    country!("TD", "235", "Chad", "Tchad"),
    country!("CL", "56", "Chile", "Chili"),
    country!("CN", "86", "China", "Chine"),
    country!("CO", "57", "Colombia", "Colombie"),
    country!("KM", "269", "Comoros", "Comores"),
    country!("CG", "242", "Congo - Brazzaville", "Congo-Brazzaville"),
    country!("CD", "243", "Congo - Kinshasa", "Congo-Kinshasa"),
    country!("CK", "682", "Cook Islands", "Îles Cook"),
    country!("CR", "506", "Costa Rica", "Costa Rica"),
    country!("CI", "225", "Côte d’Ivoire", "Côte d’Ivoire"),
    country!("HR", "385", "Croatia", "Croatie"),
    country!("CU", "53", "Cuba", "Cuba"),
    country!("CW", "599", "Curaçao", "Curaçao"),
    country!("CY", "357", "Cyprus", "Chypre"),
    country!("CZ", "420", "Czechia", "Tchéquie"),
    country!("DK", "45", "Denmark", "Danemark"),
    country!("DJ", "253", "Djibouti", "Djibouti"),
    country!("DM", "1", "Dominica", "Dominique"),
    country!("DO", "1", "Dominican Republic", "République dominicaine"),
    country!("EC", "593", "Ecuador", "Équateur"),
    country!("EG", "20", "Egypt", "Égypte"),
    country!("SV", "503", "El Salvador", "Salvador"),
    country!("GQ", "240", "Equatorial Guinea", "Guinée équatoriale"),
    country!("ER", "291", "Eritrea", "Érythrée"),
    country!("EE", "372", "Estonia", "Estonie"),
    country!("SZ", "268", "Eswatini", "Eswatini"),
    country!("ET", "251", "Ethiopia", "Éthiopie"),
    country!("FK", "500", "Falkland Islands", "Îles Malouines"),
    country!("FO", "298", "Faroe Islands", "Îles Féroé"),
    country!("FJ", "679", "Fiji", "Fidji"),
    country!("FI", "358", "Finland", "Finlande"),
    country!("FR", "33", "France", "France"),
    country!("GF", "594", "French Guiana", "Guyane française"),
    country!("PF", "689", "French Polynesia", "Polynésie française"),
    country!("GA", "241", "Gabon", "Gabon"),
    country!("GM", "220", "Gambia", "Gambie"),
    country!("GE", "995", "Georgia", "Géorgie"),
    country!("DE", "49", "Germany", "Allemagne"),
    country!("GH", "233", "Ghana", "Ghana"),
    country!("GI", "350", "Gibraltar", "Gibraltar"),
    country!("GR", "30", "Greece", "Grèce"),
    country!("GL", "299", "Greenland", "Groenland"),
    country!("GD", "1", "Grenada", "Grenade"),
    country!("GP", "590", "Guadeloupe", "Guadeloupe"),
    country!("GU", "1", "Guam", "Guam"),
    country!("GT", "502", "Guatemala", "Guatemala"),
    country!("GN", "224", "Guinea", "Guinée"),
    country!("GW", "245", "Guinea-Bissau", "Guinée-Bissau"),
    country!("GY", "592", "Guyana", "Guyana"),
    country!("HT", "509", "Haiti", "Haïti"),
    country!("HN", "504", "Honduras", "Honduras"),
    country!(
        "HK",
        "852",
        "Hong Kong SAR China",
        "R.A.S. chinoise de Hong Kong",
    ),
    country!("HU", "36", "Hungary", "Hongrie"),
    country!("IS", "354", "Iceland", "Islande"),
    country!("IN", "91", "India", "Inde"),
    country!("ID", "62", "Indonesia", "Indonésie"),
    country!("IR", "98", "Iran", "Iran"),
    country!("IQ", "964", "Iraq", "Irak"),
    country!("IE", "353", "Ireland", "Irlande"),
    country!("IL", "972", "Israel", "Israël"),
    country!("IT", "39", "Italy", "Italie"),
    country!("JM", "1", "Jamaica", "Jamaïque"),
    country!("JP", "81", "Japan", "Japon"),
    country!("JO", "962", "Jordan", "Jordanie"),
    country!("KZ", "7", "Kazakhstan", "Kazakhstan"),
    country!("KE", "254", "Kenya", "Kenya"),
    country!("KI", "686", "Kiribati", "Kiribati"),
    country!("XK", "383", "Kosovo", "Kosovo"),
    country!("KW", "965", "Kuwait", "Koweït"),
    country!("KG", "996", "Kyrgyzstan", "Kirghizistan"),
    country!("LA", "856", "Laos", "Laos"),
    country!("LV", "371", "Latvia", "Lettonie"),
    country!("LB", "961", "Lebanon", "Liban"),
    country!("LS", "266", "Lesotho", "Lesotho"),
    country!("LR", "231", "Liberia", "Libéria"),
    country!("LY", "218", "Libya", "Libye"),
    country!("LI", "423", "Liechtenstein", "Liechtenstein"),
    country!("LT", "370", "Lithuania", "Lituanie"),
    country!("LU", "352", "Luxembourg", "Luxembourg"),
    country!("MO", "853", "Macao SAR China", "R.A.S. chinoise de Macao"),
    country!("MG", "261", "Madagascar", "Madagascar"),
    country!("MW", "265", "Malawi", "Malawi"),
    country!("MY", "60", "Malaysia", "Malaisie"),
    country!("MV", "960", "Maldives", "Maldives"),
    country!("ML", "223", "Mali", "Mali"),
    country!("MT", "356", "Malta", "Malte"),
    country!("MH", "692", "Marshall Islands", "Îles Marshall"),
    country!("MQ", "596", "Martinique", "Martinique"),
    country!("MR", "222", "Mauritania", "Mauritanie"),
    country!("MU", "230", "Mauritius", "Maurice"),
    country!("MX", "52", "Mexico", "Mexique"),
    country!("FM", "691", "Micronesia", "Micronésie"),
    country!("MD", "373", "Moldova", "Moldavie"),
    country!("MC", "377", "Monaco", "Monaco"),
    country!("MN", "976", "Mongolia", "Mongolie"),
    country!("ME", "382", "Montenegro", "Monténégro"),
    country!("MS", "1", "Montserrat", "Montserrat"),
    country!("MA", "212", "Morocco", "Maroc"),
    country!("MZ", "258", "Mozambique", "Mozambique"),
    country!("MM", "95", "Myanmar (Burma)", "Myanmar"),
    country!("NA", "264", "Namibia", "Namibie"),
    country!("NR", "674", "Nauru", "Nauru"),
    country!("NP", "977", "Nepal", "Népal"),
    country!("NL", "31", "Netherlands", "Pays-Bas"),
    country!("NC", "687", "New Caledonia", "Nouvelle-Calédonie"),
    country!("NZ", "64", "New Zealand", "Nouvelle-Zélande"),
    country!("NI", "505", "Nicaragua", "Nicaragua"),
    country!("NE", "227", "Niger", "Niger"),
    country!("NG", "234", "Nigeria", "Nigéria"),
    country!("NU", "683", "Niue", "Niue"),
    country!("KP", "850", "North Korea", "Corée du Nord"),
    country!("MK", "389", "North Macedonia", "Macédoine du Nord"),
    country!("MP", "1", "Northern Mariana Islands", "Mariannes du Nord"),
    country!("NO", "47", "Norway", "Norvège"),
    country!("OM", "968", "Oman", "Oman"),
    country!("PK", "92", "Pakistan", "Pakistan"),
    country!("PW", "680", "Palau", "Palaos"),
    country!(
        "PS",
        "970",
        "Palestinian territories",
        "Territoires palestiniens",
    ),
    country!("PA", "507", "Panama", "Panama"),
    country!("PG", "675", "Papua New Guinea", "Papouasie-Nouvelle-Guinée"),
    country!("PY", "595", "Paraguay", "Paraguay"),
    country!("PE", "51", "Peru", "Pérou"),
    country!("PH", "63", "Philippines", "Philippines"),
    country!("PL", "48", "Poland", "Pologne"),
    country!("PT", "351", "Portugal", "Portugal"),
    country!("PR", "1", "Puerto Rico", "Porto Rico"),
    country!("QA", "974", "Qatar", "Qatar"),
    country!("RE", "262", "Réunion", "La Réunion"),
    country!("RO", "40", "Romania", "Roumanie"),
    country!("RU", "7", "Russia", "Russie"),
    country!("RW", "250", "Rwanda", "Rwanda"),
    country!("KN", "1", "Saint Kitts and Nevis", "Saint-Kitts-et-Nevis"),
    country!("LC", "1", "Saint Lucia", "Sainte-Lucie"),
    country!(
        "PM",
        "508",
        "Saint-Pierre-et-Miquelon",
        "Saint-Pierre-et-Miquelon",
    ),
    country!(
        "VC",
        "1",
        "Saint Vincent and the Grenadines",
        "Saint-Vincent-et-les Grenadines",
    ),
    country!("WS", "685", "Samoa", "Samoa"),
    country!("SM", "378", "San Marino", "Saint-Marin"),
    country!("ST", "239", "São Tomé and Príncipe", "Sao Tomé-et-Principe"),
    country!("SA", "966", "Saudi Arabia", "Arabie saoudite"),
    country!("SN", "221", "Senegal", "Sénégal"),
    country!("RS", "381", "Serbia", "Serbie"),
    country!("SC", "248", "Seychelles", "Seychelles"),
    country!("SL", "232", "Sierra Leone", "Sierra Leone"),
    country!("SG", "65", "Singapore", "Singapour"),
    country!("SX", "1", "Sint Maarten", "Saint-Martin (Pays-Bas)"),
    country!("SK", "421", "Slovakia", "Slovaquie"),
    country!("SI", "386", "Slovenia", "Slovénie"),
    country!("SB", "677", "Solomon Islands", "Îles Salomon"),
    country!("SO", "252", "Somalia", "Somalie"),
    country!("ZA", "27", "South Africa", "Afrique du Sud"),
    country!("KR", "82", "South Korea", "Corée du Sud"),
    country!("SS", "211", "South Sudan", "Soudan du Sud"),
    country!("ES", "34", "Spain", "Espagne"),
    country!("LK", "94", "Sri Lanka", "Sri Lanka"),
    country!("SD", "249", "Sudan", "Soudan"),
    country!("SR", "597", "Suriname", "Suriname"),
    country!("SE", "46", "Sweden", "Suède"),
    country!("CH", "41", "Switzerland", "Suisse"),
    country!("SY", "963", "Syria", "Syrie"),
    country!("TW", "886", "Taiwan", "Taïwan"),
    country!("TJ", "992", "Tajikistan", "Tadjikistan"),
    country!("TZ", "255", "Tanzania", "Tanzanie"),
    country!("TH", "66", "Thailand", "Thaïlande"),
    country!("TL", "670", "Timor-Leste", "Timor-Leste"),
    country!("TG", "228", "Togo", "Togo"),
    country!("TO", "676", "Tonga", "Tonga"),
    country!("TT", "1", "Trinidad and Tobago", "Trinité-et-Tobago"),
    country!("TN", "216", "Tunisia", "Tunisie"),
    country!("TR", "90", "Türkiye", "Turquie"),
    country!("TM", "993", "Turkmenistan", "Turkménistan"),
    country!(
        "TC",
        "1",
        "Turks and Caicos Islands",
        "Îles Turques-et-Caïques",
    ),
    country!("TV", "688", "Tuvalu", "Tuvalu"),
    country!("VI", "1", "US Virgin Islands", "Îles Vierges américaines"),
    country!("UG", "256", "Uganda", "Ouganda"),
    country!("UA", "380", "Ukraine", "Ukraine"),
    country!("AE", "971", "United Arab Emirates", "Émirats arabes unis"),
    country!("GB", "44", "United Kingdom", "Royaume-Uni"),
    country!("UY", "598", "Uruguay", "Uruguay"),
    country!("UZ", "998", "Uzbekistan", "Ouzbékistan"),
    country!("VU", "678", "Vanuatu", "Vanuatu"),
    country!("VE", "58", "Venezuela", "Vénézuéla"),
    country!("VN", "84", "Vietnam", "Vietnam"),
    country!("WF", "681", "Wallis and Futuna", "Wallis-et-Futuna"),
    country!("YE", "967", "Yemen", "Yémen"),
    country!("ZM", "260", "Zambia", "Zambie"),
    country!("ZW", "263", "Zimbabwe", "Zimbabwe"),
];

/// Countries whose national numbers carry a trunk `0` that is dropped after the
/// calling code: `020 7946 0958` in London is `20 7946 0958` after +44.
///
/// Only the ones this table is sure of. A country missing here keeps a typed
/// leading `0` in the number, and the international form under the field shows
/// it, so the member sees the `0` after the code before pressing Call rather
/// than a silent rewrite.
pub(super) const TRUNK_ZERO: &[&str] = &[
    "AE", "AR", "AT", "AU", "BA", "BD", "BE", "BG", "BR", "CH", "CN", "DE", "EG", "FI", "FR", "GB",
    "GH", "HR", "ID", "IE", "IL", "IN", "JP", "KE", "KR", "LK", "MA", "MY", "NG", "NL", "NZ", "PH",
    "PK", "RO", "RS", "SA", "SE", "SI", "SK", "TH", "TR", "TW", "UA", "VN", "ZA",
];

/// Trunk prefixes that are not a single `0`. Without them `8 495 123 4567`
/// typed in Russia would keep its `8` and send +7 849, a wrong number that
/// still dials.
pub(super) const OTHER_TRUNK_PREFIXES: &[(&str, &str)] =
    &[("RU", "8"), ("KZ", "8"), ("BY", "80"), ("HU", "06")];

/// The +1 toll-free area codes. They belong to the whole plan, so they never
/// name a country and never vote for a workspace's home country.
pub(super) const NANP_TOLL_FREE: &[&str] = &["800", "833", "844", "855", "866", "877", "888"];

/// Canadian area codes, so a +1 number reads Canada or the United States by its
/// first three digits.
pub(super) const CANADIAN_AREA_CODES: &[&str] = &[
    "204", "226", "236", "249", "250", "257", "263", "289", "306", "343", "354", "365", "367",
    "368", "382", "387", "403", "416", "418", "428", "431", "437", "438", "450", "460", "468",
    "474", "506", "514", "519", "548", "579", "581", "584", "587", "600", "604", "613", "639",
    "647", "672", "683", "705", "709", "742", "753", "778", "780", "782", "807", "819", "825",
    "867", "873", "879", "902", "905", "942",
];

/// The +1 area codes that belong to neither Canada nor the United States.
/// Without them a Jamaican number would say it rings the United States, which
/// is the wrong-country disclosure the dialler exists to prevent.
pub(super) const NANP_AREA_COUNTRIES: &[(&str, &str)] = &[
    ("242", "BS"),
    ("246", "BB"),
    ("264", "AI"),
    ("268", "AG"),
    ("284", "VG"),
    ("340", "VI"),
    ("345", "KY"),
    ("441", "BM"),
    ("473", "GD"),
    ("649", "TC"),
    ("658", "JM"),
    ("664", "MS"),
    ("670", "MP"),
    ("671", "GU"),
    ("684", "AS"),
    ("721", "SX"),
    ("758", "LC"),
    ("767", "DM"),
    ("784", "VC"),
    ("787", "PR"),
    ("809", "DO"),
    ("829", "DO"),
    ("849", "DO"),
    ("868", "TT"),
    ("869", "KN"),
    ("876", "JM"),
    ("939", "PR"),
];

/// The field's placeholder for a country: an example national number, written
/// without the calling code the button already shows. After the web dialler's
/// `nationalExampleFor`, and with its rule: only ranges nobody can be reached
/// on, because a placeholder is occasionally dialled. NANP 555-0100 to
/// 555-0199 is reserved for fiction, Ofcom keeps 020 7946 0000 to 0999 for
/// drama, and the rest are not working subscriber numbers.
pub(super) const NATIONAL_EXAMPLES: &[(&str, &str)] = &[
    ("CA", "(416) 555-0199"),
    ("US", "(206) 555-0142"),
    ("GB", "020 7946 0958"),
    ("EE", "6000 0000"),
    ("DE", "030 0000000"),
    ("FR", "01 00 00 00 00"),
];
