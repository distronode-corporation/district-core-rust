# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this
project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

`scripts/check-version.py` holds the newest released section's heading to the version in
`Cargo.toml`, and README.md and SECURITY.md to that release, so a release is one edit to
each: rename `[Unreleased]` to the version and date, bump `[workspace.package] version`
to match, and name the new version wherever README.md and SECURITY.md name the current
release. See "Releases" in CONTRIBUTING.md.

What counts as a breaking change here is a change to anything an app that pins these
crates calls or implements: a public type, function or trait, an `Event` or `Effect`
variant, or a field of one. Those take a new major version.

## [Unreleased]

Opt-in crash reports, in a new crate. Nothing an app calls or implements changes, and an
app that does not depend on the new crate builds exactly what it did.

### Added

- `district-crash`: crash reports through Sentry (0.49.3, rustls, no OpenSSL), off until the
  user turns them on. `district_crash::apply(&Settings, Consent)` sets reporting up only
  when the app passes both a DSN and `Consent::Granted`, and returns `Status::On` or
  `Status::Off(OffReason)`; called again with `Consent::Denied` it closes the reporter,
  after sending what was queued, and with consent again it opens a new one. While on, a
  panic on any thread is reported before the previous panic handler runs, and
  `capture_error` reports an error the app chooses to. A report carries the debug images
  of the main executable and of the image holding the reporter's code (the app's own
  library on Windows): file, build id (ELF) or PDB id and age (PE), load address, size
  and architecture, so the stack of a stripped build can be symbolicated against the
  release's uploaded debug files. No other loaded library is listed. Every home
  directory path in a report, the images' files included, has the user's name replaced,
  and the machine's name, the user and any address are removed. Nothing is read from the environment (a `SENTRY_DSN` there is ignored), and
  plain `http` is refused unless the DSN names this machine (`CrashError`). The asking
  (once at first start, "Don't send" first) and the settings switch are the apps'.

## [6.0.0] - 2026-10-24

A real decline. Until now declining a ring here told the service nothing, so a
deliberate refusal and a desktop nobody was at were the same thing to it and the caller
waited out the whole ring. A member's press of Decline is now sent, and once every member
rung has declined the receptionist takes a message at once. Only a person's decision is
sent: a ring that runs out, one the service withdraws, and one dropped for a sleep, a quit
or a sign-out still send nothing. This reverses the rule that a decline sent nothing
(decided 2026-10-24). A major version because `Effect`, `DistrictApi`, `Endpoint` and
`RingCancelReason` each gain a member an app matches on or implements. The contracts move
to the server's f5ffc0a3c (`contracts/i18n/` to its a9cf0ef7a), with the French of every
new error sentence and wire label.

### Added

- `Effect::DeclineCall { workspace_id, call_id }`, emitted by `RingEvent::Decline` on a
  ringing or waiting ring, after the ringtone stops and the notification goes. Breaking for
  an app that matches every `Effect`: run it through the core's runner (it reports nothing
  back, and a failure is not tried again) or send `ApiClient::decline_call` yourself.
- `DistrictApi::decline_call`, which the runner calls for that effect. Breaking for an app
  or a test that implements `DistrictApi` by hand; `ApiClient` implements it.
- `ApiClient::decline_call` and `Endpoint::CallDecline`
  (`POST /api/district/calls/{callId}/decline`, `{workspaceId}`, never repeated), with
  `district_model::CallDeclineResponse`. The endpoint is in `DESKTOP_ONLY` until the
  endpoint snapshot is refreshed from an Android release that declines too.
- `RingCancelReason::Declined` (`declined` on the wire): every member rung declined, this
  one among them, here or on another device. A ring cancelled for it is silenced and put
  away as a decline here is, with no missed call. Breaking for an exhaustive match on
  `RingCancelReason`. A reason this client does not know still reads as `Withdrawn`.

### Changed

- `contracts/fixtures/` from the server's f5ffc0a3c and `contracts/i18n/` from its
  a9cf0ef7a, which drops a `priority` workflow action reason the server's contract scanner
  had read out of a comment. New: the call
  decline answer (decoded), and the message sender name and notify-read token fixtures,
  excluded by decision (a phone's notification service reads them; a desktop has its
  live socket and its session). The recorded persona options list four engines (was
  seven). `contracts/desktop/` is unchanged.
- The error table and wire labels have the service's new codes and values, with the
  service's French: workflow conditions and the team notification action, number
  transfers, a member's mobile number, knowledge imports from a web address, and the voice
  engine that is not offered yet. Call dispositions now have labels.

### Known limits

- The service's ring cancel is per call, not per person. A member who declines on another
  device while a colleague is still rung is not told here, so this desktop rings on until
  the ring ends for everyone.

## [5.1.0] - 2026-10-24

The core of the next releases of District AI for Linux and District AI for Windows. A
minor version: new functions for the scheduler's time fields and a held number's row note,
with French time fields, one explanation of where numbers are bought, the Desk's name in
English and a notification click at a cold start that is no longer lost. Nothing an app
calls or implements is removed or changed in a way that breaks it. Pull requests #41, #42,
#43, #45 and #48.

### Security

