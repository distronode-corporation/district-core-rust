# Security Policy

## Reporting a vulnerability

Please report privately, not in a public issue.

- **Preferred:** GitHub's private vulnerability reporting. Open the repository's
  **Security** tab and choose **Report a vulnerability**, or go straight to
  <https://github.com/distronode-corporation/district-core-rust/security/advisories/new>.
- **Fallback:** email **opensource@distronode.com** if you cannot use GitHub.

Include what you did, what happened, and what you expected. A proof of concept is
welcome but not required. Never include a real access or refresh token, and never
include another person's data (a call transcript, a phone number, a contact); if one
is part of the problem, say where it appeared, not what it was.

A problem you found through one of the apps built on these crates (District AI
for Linux, District AI for Windows) can be reported here or in that app's
repository; if you cannot tell which part is at fault, either is fine, and we
move it.

Expect an acknowledgement within a few working days. There is no paid bug bounty;
what you get is credit in the changelog entry for the fix, if you want it.

## Supported versions

Only the latest release is supported. Fixes go into a new release rather than being
backported, and the apps move their pin to it.

| Version | Supported |
| --- | --- |
| 2.0.x (the current release, [2.0.0](https://github.com/distronode-corporation/district-core-rust/releases/latest)) | Yes |
| Anything older | No |

## Security model

What these crates decide, so a report can say which part of it breaks. Each app
adds its own half (where the refresh token is stored, how the sign-in link
reaches the running app, notifications, file choosers, the sleep signal), and
that app's SECURITY.md describes it.

### Sign-in (district-auth)

- Sign-in is OAuth 2.0 authorization code with PKCE, in the system browser. The
  crates never show a password field and never see a password.
- The browser returns to the app through the custom URI scheme `districtai://auth`.
  Another application on the same machine can register the same scheme and receive
  the authorization code. PKCE neutralises that: the code is useless without the
  verifier, and the verifier never leaves the process. It is held in memory for
  one sign-in attempt and never written anywhere, which is why each app keeps
  the process that started an attempt running until the browser answers.
- Each attempt also carries a random `state`. A callback whose `state` is not the
  attempt's (compared in constant time), or that repeats a parameter, is refused
  before anything in it is used, so a code injected by another program, or an old
  callback replayed from the browser's history, is never exchanged. An attempt is
  used up by its first callback, whatever the outcome.

### Handing off to the web (district-auth, district-core)

- "Manage on the web" asks the service for a one-time link that signs the
  browser in. On its own such a link signs in whichever browser opens it, so it
  is bound to the browser the app opened: the app first opens the service's
  start page (`/dashboard/handoff/start`) there with a fresh random `state`; the
  service leaves a nonce in that browser as a short-lived cookie and answers
  through `districtai://handoff` with the `state` and the nonce; the app asks for
  the link with that nonce, and the service redeems it only in a browser holding
  the matching cookie. A link minted by someone else and sent to you therefore
  does not sign your browser in to their account.
- The answer is taken only when it is `districtai://handoff` with no port, user
  or path, carries exactly one `state` equal to the one this hand-off generated
  (compared in constant time) and exactly one nonce of the shape the service
  sends. Anything else is dropped and leaves the hand-off waiting for its own
  answer, so a stray or forged link cannot cancel it.
- If no answer arrives within ten seconds, the link is asked for unbound, which
  the service accepts until it requires the binding. If no browser took the
  start page, the hand-off is given up instead.
- The `state`, the nonce and the link are held in memory only, redacted from
  `Debug` (the events, effects and screen state that carry them included), and
  never logged.

### Tokens (district-auth, district-host)

- The refresh token lives behind the `SessionStore` trait, which each app
  implements over its system's secret store. These crates never write it to a
  plain file, a log or a settings file. The access token is kept only in memory.
- The service rotates the refresh token on every refresh and treats a second
  presentation of one as theft. The refresh coordinator therefore refreshes one
  request at a time, records a SHA-256 fingerprint of the token it is about to
  send (never the token) through `RefreshMarkerFile` before sending, and saves the
  successor before using it. If the app stops mid-refresh, the next start finds
  the fingerprint and asks the user to sign in again rather than present a token
  that may already be spent.
- The settings file (`SettingsFile`) holds "ring on this computer" and the id of
  the workspace last chosen, and nothing secret. On Unix it and the other files
  are created readable by their owner only.
- The device id sent at sign-in is a random UUID (`DeviceIdentity`). It is not
  derived from the machine.
- Signing out forgets the access token, asks the service to revoke the refresh
  token, and removes it from the store. If the service cannot be reached, the
  token is kept in the store, apart from the session, and presented for
  revocation again at the next start, because the service may otherwise honour it
  until it expires.
- Every type that holds a credential redacts it from its `Debug` output, and no
  error carries a token, a code or a response body.

### Requests (district-api)

- Every API call carries the access token as a bearer header and names its
  workspace explicitly. There is no cookie store.
- HTTP redirects are not followed, so the token never travels to a host the app did
  not choose. TLS is rustls with certificate verification and no option to turn it
  off, and the operating system's trust store.
- The client names the app on the wire through `ClientIdentity` (the platform,
  the app's name and the app's version), so the service can tell the apps apart.
  It sends nothing else about the machine.

### Live updates (district-live, district-core)

- Calls and messages arrive over a WebSocket, one per workspace, opened with a
  credential the service mints for that workspace alone and that expires after
  fifteen minutes. It is kept only in memory and replaced before it expires.
- The credential is sent in the `Sec-WebSocket-Protocol` header, after the
  protocol's version marker, never in the URL. The server must select the version
  marker: a server that selects the credential (and so sends it back) is refused.
- The socket is `wss`, with the same TLS configuration and certificate store as the
  API calls. Plain `ws` is refused unless the address is this machine.
- Events are customer data. Nothing in these crates logs them, no error or status
  carries one, and one that cannot be read, or that names another workspace, is
  dropped unread. An event is only a hint: the core reads what it changed again
  with its own session rather than show what the event carried.

### Logging

Nothing in these crates logs or prints. The `log` crate is a dependency only for
its `max_level_warn` feature, which compiles every `info!`, `debug!` and `trace!`
in every crate of a build out of the binary, so no logger, however an app
configures it, can bring any of them back. Cargo unifies features across a
build, so an app that depends on district-live or district-call gets the same
ceiling for every crate it compiles. It is there because the WebSocket library
logs the handshake (credential included) and every message at trace level, and
because the media library logs an encrypted room's key at debug level and
participants' identities (which can be a caller's number) at debug and info.
A test in district-live fails if the ceiling goes, and so does one among
district-call's engine tests, which voice.yml runs.

### The core's decisions (district-core)

- A billed or irreversible action (a reply written by the model, research on a
  contact, a call placed, an audition, a District HQ question, a confirmed
  change, a deletion) is an effect only after the member's own event asks for
  it, one at a time, and never again by itself after a failure.
- What a role may do follows the service's rules. The service refuses a viewer
  every route behind the help desk and support requests, reads included, and the
  core sends none of those requests for a member whose role it has read as
  viewer. The service enforces every rule on every request; the core only
  decides what is offered.
- Three settings saves replace a stored list with exactly what they are sent (the
  allowed tools, the call directory and the routing rules), so an empty or
  half-loaded form would be a deletion the service reports as a success. A list
  is saved only as the list just read with the member's edits applied, each
  stored entry sent back whole, keys the core does not know included, and after
  a save the settings are read back before anything else can be saved.
- Carrier credentials typed by the member are held only while the form that took
  them is open, are sent only in the body of a save or a credential check, and
  print only whether they are set. Each carrier's credentials are their own type,
  so a secret cannot go out under another carrier's field names.
- A link opened from a District HQ answer is opened only when it goes to a web
  page (`https://` or `http://` and a host). A hand-off link to the web is
  opened only when it is on the service's own address over HTTPS, and never
  after the user opened another workspace or signed out.

### Calls (district-core, district-call)

- A desktop has no push service. While "ring on this computer" is on and the
  machine is awake, the core registers the desktop's presence with the service:
  the platform, `kind: "desktop"` and a random value made by each run of the app,
  which identifies nothing else. The registration is renewed every five minutes,
  and removed when the setting is turned off, before the machine sleeps, when the
  app quits, and as the first step of signing out. Changes go out one at a time,
  and one that arrives after a later one was sent is dropped.
- The core rings only for a `call_ringing` event that names its own user, while
  the setting is on and the member's role may answer. The event carries ids only.
  Answering asks the service for the call's media credential once, only when the
  member answers; declining sends nothing.
- Media credentials and encryption passphrases are kept in memory only, redacted
  from `Debug` (the events and effects that carry them included), and dropped
  when the call, room or audition ends.

What the LiveKit call engine does (district-call, with its `livekit` feature):

- The media server is the one the service names, used exactly as named, and the
  credential goes to it only over TLS (`wss`), with rustls and the operating
  system's certificates. Plain `ws` is refused unless the address is this
  machine, before anything is sent.
- An encrypted room's passphrase is handed to the media library as the text it
  is, as bytes, and never decoded. The key is derived from it the way the web and
  Android clients derive it, with the web client's settings, and each session
  gets a key of its own. Encrypted audio that never decrypts, or that arrives in
  a session with no key, is reported as a failure to decrypt rather than played.
- Only audio is received. Video in a room is noted and never subscribed to.
- The engine creates the WebRTC runtime before anything can reach libwebrtc,
  because that is what routes libwebrtc's own logging away from stderr. Its tests
  run a whole encrypted call with a logger taking every record and read
  everything the process writes, looking for the passphrases, the derived keys,
  the credentials and the identities.
- A process has the system's audio devices or a frame microphone
  (`Audio::Frames`, for tests), never both, for its whole life: whichever it asks
  for first, the other is refused. The refusal is deliberate: certain audio
  capture and track combinations are unsafe because of defects in the libwebrtc
  the LiveKit SDK links, which have been reported privately upstream. Details
  will be published once upstream has published a fix; until then, the refusal
  stays, and the engine's tests hold it in both orders.

### Privileges

The crates contain no `unsafe` code (`unsafe_code = "forbid"` for the whole
workspace), start no service and ask for no privilege.

## Scope

In scope, for example:

- A token reaching disk outside the session store, a log line, an error message or
  a `Debug` print.
- A token being sent to a host other than the District AI service.
- Completing sign-in with an authorization code the client did not request.
- One workspace's data being shown or sent in the context of another.

Out of scope:

- An operating system's secret store's own security model, and the fact that
  anything running as the user can ask it for the user's items.
- Vulnerabilities in the District AI service itself rather than in this client.
  Report those privately to the contact published in
  <https://www.distronode.com/.well-known/security.txt>.
