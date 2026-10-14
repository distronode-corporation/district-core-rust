//! What a screen shows for a value the service stores in English.
//!
//! A call's status, its direction, a member's role and the rest are stored as
//! fixed words (`no-answer`, `agency`, `eu`), and the service also stores or
//! sends a few English placeholders in place of a real value (`Unknown`). The
//! values never change; their labels do, by language. Each is generated from
//! `contracts/i18n/wire-vocabulary.json` into `catalog/wire.toml`.
//!
//! A value this build does not know is never shown as itself, turned into
//! English words: it reads as its category's own fallback where the service
//! has one, as the region code in capitals for a region, and as the generic
//! "Other" otherwise.

use crate::Locale;

#[path = "generated/wire_table.rs"]
#[rustfmt::skip]
mod table;

/// A kind of stored value, named as `contracts/i18n/wire-vocabulary.json`
/// names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum WireCategory {
    /// `status` on a call row.
    CallStatus,
    /// `direction` on a call or message row.
    CallDirection,
    /// `type` on a call row: the direction, with an unanswered inbound call
    /// read as missed.
    CallType,
    /// `sentiment` on a call row.
    Sentiment,
    /// `disposition` on a call row: the outcome the call analysis gave it.
    Disposition,
    /// `transferStatus` on a call row.
    TransferStatus,
    /// `transferReason` on a call row whose transfer failed.
    TransferReason,
    /// A stage's name in the analytics funnel.
    FunnelStage,
    /// A slice's name in the analytics sentiment chart.
    AnalyticsSentiment,
    /// A workspace member's role.
    MemberRole,
    /// A workspace's data region.
    Region,
    /// `status` on a help desk ticket.
    DeskTicketStatus,
    /// `source` on a help desk ticket.
    DeskTicketSource,
    /// `status` on a knowledge base document.
    KnowledgeDocumentStatus,
    /// `trigger` on a workflow.
    WorkflowTrigger,
    /// `status` on a workflow run, and on a workflow's latest run.
    WorkflowRunStatus,
    /// `type` of a workflow's action and of an action's result in a run.
    WorkflowAction,
    /// `outcome` of an action's result in a workflow run.
    WorkflowActionOutcome,
    /// The code that opens `reason` of an action's result in a workflow run.
    WorkflowActionReason,
    /// `type` on a phone number.
    NumberType,
    /// One of `capabilities` on a phone number.
    NumberCapability,
    /// `provider` on a phone number, and in the number search.
    NumberProvider,
    /// `status` on a meeting.
    MeetingStatus,
    /// `status` on a message: its delivery receipt, or `received`.
    MessageDeliveryStatus,
    /// `dgiStatus` on a contact: the lead research job.
    ResearchStatus,
    /// A key of `intelligence` on a contact: a section of the research.
    ContactResearchKey,
    /// The line type of a contact's phone number, from the carrier lookup.
    LineType,
    /// `status` on a subscription in the account's billing.
    BillingSubscriptionStatus,
    /// `status` on an invoice in the account's billing.
    BillingInvoiceStatus,
    /// `provider` on a messaging account.
    MessagingCarrier,
    /// `credentialSource` on a messaging account.
    MessagingCredentialSource,
}

impl WireCategory {
    /// Every category.
    pub const ALL: [Self; 31] = [
        Self::CallStatus,
        Self::CallDirection,
        Self::CallType,
        Self::Sentiment,
        Self::Disposition,
        Self::TransferStatus,
        Self::TransferReason,
        Self::FunnelStage,
        Self::AnalyticsSentiment,
        Self::MemberRole,
        Self::Region,
        Self::DeskTicketStatus,
        Self::DeskTicketSource,
        Self::KnowledgeDocumentStatus,
        Self::WorkflowTrigger,
        Self::WorkflowRunStatus,
        Self::WorkflowAction,
        Self::WorkflowActionOutcome,
        Self::WorkflowActionReason,
        Self::NumberType,
        Self::NumberCapability,
        Self::NumberProvider,
        Self::MeetingStatus,
        Self::MessageDeliveryStatus,
        Self::ResearchStatus,
        Self::ContactResearchKey,
        Self::LineType,
        Self::BillingSubscriptionStatus,
        Self::BillingInvoiceStatus,
        Self::MessagingCarrier,
        Self::MessagingCredentialSource,
    ];

    /// The name the vocabulary gives the category (`callStatus`).
    pub fn name(self) -> &'static str {
        match self {
            Self::CallStatus => "callStatus",
            Self::CallDirection => "callDirection",
            Self::CallType => "callType",
            Self::Sentiment => "sentiment",
            Self::Disposition => "disposition",
            Self::TransferStatus => "transferStatus",
            Self::TransferReason => "transferReason",
            Self::FunnelStage => "funnelStage",
            Self::AnalyticsSentiment => "analyticsSentiment",
            Self::MemberRole => "memberRole",
            Self::Region => "region",
            Self::DeskTicketStatus => "deskTicketStatus",
            Self::DeskTicketSource => "deskTicketSource",
            Self::KnowledgeDocumentStatus => "knowledgeDocumentStatus",
            Self::WorkflowTrigger => "workflowTrigger",
            Self::WorkflowRunStatus => "workflowRunStatus",
            Self::WorkflowAction => "workflowAction",
            Self::WorkflowActionOutcome => "workflowActionOutcome",
            Self::WorkflowActionReason => "workflowActionReason",
            Self::NumberType => "numberType",
            Self::NumberCapability => "numberCapability",
            Self::NumberProvider => "numberProvider",
            Self::MeetingStatus => "meetingStatus",
            Self::MessageDeliveryStatus => "messageDeliveryStatus",
            Self::ResearchStatus => "researchStatus",
            Self::ContactResearchKey => "contactResearchKey",
            Self::LineType => "lineType",
            Self::BillingSubscriptionStatus => "billingSubscriptionStatus",
            Self::BillingInvoiceStatus => "billingInvoiceStatus",
            Self::MessagingCarrier => "messagingCarrier",
            Self::MessagingCredentialSource => "messagingCredentialSource",
        }
    }

    /// The category the vocabulary names `name`, or `None`.
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|category| category.name() == name)
    }
}

/// What a screen shows for `value` in `category`. The value is matched
/// trimmed and without regard to case. A value the vocabulary does not have
/// reads as the category's fallback, the region code in capitals, or "Other";
/// never as the stored text.
pub fn wire_label(category: WireCategory, value: &str, locale: Locale) -> String {
    let value = value.trim();
    if let Some(label) = table::label(locale, category, &value.to_ascii_lowercase()) {
        return label.to_owned();
    }
    match (category, table::fallback(locale, category)) {
        (_, Some(fallback)) => fallback.to_owned(),
        (WireCategory::Region, None) => value.to_uppercase(),
        (_, None) => crate::wire_strings::wire_generic_other(locale).to_owned(),
    }
}

/// A region's long name, naming its datacenter (`Canada (Montreal)`), or the
/// region code in capitals for a region the vocabulary does not have.
pub fn region_long_label(value: &str, locale: Locale) -> String {
    let value = value.trim();
    table::region_long(locale, &value.to_ascii_lowercase())
        .map_or_else(|| value.to_uppercase(), str::to_owned)
}

/// The label for `value` when it is one of the placeholders the service
/// stores or sends in English in place of a real value (`Unknown` for a
/// caller with no name), exactly as it sends it; `None` for anything else,
/// which is real data and is shown as it is.
pub fn placeholder_label(value: &str, locale: Locale) -> Option<&'static str> {
    table::placeholder(locale, value)
}
