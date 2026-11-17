# Security model

This is the assurance case for the District AI core for Rust: who can attack it, where the
trust boundaries are, the secure design principles the code applies, and the common
weaknesses it counters, each with the mechanism and the test that holds it. The detailed
guarantees, crate by crate, are the security model in
[SECURITY.md](SECURITY.md#security-model), and the design is in
[ARCHITECTURE.md](ARCHITECTURE.md).

## What you can and cannot expect

You can expect these crates to keep a member's tokens in memory or in the app's secret store
and nowhere else, to send them only to the District AI service over verified TLS, to refuse
a sign-in or hand-off answer they did not ask for, and to log nothing. You cannot expect
them to protect against software running as the same user (which can ask the secret store
for the user's items), to enforce the service's rules (the service does that on every
request; the core only decides what to offer), or to cover the half of the model each app
adds (where the refresh token is stored, how the sign-in link reaches the app), which is in
that app's SECURITY.md.

## Threat model

| actor | what they can do | what the crates do about it |
| --- | --- | --- |
| Another program on the same machine | Register the `districtai://` scheme and receive or inject a sign-in or hand-off callback | PKCE: an intercepted code is useless without the verifier, which never leaves the process. A callback whose `state` is not the attempt's (compared in constant time), or that repeats a parameter, is refused before anything in it is used; an attempt is used up by its first callback |
| Someone on the network path | Read or alter traffic, or answer as the service | rustls with certificate verification against the operating system's trust store and no option to turn it off; plain `ws` and `http` refused unless the address is this machine |
| A hostile or compromised server, or a redirect | Send the token elsewhere, or send data the client does not expect | Redirects are never followed; every response is decoded into a typed `district-model` value or is an error; a live event that cannot be read, or names another workspace, is dropped unread |
| Someone who sends the member a link | Get a browser signed in to their account, or open an arbitrary page | A web hand-off is bound to the browser the app opened with a fresh `state` and a service nonce; links are opened only when they are web pages, and hand-off links only on the service's own address over HTTPS |
| Someone with the member's logs, crash reports or screenshots of debug output | Read a token, a credential or customer data | Nothing logs or prints; `log`'s `max_level_warn` compiles lower levels out of the whole build; every type holding a credential redacts it from `Debug`; no error carries a token, a code or a response body; crash reports are opt-in and scrubbed |
| A thief replaying a refresh token | Use a spent token | The service rotates it on every refresh; the core refreshes one at a time and records a fingerprint before sending, so a refresh interrupted mid-way asks for a new sign-in instead of presenting a token that may be spent |

## Trust boundaries

1. **The app and the core.** The app is trusted to forward the user's own actions as events
   and to implement its traits (the secret store, notifications, opening links) faithfully.
2. **The process and the service.** Everything from the network is untrusted until decoded
   and checked: API responses, live events, the media server's reports.
3. **The process and the browser.** The system browser carries sign-in and hand-offs, and
   its answers come back through a URI scheme any program can register.
4. **The process and the disk.** Only the device id, the refresh marker (a fingerprint, not
   a token) and the settings file are written, owner-only on Unix. The refresh token
   crosses into the app's secret store and nowhere else.
5. **The process and libwebrtc.** With the `livekit` feature, a C++ media library runs in the
   process; the engine creates the runtime that routes its logging away from stderr before
   anything can reach it, and refuses the audio combinations SECURITY.md describes.

## Secure design principles applied

- **Least privilege.** The crates start no service and ask for no privilege. The client
  sends a bearer token and an explicit workspace on every call, with no cookie store, so a
  request can act only where it says. The live credential is minted for one workspace and
  expires after fifteen minutes. CI workflows start read-only (`permissions: contents: read`
  at the top of `.github/workflows/ci.yml`).
- **Fail closed.** A callback or hand-off answer that does not match exactly is dropped. A
  response that does not decode is an error. A server that selects the live credential as
  its WebSocket protocol is refused. An interrupted refresh ends the session rather than
  risk presenting a spent token. Crash reports are off until both a DSN and consent are
  given.
- **Complete mediation.** Every API call goes through the one `ApiClient`, which adds the
  token and the workspace and refuses redirects; every live connection goes through the one
  TLS configuration shared with the API client; every user-facing link passes the same
  checks before it is opened. Billed or irreversible actions become effects only after the
  member's own event, one at a time.
- **Economy of mechanism.** The core is a pure `Model::update` from events to effects, so
  each decision is a function the tests drive directly. One HTTP client, one TLS
  configuration, one refresh coordinator.
- **Memory safety.** `unsafe_code = "forbid"` for the whole workspace (`Cargo.toml`).
- **Open design.** The crates, their tests and the recorded responses they are tested
  against are public; nothing relies on the code being secret.

## Common weaknesses countered

Mapped to the CWE Top 25 and the OWASP Top 10 where they apply to a client library.

| weakness | how it is countered | where it is tested |
| --- | --- | --- |
| Cross-site request forgery and login CSRF (CWE-352) | Random `state` per attempt, compared in constant time; hand-offs bound to the browser by a nonce | `crates/district-auth/tests/login.rs`, `crates/district-auth/tests/handoff.rs` |
| Authorization code interception (CWE-294, CWE-345) | PKCE with S256; the verifier held in memory for one attempt | `crates/district-auth/tests/pkce_vectors.rs`, `crates/district-auth/tests/login.rs` |
| Improper certificate validation (CWE-295) | rustls with the operating system's verifier (`rustls-platform-verifier`); no setting disables it | `crates/district-live/tests/network.rs` (`wss_speaks_tls_before_anything_else`) |
| Cleartext transmission (CWE-319) | `wss` and `https` only, plain only to this machine | `crates/district-live/tests/connection.rs`, `crates/district-live/tests/network.rs` |
| Open redirect and credential forwarding (CWE-601) | `reqwest::redirect::Policy::none()` (`crates/district-api/src/config.rs`) | `crates/district-api/tests/responses.rs` (`a_redirect_is_an_error_and_is_never_followed`) |
| Sensitive information in logs and errors (CWE-532, CWE-209) | No logging; `max_level_warn` ceiling; `Debug` redaction; errors carry no token, code or body | `crates/district-live/tests/rules.rs`, `crates/district-auth/tests/login.rs` (`no_error_names_the_code_or_the_state`) |
| Insecure storage of credentials (CWE-922, CWE-312) | Refresh token only behind `SessionStore`; files owner-only on Unix and holding nothing secret | `crates/district-host/tests/files.rs`, `crates/district-host/tests/settings.rs` |
| Improper input validation and deserialization of untrusted data (CWE-20, CWE-502) | serde into typed values with required fields; recorded server responses decoded with unknown fields refused in the tests | `crates/district-model/tests/contracts/` |
| Memory safety (CWE-787, CWE-416) | Safe Rust only, `unsafe_code = "forbid"` | The compiler |
| Reaching surfaces the client must never call | An endpoint table held to the Android app's, plus a scan of every `src/` for forbidden paths | `crates/district-api/tests/endpoint_parity.rs`, `crates/district-api/tests/forbidden_literals.rs` |

Dependencies are checked by `cargo deny` (advisories, licences, bans and sources) on every
pull request and push to main (`ci.yml`), and for advisories again every week
(`audit.yml`). CodeQL analyses the Rust code.
