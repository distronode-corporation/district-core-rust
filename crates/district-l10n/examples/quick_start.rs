// SPDX-FileCopyrightText: Distronode Corporation
//
// SPDX-License-Identifier: Apache-2.0

//! The smallest use of the core: pick the language from the user's preferred
//! languages, as the operating system lists them, and format a price and a call's
//! length the way the District AI service does. README.md's quick start runs it:
//!
//! ```text
//! cargo run -p district-l10n --example quick_start --locked
//! ```

use district_l10n::{Locale, format};

fn main() {
    for tags in [["en-US"], ["fr_CA.UTF-8"]] {
        let locale = Locale::from_tags(&tags);
        println!(
            "{}: {}, {}",
            locale.tag(),
            format::price(locale, 4900, Some("CAD")),
            format::minutes_seconds(locale, 125),
        );
    }
}