- webpki-root-certs 1.0.10 in Cargo.lock, for RUSTSEC-2026-0383 (CVE-2026-111943): 1.0.9
  still bundled emSign ECC Root CA - G3, which can let a certificate path through the
  distrusted emSign intermediates. It reaches the lock through rustls-platform-verifier
  0.7, which builds it only for wasm32, so neither app's build compiled it; lock-based
  scanners (OpenSSF Scorecard) reported it all the same (#37).

### Added

- `scheduling_admin::time_field_text`, `scheduling_admin::time_field_wire` and
  `scheduling_admin::TimeFieldError`: a time field's display and parse pair. French shows a
  scheduler time as « 9 h », « 9 h 30 », « 17 h » to edit (ordinary spaces, as `format::time`),
  English keeps `09:00`; either language reads back `09:30`, `9:30`, `9 h 30`, `9h30` and
  `9 h`, with an ordinary, no-break (U+00A0) or narrow no-break (U+202F) space or none around
  the `h`, and saves `HH:MM`.
- `number_row_note(number, can_change)`: the sentence to show on a held number's row. It is
  `number_actions_note`'s sentence except a viewer's, which the screen's note
  (`numbers_scope_note`) already says once; until now a viewer read "Carrier setup and number
  changes need an agency or client role..." on the screen and again on every number. Keep
  gating the row's actions on `number_actions_note(..).is_none()`.

### Changed

- The working hours editor and the custom-hours override form hold their times as the field
  shows them (`HoursRange::start`/`end`, `OverrideForm::start`/`end`, the
  `SetRangeStart`/`SetRangeEnd` values): unchanged in English, « 9 h » in French. The save
  still sends `HH:MM`. A French member no longer edits « 09:00 ».
- `25:00`, `9:75` and any other time past 23:59 is refused before the save, with
  `time.format_error` as an unreadable time is; until now it reached the scheduler.
- The French of `time.format_error` gives the French form as its example: « Entrez une heure,
  par exemple 9 h ou 13 h 30. » (was « Entrez une heure au format HH:MM. »).
- `marketplace.web_caption` (`MarketplaceScreen::WEB_CAPTION`, the caption under the link to
  the web marketplace) is now "Opens in your browser." / « S'ouvre dans votre navigateur. »
  (was "Numbers are bought on the District AI website. This opens it in your browser."). The
  caption no longer explains where numbers are bought: the screen's note
  (`MarketplaceScreen::scope_note`, `numbers.purchase_elsewhere` for a member who can buy) is
  that one explanation, so an app must show the scope note to every member, buyers included.
- Desk is called the Desk: the English of `desk.failed_title`, `desk.off_title`,
  `desk.off_body`, `desk.settings_failed_title` and `desk.turn_on` names it "the Desk" instead
  of "the help desk", as the apps do ("Could not load the Desk", "The Desk is off", "Turn on
  the Desk"). The French already said « comptoir ». No key or function changed.

### Fixed

- A notification opened while the stored session is still being restored is no longer
  dropped. At a cold start from a notification that outlived the app, the app sends
  `Event::OpenNotification` before the session is back, and until now the click did nothing:
  the window opened on the overview. The core now keeps the latest such click (one only, the
  later replacing the earlier) and opens it, workspace and all, once the session restores and
  the workspace list is read. It is dropped if the restore ends with nobody signed in, so it is
  never opened for an account that signs in afterwards, and if the workspace list cannot be
  read or has no workspace. Signed in, a click behaves as before.
- A notification kept from the restore is dropped if the user navigates (`Event::Navigate` or
  `Event::Back`) before the workspace list is read, rather than opened afterwards over the
  screen they chose.

## [5.0.1] - 2026-10-22

The core of District AI for Linux 3.0.3 and District AI for Windows 3.0.3: a security
update, with no change to anything an app calls.

### Security

- rustls-webpki 0.103.16 in Cargo.lock, for RUSTSEC-2026-0382 (CVE-2026-111943):
  0.103.15 could accept a certificate chain through one of the distrusted emSign
  intermediates, whose private keys are now public.

## [5.0.0] - 2026-10-16

The core of District AI for Linux 3.0.2 and District AI for Windows 3.0.2. A major
version because the hand-off effects' nonce is no longer optional (below); the rest
is French, lists that follow their item, and booking forms whose complaints clear.

### Changed

- Breaking: the `nonce` of `Effect::RequestSchedulingHandOff` and of
  `Effect::RequestBillingHandOff` is a `HandOffNonce`, no longer an
  `Option<HandOffNonce>`. Since 4.0.2 the core never asks for a hand-off link unbound,
  so `None` could not happen; the type now says so, and an app that matched
  `nonce: Some(_)` or read `nonce.as_ref()` reads the nonce itself. The runner still
  sends it as the body's `nonce`; `ApiClient::scheduling_hand_off` and
  `ApiClient::billing_hand_off` are unchanged.
- The French warning beside a new booking API key uses the key's pronoun: « Copiez-la
  maintenant. Elle ne sera plus affichée. » (was « Copiez-le… Il… », the secret's). A
  webhook's new signing secret has its own sentence, `webhook.secret_reveal_warning`
  ("Copy the secret now. It is not shown again.", « Copiez le secret maintenant. Il ne
  sera plus affiché. »), in `MintedSecret::labels()`. Visual QA LD08.
- A help desk ticket's row in the queue follows a reply or a status change made on the
  ticket (a reply moves it to `waiting`), and a support request's row follows its
  close, so the list and the open item no longer disagree until the list is read
  again. Visual QA LD09.
- A contact that could not be added or saved because another contact already holds its
  phone number or email address (the service's 409) reads as that in French too,
  « Un autre contact a déjà ce numéro de téléphone ou cette adresse courriel. »
  (`failure.contact_exists`; English keeps the service's own sentence), and offers no
  retry, which would get the same answer. It used to read as the generic « Une erreur
  est survenue. Veuillez réessayer. ». `FailureText::from_contact_error` is new. Visual
  QA LB7.
- In the booking pages, a form's complaint ("A host's priority has to be a whole number,
  0 or more.", "Pick a date.", "A Select question needs at least one option.") goes as
  soon as the hosts, the override or the question is changed, and the next save checks
  again; it used to stay beside the corrected value until the next save. Visual QA LC20.
- SECURITY.md describes the hand-off as it is since 4.0.2: given up, never asked for
  unbound, when the browser does not answer within `HAND_OFF_CALLBACK_WAIT`.
- `contracts/i18n/` from the server's ecf098046: the error code
  `sending_domain_provider_changed` ("This sending domain was set up with a different
  email provider. Remove it and connect it again to get the new DNS records."), the
  workflow skip reasons `email-misconfigured` and `ses-missing` (« Courriel mal
  configuré », « SES non configuré »; the English "SES missing" keeps the initialism
  in capitals, as District AI for Android writes it), the number status `in-use` ("Active",
  « Actif », where it read as the generic label), and the portal's headings for three
  research keys: `estimatedValue` "DGI Evaluation" (« Évaluation DGI »), `person`
  "Contact Profile" (« Profil du contact ») and `callerIntel` "Caller intelligence"
  (« Renseignements sur l'appelant »), which read as the key itself ("EstimatedValue")
  before. The error's French says « les nouvelles entrées DNS », never
  « enregistrements », which the glossary keeps for recordings; the service's French
  moves to the same words, and `crates/district-l10n/tests/errors.rs` says when the
  contract has caught up.
- A call's length in French no longer breaks between the minutes and the seconds:
  « 3 min 4 s » with a no-break space (U+00A0) between each number and its unit and
  between the two parts (`format::minutes_seconds`, so a call row's length and the
  analytics average), as the service's format vectors now say. It was
  « 3 min 4 s », with an ordinary space in the middle.

## [4.0.2] - 2026-10-15

The core of District AI for Linux 3.0.1 and District AI for Windows 3.0.1: a desktop stops
ringing when the call is answered on another device, when the ring's window closes, or
when the caller hangs up, as soon as the service says so.

### Added

- `TelemetryEventType::CallRingCancelled` (`call_ring_cancelled`), whose data is
  `{callId, reason}`, and `RingCancelReason` (`answered`, `timeout`, `call_ended`,
  `withdrawn`), read with `TelemetryEnvelope::ring_cancel_reason`. A reason this client
  does not know reads as `Withdrawn`. Held to the new desktop fixture
  `telemetry-event-call-ring-cancelled.json` (the desktop set now from 6cd0ff667).
- The core acts on the event from the open workspace's socket and from every other
  workspace's where the member takes calls: a ring still ringing or waiting here for
  that call stops its ringtone and loses its notification, and its deadline is
  dropped. Answered elsewhere, the ring ends as `RingEnd::AnsweredElsewhere`, which
  says "That call was answered on another device." (`IncomingRing::ANSWERED_ELSEWHERE`,
  heading `IncomingRing::ANSWERED_ELSEWHERE_TITLE`, "Answered on another device"); a
  caller who hung up ends it as `RingEnd::CallEnded`; a timeout, a withdrawal or an
  unknown reason makes it a missed call with the missed-call notification, as the
  local deadline does.
- Three strings, in English and Canadian French: `ringing.answered_elsewhere`,
  `live.answered_elsewhere_title` and `failure.hand_off_unanswered`.
- A read that failed can be read again where only closing and reopening could before:
  `HoursEvent::RetryRead` (the week's editor its rules, the overrides' editor its
  overrides), `EventTypeEvent::RetryDialogRead` (the hosts dialog's hosts, then the
  users who could be added, whose read is now `HostsDialog::candidates_load`) and
  `DeveloperEvent::RetryDeliveries` (the open webhook's deliveries). Each does nothing
  unless that read failed, as `BookingsEvent::RetryDialogRead` does.
- `district_l10n::support_status_label(status_name, category, locale)` (also
  `district_core::l10n::support_status_label`), and `listed_support_status_label`,
  the label of a listed name only: a support request's status in the
  app's language, never the help desk's own word. A name this build lists (`Received`,
  `Open`, `Waiting for support`, `Waiting for customer`, `In progress`, `Pending`,
  `Escalated`, `Resolved`, `Done`, `Closed`, `Canceled`, `Declined`, `unknown`, and
  their spellings) reads as its label; an unlisted one as its category's (`NEW` Open,
  `INDETERMINATE` In progress, `DONE` Closed, `PENDING` Received), and an unknown
  category as "Status unknown". The thirteen `request_status.*` labels in
  `catalog/support.toml` are District AI for iOS's, word for word in both languages.
- `SentimentShare::name`: the service's own name for a sentiment band (`Positive
  Sentiment`, `Neutral Sentiment`, `Friction Sentiment`), the same in every language,
  for an app to key the band's colour on. Keyed on the translated `label`, French drew
  all three bands in the friction colour.

### Changed

- The booking pages' hand-off to the web is never asked for unbound. When the browser
  has not answered the start page after `HAND_OFF_CALLBACK_WAIT`, the hand-off is given
  up with a retryable notice (`failure.hand_off_unanswered`, "Your browser didn't
  respond to District AI in time, so the page wasn't opened. Try again, and let your
  browser open District AI if it asks."), where it used to ask for a link bound to no
  browser.
  `Effect::RequestSchedulingHandOff` now always carries a nonce, so the service can
  require one (`HANDOFF_REQUIRE_NONCE`, security sweep S33).
- `HAND_OFF_CALLBACK_WAIT` is 30 seconds (was 10), for both hand-offs, as District Core
  for Swift waits: it covers the time a person takes on the browser's "Open District
  AI?" prompt, and running out now costs a retry rather than an unbound link. The
  service's nonce cookie lives five minutes.
- The billing hand-off (checkout and the billing page, in the app or the browser) does
  the same: no answer in time gives the purchase up with the same notice, and
  `Effect::RequestBillingHandOff` always carries a nonce. With both, no link the core
  asks for is unbound.
- `dialer.hint` writes its example number with no-break spaces between the groups
  (`+1 212 555 0142`), in English and French, so it never breaks after `+1`.
- `SupportRequestScreen::closed_message` names the status by its listed label, and an
  unlisted one as "Done", the service's default name for a close: "Closed as Done."
  stays, and the French reads "Fermée (état : Terminée)." where it carried the
  desk's English.

### Unchanged on purpose

- The event never touches a call this desktop answered or is answering: it reaches the
  answering device too, sometimes after the answer, so an answer on its way, the call
  joined and its live transcript are left alone. It is not a `call_ended`: it never
  runs the hang-up of a call answered here.
- The local thirty-second deadline stays as the fallback for a cancel that never
  arrives. A repeated cancel, on the socket or a push, does nothing more.

`RingEnd::AnsweredElsewhere`, `TelemetryEventType::CallRingCancelled` and the three
retry events are new variants of public enums, and `HostsDialog::candidates_load` and
`SentimentShare::name` new fields of public structs, which this file's rule counts as breaking. They ship in a
patch release because the only apps that pin 4.0, District AI for Linux and District
AI for Windows, move to this release in their 3.0.1 together with the matching arms.

## [4.0.1] - 2026-10-15

The language signals District AI for Linux 3.0.0 and District AI for Windows 3.0.0 send
the service: every request names the app's language, and the account is told it.

### Added

- Every request carries `Accept-Language`: `fr-CA` when the app shows French, `en`
  otherwise (`district_l10n::current`), so what the service writes in an answer is in
  that language.
- `Endpoint::SaveSettings` (`POST /api/settings`, scoped to the account) and
  `ApiClient::save_language`, which writes the account's language alone
  (`{"profile": {"locale": "fr"}}`). `district-model`: `SettingsResponse`,
  `SettingsSaveResponse` and the body `LanguageSave`, held to the three
  `district-settings*.json` fixtures, which the fixture manifest now decodes rather than
  excludes.
- `Effect::SaveLanguage` and `Event::LanguageSaved`, run by the core's own
  `EffectRunner` through the new `DistrictApi::save_language`. The model asks for
  it after every completed sign-in and after the session restored at start-up, so
  once per launch: the core keeps no state across runs that could say the account was
  told already. A failed write is not retried until the next sign-in or launch.

These are additions to `Effect`, `Event` and `DistrictApi`, which this file's rule
counts as breaking. They ship in a patch release because the only apps that pin 4.0,
District AI for Linux 3.0.0 and District AI for Windows 3.0.0, hand every effect to the
core's `EffectRunner` and implement none of these traits themselves.

## [4.0.0] - 2026-10-14

The core of District AI for Linux 3.0.0 and District AI for Windows 3.0.0: every word in
English and Canadian French, scheduling administration, number management, starting a
conversation and reporting AI-generated content. It is the six 4.0.0 betas below, and
these changes since 4.0.0-beta.6.

What 4.0.0 brings over 3.1.1:

- **Languages.** `district-l10n`, a new crate: every sentence and label the core writes
  comes from its catalogs, in English and reviewed Canadian French (`Locale`,
  `CoreConfig::locale`). The service's error codes, stored values and formats (money,
  dates, times, durations, numbers) read as the service writes them, held to
  `contracts/i18n/`. English is unchanged except where a beta below says so.
- **Scheduling administration**, native in the core (`Route::SchedulingAdmin`): the
  overview, bookings, event types with their hosts and questions, working hours,
  calendars, the team, developer access with webhooks, and settings.
- **Numbers and carriers**: managing the numbers a workspace holds, the carrier
  accounts, SIP trunks, the passcode sender and the A2P and toll-free registrations.
- **Conversations and reports**: starting a conversation from the inbox, and reporting
  AI-generated content from any screen.
- **Presence**: going away unregisters the desktop presence alone, so a Windows push
  registration stays.
- Breaking, for the apps that pin the core: new `Route`, `Event` and `Effect` variants,
  new fields on `CoreConfig`, `SignedIn` and several screens, and new `DistrictApi`
  methods; each beta lists its own.

### Changed

- `sip_trunk_meta` shows a SIP trunk's status by its label (`Active`, « Actif »), and a
  status this build does not know as the generic `Other` (« Autre »), never as stored.
- README.md and SECURITY.md name 4.0.0 as the current release, used by District AI for
  Linux 3.0.0 and District AI for Windows 3.0.0; the crate table names
  `district-l10n`; the README has the project page link and the account paragraph.
- `scripts/check-version.py` holds README.md and SECURITY.md to the newest release
  that is not a pre-release, so a beta cut for testing never claims to be the
  current release, and has a `--self-test` that CI runs first.
- CHANGELOG.md has the 1.2.1 backport's section.
- CI checks that every catalog has its reviewed French (`l10n.py check --require fr-CA`),
  one catalog per call, since a key may be defined in two of them.
- An OpenSSF Scorecard workflow, as district-linux has. Code scanning (CodeQL) is
  GitHub's default setup on this repository, as on district-linux.

## [4.0.0-beta.6] - 2026-10-14

Findings from the Linux and Windows French passes: lengths, chart days, balances and
prices written the French way, the scheduler's webhooks as the web portal offers them,
and every contract set synced to the service's pre-release French.

### Added

- `call_duration_label`: a call row's length. English is the service's own text
  (`1m 5s`); French is written from its seconds as the portal reads them (`1 min 5 s`,
  `60 min`, `0 min`), held to `formatMinutesSeconds` in `contracts/i18n/format-vectors.json`.
- `subscription_price` and `invoice_price`: a subscription's or an invoice's amount in
  the currency the service now names. French writes it as `formatMoney` does
  (`249,00 $ US`, `249,00 $ CA`); English stays `$249.00` (a plan decision).
- `scheduling_admin::WEBHOOK_EVENTS`: the four booking events the webhook form offers,
  as the web portal does. `WebhookEditor::kept_events` names the other events the
  scheduler knows that a webhook already sends; a save keeps them.
- `district_l10n::format::day_month`: `Aug 9`, `9 août`, `1er sept.`.

### Changed

- `contracts/` is synced, every set, to monorepo 43eed5fd6: 21 new mobile fixtures
  (18 decoded by the types this crate already had, three for the account's saved
  language excluded: no desktop screen reads or saves it), `currency` on billing
  subscriptions and invoices, and the deliveries fixture's `success` status. The
  catalogs take the contract's French verbatim: 11 API error sentences
  (« transporteur », « soutien », « prospects », « Web », and « offerte » where the
  catalog had kept « disponible » for two codes), « Appels composés au total »,
  and a help desk ticket's status in the masculine (« Ouvert », « Réglé », « Inconnu »),
  in the wire labels and the help desk's own.
- `format_money` ignores the currency in English, which stays `$249.00`.
- A count the core places in a French sentence was already grouped by
  `district_l10n::format::integer` (`1 500 numéros dans ce compte`); a test now pins it.
- `AnalyticsReport::trend` labels each day in French from its `isoDate` (`9 août`);
  English keeps the service's label (`Aug 9`).
- `carrier_balance` writes a balance with its currency as money (`Balance $12.50`,
  `Solde : 12,50 $ US`), and one without as a number with two decimals.
- A scheduling webhook delivery whose status the vocabulary does not list reads as the
  generic `Other` (`Autre`), never as the status itself.
- `ProviderStatusResponse::provider` is `Option<Option<String>>`, so the `null` the
  service sends with no carrier at all goes back as it came.
- `scripts/sync-contracts.py` can scope a substitution to the files it applies to, for
  a stand-in the server already writes: the number lookup fixture's `+14165550142`
  becomes `+14165550143` there only.

### Fixed

- Saving a webhook, or deleting one, while a webhook's deliveries were open left them
  loading forever: the webhooks were read again and every webhook's deliveries
  dropped, but the open one's were never read again. They are now read beside the
  webhooks, and a webhook that is gone closes its deliveries.

## [4.0.0-beta.5] - 2026-10-14

Two places where the desktop apps showed a stored word or a raw timestamp now read as
labels and dates, in English and in French.

### Added

- `contracts/i18n/` from the service's current localization facts (monorepo 2528233a9):
  the wire vocabulary has 39 categories, up from 31. `WireCategory` names the eight new
  ones (`NumberStatus`, `A2pCampaignType`, `A2pRegistrationStatus`,
  `TollFreeVerificationStatus`, `SipTrunkStatus`, `SchedulingWebhookDeliveryStatus`,
  `SchedulingHostRole`, `SchedulingQuestionType`), and `catalog/wire.toml` has their 26
  labels with reviewed French: the service's own wherever the contract has it, byte for
  byte, and the programme's reviewed French for the ten values the service shows as sent.
- `scripts/sync-contracts.py --sets` brings only the named sets to the server's commit;
  each other set is re-read at the commit `SOURCE.toml` records for it and must come out
  byte for byte as it is here. `SOURCE.toml` now records a commit per set. The fixture
  sets stay at 46e1740, whose newer fixtures this client has no types for yet.

### Changed

- `EventTypeSection::host_rows` and `question_rows` show a host's role and a question's
  type by their labels (`Required`, `Rotation · archived`, `Select · required`; in
  French `Obligatoire`, `Liste déroulante · obligatoire`), and a role or type this build
  does not know as the generic `Other` (`Autre`), never as the stored word. They used to
  show the stored word (`required`, `select · required`) in both languages.
- `DeviceRow::last_active` writes the day through `district_l10n::format` (`Last active
  Aug 15, 2026`, `Dernière activité : 15 août 2026`) instead of the service's raw
  ISO 8601 stamp. The day is the one the stamp falls on in UTC, since the core knows no
  time zone of the computer's; the apps already say the time in local time when they can
  read the stamp. A stamp that is not an instant is still shown as it came.

## [4.0.0-beta.4] - 2026-10-14

The catalogs in Canadian French, for testing French in the desktop apps. English is
unchanged, and no app shows French until it sets `CoreConfig::locale`.

### Changed

- The catalogs carry their reviewed Canadian French (fr-CA, #24): 1,481 entries across
  `crates/district-l10n/catalog/*.toml`, so `district_l10n::FR_COMPLETE` is true. The
  language stays the app's choice (`CoreConfig::locale`), and the apps keep French off.
- The scheduling formatters' French is held by tests to the service's own output: the
  first of a month reads `1er` (`1er août 2026`), the clock is 24-hour with no
  minutes on the hour (`12 sept. 2026, 15 h 04`, `9 h`), and a date range keeps both
  years. They already wrote it this way through `district_l10n::format`; no output
  changed in either language.

### Security

- rustls 0.23.46 in Cargo.lock (#24), for RUSTSEC-2026-0346, -0347, -0348 and -0349,
  which 0.23.45 carries.

## [4.0.0-beta.3] - 2026-10-14

Scheduling administration in the core for both desktop apps, and a presence that no
longer removes a Windows push registration when it goes away.

### Added

- Scheduling administration, native in the core, ported from District AI for iOS and
  district-core-swift 7.0.0-beta.1. Each section is a route,
  `Route::SchedulingAdmin(SchedulingSection)`, with its state in
  `SignedIn::scheduling_admin` (`SchedulingAdmin`) and its events in
  `Event::SchedulingAdmin(SchedulingAdminEvent)`. The per-area API:
  - the overview (`OverviewSection`);
  - bookings and one booking (`BookingsSection`, `BookingDetailSection`: cancel, move,
    hand over);
  - event types and one event type (`EventTypesSection`, `EventTypeSection`: create,
    edit, switch on or off, archive, test email, delete, hosts and routing, questions);
  - working hours and date overrides (`HoursSection`);
  - calendars (`CalendarSection`: Google and Microsoft through a single-use browser
    sign-in, CalDAV through a form, disconnect);
  - the team (`TeamSection`: members, teams and their hosts, archiving an account);
  - developer (`DeveloperSection`: API keys, connected apps and the MCP address,
    webhooks and their deliveries);
  - settings (`SettingsSection`: the booking page and its images, the assistant, the
    member's profile and notifications).

  Every write that removes something is asked once more, every refusal reads as one
  of the web dashboard's sentences, and every word comes from the new catalog
  `catalog/scheduling.toml` (`scheduling_strings`).
- `district-api`: `ApiClient::scheduling_admin` (`POST /api/district/scheduling/admin`
  and its upload route) and `ApiClient::scheduling_sign_in`
  (`GET /api/district/scheduling/sso`, read for its `Location`, never followed).
- `district-model`: the admin RPC's wire types (`SchedulingAdminRequest`,
  `SchedulingAdminReply`, `SchedulingAdminData` and every payload), held strictly to
  the 39 `district-scheduling-*` fixtures.

### Changed

- The presence going away (the setting off, sleep, quitting) unregisters the presence
  alone: `POST devices/unregister` with `{"kind": "desktop"}`, so a Windows push
  registration of the same installation stays. Signing out still removes every kind,
  with no body, through the new `Presence::unregister_all`. A service that predates
  the kind ignores the body and behaves as before.
- Breaking:
  - `Route`, `Event` and `Effect` have new variants (`Route::SchedulingAdmin`,
    `Event::SchedulingAdmin`, `Event::SchedulingAdminAnswered`,
    `Event::SchedulingSignInReady`, `Effect::SchedulingAdmin`,
    `Effect::SchedulingSignIn`), and `SignedIn` a new field, `scheduling_admin`.
  - `DistrictApi` has two new methods, `scheduling_admin` and `scheduling_sign_in`.
  - `ApiClient::unregister_presence` and `PresenceApi::unregister_presence` take the
    kind (`None` for every kind); `Presence` has a new method, `unregister_all`.
  - `BodyKind` has a new variant, `OptionalJson`, which `PushUnregister` uses.

## [4.0.0-beta.2] - 2026-10-13

Three features the iOS and Mac apps have, now in the core for both desktop apps, and the
server's wire vocabulary synced.

### Added

- Reporting AI-generated content, lifted from District AI for Windows
  (`district-ffi/src/report.rs`) so that both apps raise the same report: a support
  request through the support form, from any screen, without navigating.
  - Raise it with `SupportEvent::Report { platform, target, note }` (`ReportTarget`:
    a call, a contact, a conversation event, an AI-written reply, a District HQ
    answer, a meeting's minutes or action items); put it away with
    `SupportEvent::DismissReport`.
  - Read it back with `SupportScreen::report_status` (`ReportStatus`: sending, sent,
    failed) and `SupportScreen::report_refusal`, the sentence to show while the
    member has a support request of their own typed. The dialog's words are
    `ReportDialog`.
  - What the support desk receives is English in every language and names the app
    (`report_message`, `report_subject`, `REPORT_NOTE_LIMIT`); what the member reads
    is translatable.
- Starting a conversation from the inbox: `InboxEvent::Compose(ComposeEvent)` and
  `InboxScreen::compose`, a `ComposeScreen`.
  - A recipient is a contact from the most recent window
    (`COMPOSE_CONTACT_WINDOW`, filtered by name, number or address) or a number or
    address typed in (`ComposeRecipient`); a text message comes first among its
    channels, and an email needs a subject.
  - The first message is sent once; its thread is then found by the message's id
    and opened, its reply going where the service says the thread is reached. No new
    endpoint.
- Managing the numbers a workspace holds; buying a number stays on the web.
  `MarketplaceScreen::admin` (a `NumberAdmin`) and
  `MarketplaceEvent::Numbers(NumberAdminEvent)`, with a new carrier tab
  (`MarketplaceTab::Carrier`).
  - Reapplying a number's routing; releasing it behind a confirmation
    (`NumberConfirmation`), after which the numbers are read again; the billed lookup
    behind a confirmation.
  - On the carrier tab: the carrier accounts' status (`CarrierStatus`), the SIP
    trunks (`SipTrunks`, list and create), the one-time passcode sender on or off
    (`VerifyService`), and the A2P 10DLC and toll-free registrations
    (`A2pForm`, `TollFreeForm`), each behind a confirmation.
  - Each write has its own `NumberWrite`. A write that costs money or cannot be
    undone is offered again after a failure only when the failure proves it did not
    happen.
  - One effect and one event carry every call: `Effect::NumberAdmin` with a
    `NumberCall`, `Event::NumberAdminDone` with a `NumberAnswer`, run by the new
    `DistrictApi::number_call`.
  - `district-api`: ten endpoints and their typed calls (`configure_number`,
    `release_number`, `lookup_number`, `provider_status`, `sip_trunks`,
    `create_sip_trunk`, `verify_service`, `set_verify_service`, `register_a2p`,
    `verify_toll_free`), each in `DESKTOP_ONLY`. The lookup is a read that is never
    repeated: the carrier bills it.
  - `district-model`: their types. No contract fixture records them yet; their
    shapes are the routes'.
- `district-l10n`: two new catalogs, `support.toml` (`support_strings`) and
  `numbers.toml` (`numbers_strings`), and the inbox's new words in `core.toml`.
- `district-l10n`: the wire vocabulary at the server's `46e1740a6`, from 11 categories
  to 31. `WireCategory` names the 20 new ones and `wire_label` labels their values,
  with a category's own fallback where the service has one.

### Changed

- `placeholder_label`: the summary of an unanswered call is now "No answer, no
  conversation took place."; rows stored with the old spelling, an em dash where the
  comma is, read as the same placeholder.
- Breaking: `SupportScreen` has a new field (`reporting`) and `SupportEvent` two new
  variants; `InboxScreen` a new field (`compose`) and `InboxEvent` a new variant;
  `MarketplaceScreen` a new field (`admin`), and `MarketplaceTab` and
  `MarketplaceEvent` new variants; `Effect` and `Event` one new variant each;
  `DistrictApi` a new method (`number_call`); `WireCategory` 20 new variants.
- Breaking: `MarketplaceScreen::read_only_note`, `READ_ONLY` and `READ_ONLY_VIEWER`
  are gone, with the catalog entries `marketplace.read_only` and
  `marketplace.read_only_viewer`: the screen is no longer read only.
  `MarketplaceScreen::scope_note` says what it does instead.

## [4.0.0-beta.1] - 2026-10-13

### Added

- `district-l10n`, a new crate holding every sentence and label the core writes,
  in English and Canadian French. There is no French yet: `FR_COMPLETE` is false
  and the French arm of every accessor carries the English.
  - `Locale` (`En`, `FrCa`) and `Locale::from_tags`: the first of the user's
    preferred languages that the core writes decides (`fr-*` French, `en-*`
    English, any other language passed over); none gives English.
  - `strings`, the accessors generated from `catalog/core.toml`, and `english`,
    the English of each one that takes no argument, as a constant.
  - `api_error_text`: the service's sentence for each of its error codes, from
    `contracts/i18n/api-errors.json`.
  - `wire_label`, `region_long_label` and `placeholder_label`: what a screen shows
    for a value the service stores in English (call status, direction, sentiment,
    disposition, transfer outcome, funnel stage, role, region, and the server's
    placeholders), from `contracts/i18n/wire-vocabulary.json`. A value this build
    does not know reads as "Other", the category's own fallback, or a region's
    code in capitals, never as the stored text turned into words. Adopting it in
    place of the desktop apps' `humanize` changes two English labels to the
    portal's: sentiment `negative` reads "Friction", and transfer status
    `success` reads "Transferred".
  - `format`: money, dates, times, durations, numbers, percentages, lists and
    plural categories, held byte for byte to the service's formatting vectors
    (`contracts/i18n/format-vectors.json`) in both languages.
- `CoreConfig::locale`, the language the core writes in, and `Model::set_locale`
  to change it. One per process. `district_core::Locale` and `district_core::l10n`
  re-export the new crate.
- `format_money`, an amount in cents in a named currency (`CA$249.00`,
  `249,00 $ CA`); with no currency it is `format_cents`, which writes what it
  always has. `district-model`: `currency` on `BillingSubscription` and
  `BillingInvoice`, read when the service names one, which it does not yet.
- `NumberSearchForm::number_types` and `SupportForm::kinds`, the localized
  counterparts of `NUMBER_TYPES` and `KINDS`.
- `contracts/i18n/`, a third set vendored by `scripts/sync-contracts.py`; the
  catalog tools in `tools/l10n/`; `scripts/gen-l10n.py`, which CI runs with
  `--check`.

### Changed

- Breaking: `CoreConfig` has a new field, `locale`. `Locale::En` keeps every
  word the core writes exactly as it was.
- Every sentence and label the core writes comes from `district-l10n`'s catalog,
  in the configured language. The English is unchanged, byte for byte.
- The public string constants (`FAILED_TITLE`, `EMPTY_TITLE` and the rest) keep
  their English values, now taken from the catalog's `english` constants. A
  screen shows the accessor of the same key from `strings`, in the chosen
  language.
- In French, a refusal is never shown in the service's English: a known error
  code reads as the catalog's sentence for it, anything else as the screen's own
  words. In English the service's own sentence is still shown where it wrote one.

## [3.1.1] - 2026-10-13

### Fixed

- `plan_name` names the plan ("Voice Pro") instead of returning the key the
  service stores it under ("VoicePro"). A key one of `PlanTier::ALL` publishes
  reads as its `PlanTier::label`, the name the service's own plan chooser shows;
  any other tier is shown as sent, and a workspace with none still reads "No plan
  on this workspace".

## [3.1.0] - 2026-10-09

### Added

- `SignedIn::no_workspace_message` and `WorkspacesState::CHOOSE_PLAN_MESSAGE`: an
  account with no workspace that is offered the plans (`SignedIn::offers_plans`) is
  told to choose a plan to set one up, not to contact support. Every other state,
  and every app that does not buy in the app, reads `WorkspacesState::message` as
  before.
- Waiting for the workspace a checkout paid for, which the service makes when the
  payment's webhook lands, possibly after its success page shows.
  `PurchaseState::setting_up` says the wait is on: no plans are offered meanwhile,
  so nobody pays twice, and `SignedIn::no_workspace_title` and
  `no_workspace_message` give `WorkspacesState::SETTING_UP_TITLE` and
  `SETTING_UP_MESSAGE`. `WORKSPACE_SETUP_WAITS` (2, 4, 8 and 16 seconds) are the
  waits between lists. When they are over with still no workspace, the plans are
  offered again with `WorkspacesState::CHOOSE_PLAN_AFTER_CHECKOUT_MESSAGE`. A
  failed list, signing out or a new purchase ends the wait.

### Changed

- `Event::EmbeddedClosed` with no workspace open lists the workspaces again
  (`Effect::LoadWorkspaces`), then again after each of `WORKSPACE_SETUP_WAITS`
  while the list is empty (`Effect::Wait`), and opens the workspace checkout made
  as soon as it is listed, without a restart. Only in an app with
  `CoreConfig::in_app_purchases`.

## [3.0.0] - 2026-10-09

### Added

- Buying in the app, for District AI for Windows only, through the service's own
  Stripe checkout and billing page shown inside the app window. Off unless the app
  sets `CoreConfig::in_app_purchases` (see Changed); with it off nothing below
  happens and billing stays read only.
  - `BillingEvent::ChoosePlan(PlanChoice)` with `PlanTier` (the four voice plans,
    by their published keys), `PlanTerm` (monthly or annual) and an optional
    `PromoCode` (taken only in the one shape the checkout reads, refused whole
    otherwise), then `ConfirmPurchase` or `CancelPurchase`; `ManageInApp` for the
    billing page; `DismissPurchaseNotice`. `SignedIn::offers_plans` (purchases on,
    and a role that can change the plan, or no workspace yet) and
    `SignedIn::offers_manage_in_app` say what to show.
  - Each starts the hand-off of 1.1.0's booking pages (S33), with its own `state`:
    the start page opens in a new private view inside the app
    (`Effect::OpenEmbedded` with `EmbeddedView::NewPrivate`), the
    `districtai://handoff` answer the app catches there comes back as
    `Event::HandOffCallback`, as from the browser, and the link is asked for with
    its nonce (`Effect::RequestBillingHandOff`, `Event::BillingHandOffReady`) and
    opened in the same view (`EmbeddedView::Same`). `BillingDestination::next`
    builds the only two destinations the service admits, `/checkout` with `tier`,
    `term`, `promo` and `reason=no-workspace`, or `/dashboard/district/billing`.
  - `UrlOpener::open_embedded`, defaulting to false ("not supported"). Then, or
    on `Event::EmbeddedUnavailable`, the purchase starts again in the system
    browser and `PurchaseState::in_browser` says so. `Event::EmbeddedClosed`
    gives up a start page the closed view held and reads the billing screen again.
  - "Purchases on this computer" (`PurchaseSetting`, `Off` or `SignInEveryTime`,
    the default), read at sign-in (`Effect::ReadPurchaseSetting`,
    `Event::PurchaseSettingRead`), changed with `Event::SetPurchases` and kept with
    `Effect::SavePurchaseSetting`. Off hides every purchase action and drops a
    purchase under way.
  - The words: `PurchaseState`'s constants (the confirmation naming Stripe, the
    in-app note, the browser fallback), `PurchaseSetting::LABEL` and `BODY`, the
    plan and term labels. No price is named; checkout shows them.
  - Each refusal of the hand-off maps to a notice, `invalid_next` among them.
- `district-api`: `ApiClient::billing_hand_off` for `POST /api/district/billing/handoff`,
  `Endpoint::BillingHandOff` (account-scoped, never repeated, on `DESKTOP_ONLY`).
- `district-model`: `BillingHandOffResponse` (the scheduling hand-off's two fields,
  as the service answers both) and `CODE_INVALID_NEXT`.
- `district-host`: the settings file keeps `purchases`, only once it is set, so no
  other app's file changes.

### Changed

- Breaking: `CoreConfig` has a new field, `in_app_purchases`. District AI for Linux
  sets it false, and then every event, effect and view is what 2.0.0 had.
- Breaking: `DistrictApi` has a new required method, `billing_hand_off`.
- Breaking: `Settings` has two new required methods, `purchases` and
  `set_purchases`. `district-host`'s `SettingsFile` implements them, and
  `Preferences` has a new field, `purchases`.
- Breaking: new `Event` variants (`SetPurchases`, `EmbeddedClosed`,
  `PurchaseSettingRead`, `BillingHandOffReady`, `EmbeddedUnavailable`), `Effect`
  variants (`OpenEmbedded`, `RequestBillingHandOff`, `ReadPurchaseSetting`,
  `SavePurchaseSetting`) and `BillingEvent` variants (`ChoosePlan`,
  `ConfirmPurchase`, `CancelPurchase`, `ManageInApp`, `DismissPurchaseNotice`), and
  a new `SignedIn` field, `purchase`.
- `UrlOpener` has a new method, `open_embedded`, with a default, so an existing
  implementation keeps compiling and never shows a page inside the app.

## [2.0.0] - 2026-10-09

### Added

- `district-auth`: the authorize URL names the app's platform (`platform=linux` or
  `platform=windows`, `Platform::wire`), after `redirect_uri`. The service offers
  account creation on its sign-in page to the Windows app only, binding the value to
  the attempt's challenge and state; it reads any other value, or none, as before.
- The live transcript of the call on this desktop, with the semantics of
  district-core-swift 6.0.0's `TranscriptReducer`. Once the call has an id (a placed
  call's from the dial's answer, an answered call's from its ring), the core asks the
  call's workspace socket for it with the new `Effect::WatchTranscript`, and applies the
  `transcript_*` frames that socket delivers for that call to
  `ActiveCall::transcript()`, a `LiveTranscript`: `lines()` in (epoch, index) order with
  their speaker and text, interim lines (`is_final` false) replaced in place by later
  revisions and their finals, `phase()` (`Subscribing`, `Live`, `Reconnecting`,
  `Ended`, `Unavailable`), `is_complete()`, and `final_transcript()`, the full
  transcript read with backoff once the live one ends. `status()`, `speaker_label()` and
  the `LiveTranscript` constants are the words both apps show. Duplicates, gaps (healed
  after `GAP_HEAL` with one subscribe in flight), snapshots in parts, retractions,
  `agent_error`, `rate_limited` and `not_live` (retried only on a call event showing the
  call in progress, at most once per `NOT_LIVE_RETRY`) follow the contract. A frame for
  another call or workspace, one that cannot be read, and one after the call ended
  (except a retraction, which still takes lines off) change nothing.
- `district-model`: `TelemetryEventType` names the five transcript events
  (`TranscriptSnapshot`, `TranscriptSegment`, `TranscriptEnded`, `TranscriptRetracted`,
  `TranscriptError`, and `is_transcript()`), `TelemetryEnvelope::transcript_event()`
  reads their data into `TranscriptEvent` (refusing another `v`, a call id that
  disagrees with the envelope's, and text over `TRANSCRIPT_TEXT_MAX_UTF16`), with the open
  vocabularies `TranscriptSpeaker`, `TranscriptEndReason`, `TranscriptRetractReason` and
  `TranscriptErrorCode`; a segment's text stays out of `Debug`. `TranscriptClientOp`
  writes the client's ops.
- `district-live`: a connection keeps the calls whose transcript it receives
  (`subscribe_transcript`, `unsubscribe_transcript`, `resubscribe_transcript`, on
  `TelemetryConnection` and per workspace on `TelemetryHub`) and sends a
  `transcript.subscribe` for each on every socket it opens, renewals included. An op it
  sends does not count as the server speaking. `is_transient` is public.
- `LiveHub::transcripts()` names the calls whose transcript the sockets are asked for.

### Changed

- Breaking: `TelemetryEventType` gains five variants, so an exhaustive `match` over it
  needs them; `Effect` gains `WatchTranscript`; and `LiveUpdates` gains
  `watch_transcript`, which `LiveHub` implements. The six transcript fixtures are now
  decoded with their typed data in the contract gate, not as envelopes of an unknown
  type.

### Fixed

- `Event::Quitting` saves the open thread's reply when its save is still waiting for
  the typing to stop, and sends every draft write queued behind the one on its way.
  Before, a reply typed in the last two seconds before quitting, or queued behind a
  slow save, was lost.

## [1.2.1] - 2026-10-13

### Fixed

- Billing names the plan as the service's pricing page does (`plan_name`): "Voice Solo",
  "Voice Starter", "Voice Pro" and "Voice Studio", not the tier as the service stores it
  ("VoicePro"). A tier this build does not know is still shown as the service spells it,
  and a workspace with no tier still reads "No plan on this workspace". A backport to the
  1.2 line, for the apps that pin it; nothing an app calls or implements changes.

## [1.2.0] - 2026-10-08

### Changed

- The contract fixtures follow the service's current shapes. The Android set gains nine
  files and loses none: the live call transcript's six frames on the telemetry socket,
  the telemetry token (the service now writes the desktop set's file to both sets, byte
  for byte, because the mobile apps open the same socket), and the two answers of the
  Apple apps' native second sign-in step. The desktop set is unchanged.
- The live transcript's frames decode as `TelemetryEnvelope`s of an event type this
  client does not name (`TelemetryEventType::Unknown`), on purpose: the desktop apps
  show no live transcript, and the core reads nothing again for them, so a call's
  stream of transcript frames costs no requests. Tests hold both to that.
- The two native second-step fixtures are excluded by decision: a desktop signs in
  through the browser, which asks for the second factor itself.

### Removed

- `AiPersona::avatar_recording` (`videoRecording` on the wire). It named a recording of
  avatar calls the service has never made, and the service no longer stores it. A shipped
  build still reads a persona that carries the key, which it ignores. This is a change
  to a public type; no app pins the core yet, so it ships in a minor release.
  `CallSummary::recording_url` stays, always `None`, because the service still sends
  `recordingUrl: null` for older clients that require the key.

## [1.1.0] - 2026-10-08

### Security

- "Manage on the web" binds the sign-in it hands to the browser to the browser the app
  opened. The core first opens the service's hand-off start page
  (`HAND_OFF_START_PATH`) with a fresh `state`; the browser answers through a
  `districtai://handoff` link carrying a one-time nonce that the service also keeps in
  that browser as a short-lived cookie, and the link is asked for with that nonce, so it
  signs in only the browser holding the cookie. A link sent to someone else's browser no
  longer signs that browser in to your account. A `districtai://handoff` link that does
  not answer the hand-off under way is dropped and leaves it waiting. If the browser
  does not answer within ten seconds (`HAND_OFF_CALLBACK_WAIT`), the link is asked for
  unbound, as before, which the service accepts until it requires the binding; if no
  browser took the start page, the hand-off is given up. The `state` and the nonce are
  redacted from `Debug` and never logged.

### Added

- district-auth: `HandOffState`, `HandOffNonce`, `HandOffError`, `HAND_OFF_START_PATH`,
  `HAND_OFF_HOST`, `HAND_OFF_NONCE_LEN`, `is_valid_hand_off_state` and
  `is_valid_hand_off_nonce`.
- district-model: `CODE_INVALID_NONCE` and `CODE_NONCE_REQUIRED`, the codes of the
  service's two 400s for a hand-off's nonce, each shown in words of the app's own.
- district-core: `Event::HandOffCallback`, and `Event::from_link`, which turns a link
  the desktop hands the app into the sign-in's or the hand-off's event by its host, so
  an app routes every `districtai:` link through one call. `HandOffLeg`,
  `HAND_OFF_CALLBACK_WAIT`, and `SchedulingScreen::opening` and
  `SchedulingScreen::minting`.

### Changed

- These change what an app calls or matches on. No app pins the core yet, so they ship
  as a minor release, as the plan for this release set out:
  - `ApiClient::scheduling_hand_off` and `DistrictApi::scheduling_hand_off` take a
    `nonce: Option<&str>`; without one the key is left out of the body, never sent as
    `null`.
  - `Effect::RequestSchedulingHandOff` has a `nonce` field.
  - `SchedulingScreen::opening`, a field, is now `SchedulingScreen::hand_off`, a
    `HandOffLeg`; `opening()` and `minting()` say what the field said.
  - "Manage on the web" now yields `Effect::OpenOneTimeUrl` for the start page and an
    `Effect::Wait`, rather than `Effect::RequestSchedulingHandOff` at once.

## [1.0.0] - 2026-10-08

### Added

- The shared Rust core of the District AI desktop apps, as its own repository:
  district-model, district-api, district-auth, district-live, district-core,
  district-call and district-host, the contract fixtures they are tested against, and
  the scripts that vendor those fixtures and check this repository. It is extracted
  from District AI for Linux 2.1.0
  ([distronode-corporation/district-linux](https://github.com/distronode-corporation/district-linux))
  with the history of every file it holds; a commit that names a pull request names it
  in that repository (`distronode-corporation/district-linux#NN`). District AI for
  Linux and District AI for Windows depend on it by tag.
- `Platform` and `ClientIdentity` in district-model: the app a request comes from is
  named once, by the app, and the crates write it on the wire (the User-Agent, the
  sign-in's platform, the desktop's presence registration and the identity a room is
  joined with). The User-Agent carries the app's version, not the crate's. Linux's
  bytes on the wire are unchanged.
- district-host: what a desktop app keeps on the machine and how it hears the machine
  sleep, on any operating system (the device id, the refresh marker, the settings file
  and the sleep protocol), split out of District AI for Linux's Linux-only crate.
- CI on Windows (`windows-2025`, MSVC) as well as Linux, and a `deny.toml` that judges
  the crates that build only on Windows as strictly as the rest.

### Changed

- The version starts again at 1.0.0 for this repository; it says nothing about the
  apps' own versions.
- The endpoint table's desktop additions are `DESKTOP_ONLY` (they were `LINUX_ONLY`),
  because a desktop app on another system shares them.
