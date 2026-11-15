//! Two-step verification, the fresh re-authentication and the account's
//! deletion: what each call sends, what it decodes from the service's recorded
//! answers, and which answers are not the success they look like.

mod common;

use std::fs;
use std::path::PathBuf;

use common::client;
use district_api::{ApiError, Endpoint};
use district_model::{Credential, ReauthAction};
use serde_json::{Value, json};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

fn fixture(name: &str) -> Value {
    let file = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../contracts/fixtures")
        .join(name);
    serde_json::from_str(&fs::read_to_string(file).unwrap()).unwrap()
}

async fn serving(verb: &str, route: &str, status: u16, body: Value) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method(verb))
        .and(path(route))
        .respond_with(ResponseTemplate::new(status).set_body_json(body))
        .mount(&server)
        .await;
    server
}

async fn only_request(server: &MockServer) -> Request {
    let mut requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 1, "exactly one request");
    requests.remove(0)
}

fn body_of(request: &Request) -> Value {
    serde_json::from_slice(&request.body).unwrap()
}

fn code(value: &str) -> Credential {
    Credential::new(value)
}

#[tokio::test]
async fn the_status_is_read_and_must_hold_together() {
    let server = serving(
        "GET",
        "/api/auth/mfa",
        200,
        fixture("district-mfa-status.json"),
    )
    .await;
    let status = client(&server).mfa_status().await.unwrap();
    assert!(status.enrolled);
    assert_eq!(status.backup_codes_remaining, 7);
    assert!(status.can_disable);
    let request = only_request(&server).await;
    assert!(request.body.is_empty());
    assert_eq!(request.url.query(), None);

    let server = serving(
        "GET",
        "/api/auth/mfa",
        200,
        json!({"enrolled": true, "enrolledAt": null, "backupCodesRemaining": 0, "canDisable": true}),
    )
    .await;
    assert_eq!(
        client(&server).mfa_status().await.unwrap_err(),
        ApiError::Unconfirmed {
            endpoint: Endpoint::MfaStatus
        }
    );
}

#[tokio::test]
async fn enrolling_sends_no_body_and_answers_the_secret() {
    let body = fixture("district-mfa-enroll.json");
    let server = serving("POST", "/api/auth/mfa/enroll", 200, body.clone()).await;
    let enrollment = client(&server).mfa_enroll().await.unwrap();
    assert_eq!(serde_json::to_value(&enrollment).unwrap(), body);
    assert_eq!(enrollment.secret, "JBSWY3DPEHPK3PXP");
    assert!(only_request(&server).await.body.is_empty());
}

#[tokio::test]
async fn confirming_sends_the_code_and_answers_the_recovery_codes() {
    let server = serving(
        "POST",
        "/api/auth/mfa/confirm",
        200,
        fixture("district-mfa-confirm.json"),
    )
    .await;
    let confirmed = client(&server).mfa_confirm(&code("123456")).await.unwrap();
    assert_eq!(confirmed.backup_codes, ["aaaa-bbbb", "cccc-dddd"]);
    assert_eq!(
        body_of(&only_request(&server).await),
        json!({"totp": "123456"})
    );

    let server = serving(
        "POST",
        "/api/auth/mfa/confirm",
        200,
        json!({"enrolled": false}),
    )
    .await;
    assert_eq!(
        client(&server)
            .mfa_confirm(&code("123456"))
            .await
            .unwrap_err(),
        ApiError::Unconfirmed {
            endpoint: Endpoint::MfaConfirm
        }
    );
}

#[tokio::test]
async fn turning_off_sends_no_body_and_must_say_it_is_off() {
    let server = serving(
        "POST",
        "/api/auth/mfa/disable",
        200,
        fixture("district-mfa-disable.json"),
    )
    .await;
    client(&server).mfa_disable().await.unwrap();
    assert!(only_request(&server).await.body.is_empty());

    let server = serving(
        "POST",
        "/api/auth/mfa/disable",
        200,
        json!({"enrolled": true}),
    )
    .await;
    assert_eq!(
        client(&server).mfa_disable().await.unwrap_err(),
        ApiError::Unconfirmed {
            endpoint: Endpoint::MfaDisable
        }
    );
}

#[tokio::test]
async fn a_provider_reauthentication_sends_the_code_the_verifier_and_the_action() {
    let server = serving(
        "POST",
        "/api/auth/native/reauth/sso",
        200,
        fixture("district-native-reauth.json"),
    )
    .await;
    let seconds = client(&server)
        .native_reauth_sso(
            ReauthAction::DeleteAccount,
            &code("one-time"),
            "verifier",
            "districtai://auth",
        )
        .await
        .unwrap();
    assert_eq!(seconds, 300);
    assert_eq!(
        body_of(&only_request(&server).await),
        json!({
            "action": "delete-account",
            "code": "one-time",
            "codeVerifier": "verifier",
            "redirectUri": "districtai://auth",
        })
    );
}

