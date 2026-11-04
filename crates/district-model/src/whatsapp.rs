//! The WhatsApp composer's reads: the customer-service window and the message
//! templates.

use serde::{Deserialize, Serialize};

/// `GET /api/district/workspace/whatsapp/window`: whether free-form WhatsApp text
/// may go from a sender to a customer now, and until when. The composer's
/// preview only: the send route enforces the same window itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
#[serde(rename_all = "camelCase")]
pub struct WhatsAppWindowResponse {
    /// `true` on a successful answer.
    #[serde(default)]
    pub success: bool,
    /// The window.
    pub window: WhatsAppWindow,
    /// The sender whose window this is: the one asked about, or else the one
    /// the send route would choose.
    pub sender_id: Option<String>,
    /// The number of ours the customer's latest WhatsApp message reached, or
    /// `None` when they never wrote. It labels the thread; it never names the
    /// sender.
    pub received_on: Option<String>,
}

/// The 24-hour customer-service window. When it is closed, only a template can
/// be sent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
#[serde(rename_all = "camelCase")]
pub struct WhatsAppWindow {
    /// Whether free-form text may be sent now.
    pub open: bool,
    /// When it closes, as an ISO 8601 instant; `None` when closed.
    pub expires_at: Option<String>,
}

/// `GET /api/district/workspace/whatsapp/templates`: the workspace's WhatsApp
/// message templates.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
#[serde(rename_all = "camelCase")]
pub struct WhatsAppTemplatesResponse {
    /// `true` on a successful answer.
    #[serde(default)]
    pub success: bool,
    /// The templates.
    #[serde(default)]
    pub templates: Vec<WhatsAppTemplate>,
}

/// One WhatsApp message template. Only an `approved` one can be sent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
#[serde(rename_all = "camelCase")]
pub struct WhatsAppTemplate {
    /// The template's id, which the send route takes as `templateId`.
    pub id: String,
    /// The sender it belongs to, or `None` for one not tied to a sender.
    pub sender_id: Option<String>,
    /// The template's name at the provider.
    pub name: String,
    /// Its language, for example `en_US`.
    pub language: String,
    /// The provider's category, for example `UTILITY` or `MARKETING`.
    pub category: String,
    /// `approved`, `pending` or another provider status.
    pub status: String,
    /// The body text, with its `{{n}}` or `{{name}}` variables.
    pub body: String,
    /// How many positional `{{n}}` variables the body has, or `None` when it uses
    /// named ones or has no body text.
    pub variable_count: Option<i64>,
}

/// The only template status the send route accepts.
pub const TEMPLATE_STATUS_APPROVED: &str = "approved";

impl WhatsAppTemplate {
    /// How many `{{n}}` variables to ask for when this template is offered from
    /// the sender `sender_id`, or `None` when it must not be offered there: the
    /// rule the web composer applies, which is what the send route accepts.
    /// Only an [`approved`](TEMPLATE_STATUS_APPROVED) template, tied to no
    /// sender or to this one, with an id, a body that is not blank and a count
    /// of numbered variables. A `None` count is a body with named variables,
    /// which the send route cannot fill; the list holds pending templates and
    /// those too, for the settings screens.
    pub fn sendable_variables(&self, sender_id: &str) -> Option<usize> {
        let sender_fits = self.sender_id.as_deref().is_none_or(|own| own == sender_id);
        if self.status != TEMPLATE_STATUS_APPROVED
            || !sender_fits
            || self.id.is_empty()
            || self.body.trim().is_empty()
        {
            return None;
        }
        self.variable_count
            .and_then(|count| usize::try_from(count).ok())
    }
}

/// `body` with its numbered placeholders filled, exactly as the service fills
/// it (`lib/whatsapp/template-fill.ts`): each `{{n}}`, spaces allowed inside the
/// braces, becomes `values[n - 1]` trimmed, when that is not empty, and stays as
/// it was otherwise, so a preview shows what is still missing. Named
/// placeholders (`{{first_name}}`) are left alone.
///
/// The service stores this as the message's text, so the composer's preview,
/// the thread and what the customer reads are the same words.
pub fn fill_whatsapp_template<S: AsRef<str>>(body: &str, values: &[S]) -> String {
    let mut filled = String::with_capacity(body.len());
    let mut rest = body;
    while let Some(start) = rest.find("{{") {
        filled.push_str(&rest[..start]);
        let after = &rest[start..];
        match placeholder(after) {
            Some((number, length)) => {
                let value = number
                    .checked_sub(1)
                    .and_then(|index| values.get(index))
                    .map(|value| value.as_ref().trim_matches(is_js_space))
                    .filter(|value| !value.is_empty());
                filled.push_str(value.unwrap_or(&after[..length]));
                rest = &after[length..];
            }
            None => {
                filled.push('{');
                rest = &after[1..];
            }
        }
    }
    filled.push_str(rest);
    filled
}