#[tokio::test]
async fn a_proof_for_another_action_or_not_confirmed_is_refused() {
    for answer in [
        fixture("district-native-reauth.json"),
        json!({"success": false, "action": "2fa-change", "expiresInSeconds": 300}),
    ] {
        let server = serving("POST", "/api/auth/native/reauth/sso", 200, answer).await;
        assert_eq!(
            client(&server)
                .native_reauth_sso(ReauthAction::TwoStepChange, &code("c"), "v", "r")
                .await
                .unwrap_err(),
            ApiError::Unconfirmed {
                endpoint: Endpoint::NativeReauthSso
            }
        );
    }
}

#[tokio::test]
async fn a_password_reauthentication_sends_the_code_only_once_asked() {
    let answer = json!({"success": true, "action": "2fa-change", "expiresInSeconds": 120});
    let server = serving(
        "POST",
        "/api/auth/native/reauth/password",
        200,
        answer.clone(),
    )
    .await;
    let seconds = client(&server)
        .native_reauth_password(ReauthAction::TwoStepChange, &code("pw"), None)
        .await
        .unwrap();
    assert_eq!(seconds, 120);
    assert_eq!(
        body_of(&only_request(&server).await),
        json!({"action": "2fa-change", "password": "pw"})
    );

    let server = serving("POST", "/api/auth/native/reauth/password", 200, answer).await;
    client(&server)
        .native_reauth_password(
            ReauthAction::TwoStepChange,
            &code("pw"),
            Some(&code("654321")),
        )
        .await
        .unwrap();
    assert_eq!(
        body_of(&only_request(&server).await),
        json!({"action": "2fa-change", "password": "pw", "code": "654321"})
    );

    let server = serving(
        "POST",
        "/api/auth/native/reauth/password",
        200,
        json!({"success": true, "action": "delete-account", "expiresInSeconds": 300}),
    )
    .await;
    assert_eq!(
        client(&server)
            .native_reauth_password(ReauthAction::TwoStepChange, &code("pw"), None)
            .await
            .unwrap_err(),
        ApiError::Unconfirmed {
            endpoint: Endpoint::NativeReauthPassword
        }
    );
}

#[tokio::test]
async fn the_refusals_keep_the_service_code() {
    for (status, code_name) in [
        (401, "MFA_REQUIRED"),
        (403, "incorrect_password"),
        (400, "mfa_code_invalid"),
        (409, "NO_PASSWORD_SET"),
    ] {
        let server = serving(
            "POST",
            "/api/auth/native/reauth/password",
            status,
            json!({"error": "refused", "code": code_name}),
        )
        .await;
        let error = client(&server)
            .native_reauth_password(ReauthAction::DeleteAccount, &code("pw"), None)
            .await
            .unwrap_err();
        assert_eq!(error.code(), Some(code_name), "{error:?}");
    }
}

#[tokio::test]
async fn deleting_sends_the_typed_email_and_answers_the_pending_erasure() {
    let server = serving(
        "DELETE",
        "/api/auth/delete-account",
        200,
        fixture("district-account-delete.json"),
    )
    .await;
    let pending = client(&server)
        .delete_account("Ada@example.com")
        .await
        .unwrap();
    assert!(!pending);
    assert_eq!(
        body_of(&only_request(&server).await),
        json!({"confirmation": "Ada@example.com"})
    );

    let server = serving(
        "DELETE",
        "/api/auth/delete-account",
        200,
        json!({"success": true, "accountDeleted": true, "externalErasurePending": true}),
    )
    .await;
    assert!(
        client(&server)
            .delete_account("a@example.com")
            .await
            .unwrap()
    );

    for answer in [
        json!({"success": true, "accountDeleted": false}),
        json!({"success": false, "accountDeleted": true}),
    ] {
        let server = serving("DELETE", "/api/auth/delete-account", 200, answer).await;
        assert_eq!(
            client(&server)
                .delete_account("a@example.com")
                .await
                .unwrap_err(),
            ApiError::Unconfirmed {
                endpoint: Endpoint::DeleteAccount
            }
        );
    }
}

#[tokio::test]
async fn a_failure_comes_through_as_the_error_it_is() {
    let server = MockServer::start().await;
    Mock::given(wiremock::matchers::any())
        .respond_with(ResponseTemplate::new(503))
        .mount(&server)
        .await;
    let client = client(&server);
    let server_error = |error: ApiError| matches!(error, ApiError::Server { status: 503, .. });
    assert!(server_error(client.mfa_status().await.unwrap_err()));
    assert!(server_error(client.mfa_enroll().await.unwrap_err()));
    assert!(server_error(
        client.mfa_confirm(&code("1")).await.unwrap_err()
    ));
    assert!(server_error(client.mfa_disable().await.unwrap_err()));
    assert!(server_error(
        client
            .native_reauth_sso(ReauthAction::DeleteAccount, &code("c"), "v", "r")
            .await
            .unwrap_err()
    ));
    assert!(server_error(
        client
            .native_reauth_password(ReauthAction::DeleteAccount, &code("p"), None)
            .await
            .unwrap_err()
    ));
    assert!(server_error(client.delete_account("a").await.unwrap_err()));
}