/// The number in a `{{ n }}` at the start of `text` and the placeholder's
/// length in bytes, or `None` when `text` does not start with one. A number
/// too large to index anything is `usize::MAX`, which fills nothing.
fn placeholder(text: &str) -> Option<(usize, usize)> {
    let inner = text.strip_prefix("{{")?;
    let digits_at = inner.len() - inner.trim_start_matches(is_js_space).len();
    let digits = &inner[digits_at..];
    let digit_count = digits.bytes().take_while(u8::is_ascii_digit).count();
    if digit_count == 0 {
        return None;
    }
    let after_digits = &digits[digit_count..];
    let close_at = after_digits.len() - after_digits.trim_start_matches(is_js_space).len();
    after_digits[close_at..].strip_prefix("}}")?;
    let number = digits[..digit_count].parse().unwrap_or(usize::MAX);
    let length = 2 + digits_at + digit_count + close_at + 2;
    Some((number, length))
}

/// JavaScript's white space (`\s`, and what `trim` removes): Unicode's, and the
/// byte order mark, which Rust does not count.
fn is_js_space(c: char) -> bool {
    c.is_whitespace() || c == '\u{FEFF}'
}

#[cfg(test)]
mod tests {
    use super::*;

    fn template(status: &str, sender_id: Option<&str>, count: Option<i64>) -> WhatsAppTemplate {
        WhatsAppTemplate {
            id: "wa_tpl_1".to_owned(),
            sender_id: sender_id.map(str::to_owned),
            name: "reminder".to_owned(),
            language: "en_US".to_owned(),
            category: "UTILITY".to_owned(),
            status: status.to_owned(),
            body: "Hi {{1}}".to_owned(),
            variable_count: count,
        }
    }

    #[test]
    fn only_an_approved_template_of_this_sender_or_none_is_sendable() {
        assert_eq!(
            template("approved", None, Some(1)).sendable_variables("s1"),
            Some(1)
        );
        assert_eq!(
            template("approved", Some("s1"), Some(0)).sendable_variables("s1"),
            Some(0)
        );
        assert_eq!(
            template("approved", Some("s2"), Some(1)).sendable_variables("s1"),
            None
        );
        assert_eq!(
            template("pending", None, Some(1)).sendable_variables("s1"),
            None
        );
        assert_eq!(
            template("APPROVED", None, Some(1)).sendable_variables("s1"),
            None
        );
        assert_eq!(
            template("approved", None, None).sendable_variables("s1"),
            None
        );
        assert_eq!(
            template("approved", None, Some(-1)).sendable_variables("s1"),
            None
        );
        let blank = WhatsAppTemplate {
            body: " \n ".to_owned(),
            ..template("approved", None, Some(0))
        };
        assert_eq!(blank.sendable_variables("s1"), None);
        let no_id = WhatsAppTemplate {
            id: String::new(),
            ..template("approved", None, Some(0))
        };
        assert_eq!(no_id.sendable_variables("s1"), None);
    }

    #[test]
    fn a_filled_template_matches_the_services_fill() {
        let body = "Hi {{1}}, this is a reminder of your appointment on {{2}}.";
        assert_eq!(
            fill_whatsapp_template(body, &["Ada", "Thursday"]),
            "Hi Ada, this is a reminder of your appointment on Thursday."
        );
        // Trimmed, and an empty or missing value leaves its placeholder.
        assert_eq!(
            fill_whatsapp_template(body, &["  Ada \u{FEFF}", "   "]),
            "Hi Ada, this is a reminder of your appointment on {{2}}."
        );
        assert_eq!(fill_whatsapp_template::<&str>(body, &[]), body);
        // Spaces inside the braces, the same number twice, and out of range.
        assert_eq!(
            fill_whatsapp_template("{{ 1 }} and {{1}} and {{3}}", &["x", "y"]),
            "x and x and {{3}}"
        );
        // {{0}}, named and malformed placeholders stay as they are.
        assert_eq!(
            fill_whatsapp_template("{{0}} {{name}} {{1 {{1}} {1}} {{{1}}}", &["v"]),
            "{{0}} {{name}} {{1 v {1}} {v}"
        );
        assert_eq!(
            fill_whatsapp_template("{{99999999999999999999999}}", &["v"]),
            "{{99999999999999999999999}}"
        );
        // Non-ASCII text around and inside.
        assert_eq!(
            fill_whatsapp_template("Bonjour {{\u{a0}1\u{a0}}}, à jeudi", &["Éloïse"]),
            "Bonjour Éloïse, à jeudi"
        );
        assert_eq!(fill_whatsapp_template("{{", &["v"]), "{{");
        assert_eq!(fill_whatsapp_template("{{1}", &["v"]), "{{1}");
    }
}
