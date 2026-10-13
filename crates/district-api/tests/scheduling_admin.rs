//! The booking scheduler's administration: what an operation sends, how each
//! shape of answer is read, and the image upload.

mod common;

use std::fs;
use std::path::PathBuf;

use common::client;
use district_api::{ApiError, ERROR_CODE_HEADER, Endpoint};
use district_model::{
    CODE_INVALID_PARAMS, CODE_SCHEDULING_NOT_READY, SchedulingAdminData, SchedulingAdminReply,
    SchedulingAdminRequest, SchedulingUpload, SchedulingUploadTarget,
};
use serde_json::{Value, json};
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const WORKSPACE: &str = "ws-sched";

fn fixture(name: &str) -> Value {
    let file = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../contracts/fixtures")
        .join(name);
    let text = fs::read_to_string(&file)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", file.display()));
    serde_json::from_str(&text).expect("a fixture is JSON")
}

async fn answering(route: &str, response: ResponseTemplate) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(route))
        .respond_with(response)
        .mount(&server)
        .await;
    server
}

const ADMIN: &str = "/api/district/scheduling/admin";
const UPLOAD: &str = "/api/district/scheduling/admin/upload";

fn cancel() -> SchedulingAdminRequest {
    SchedulingAdminRequest::CancelBooking {
        id: "bk-1".into(),
        reason: None,
    }
}

#[tokio::test]
async fn an_operation_sends_its_name_and_parameters_and_reads_its_answer() {
    let server = answering(
        ADMIN,
        ResponseTemplate::new(200).set_body_json(fixture("district-scheduling-booking.json")),
    )
    .await;

    let reply = client(&server)
        .scheduling_admin(WORKSPACE, &cancel())
        .await
        .unwrap();

    let SchedulingAdminReply::Done(SchedulingAdminData::Booking(booking)) = reply else {
        panic!("{reply:?}")
    };
    assert_eq!(booking.status, "cancelled");
    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 1);
    let sent: Value = serde_json::from_slice(&requests[0].body).unwrap();
    assert_eq!(
        sent,
        json!({"workspaceId": WORKSPACE, "op": "bookings.cancel", "params": {"id": "bk-1"}})
    );
    assert_eq!(requests[0].url.query(), None);
}

#[tokio::test]
async fn an_operation_that_takes_nothing_sends_an_empty_object() {
    let server = answering(
        ADMIN,
        ResponseTemplate::new(200).set_body_json(fixture("district-scheduling-me.json")),
    )
    .await;
    let reply = client(&server)
        .scheduling_admin(WORKSPACE, &SchedulingAdminRequest::Me)
        .await
        .unwrap();
    assert!(matches!(
        reply,
        SchedulingAdminReply::Done(SchedulingAdminData::Me(_))
    ));
    let sent: Value =
        serde_json::from_slice(&server.received_requests().await.unwrap()[0].body).unwrap();
    assert_eq!(sent["params"], json!({}));
}

#[tokio::test]
async fn the_schedulers_refusal_is_an_answer_with_a_200() {
    let server = answering(
        ADMIN,
        ResponseTemplate::new(200).set_body_json(fixture("district-scheduling-admin-failure.json")),
    )
    .await;
    let reply = client(&server)
        .scheduling_admin(WORKSPACE, &cancel())
        .await
        .unwrap();
    let SchedulingAdminReply::Refused(failure) = reply else {
        panic!("{reply:?}")
    };
    assert_eq!(
        (failure.failure.as_str(), failure.status),
        ("rejected", 403)
    );
}

#[tokio::test]
async fn the_routes_own_refusals_are_errors_by_code() {
    let not_ready = answering(
        ADMIN,
        ResponseTemplate::new(409)
            .insert_header(ERROR_CODE_HEADER, CODE_SCHEDULING_NOT_READY)
            .set_body_json(fixture("district-scheduling-admin-not-ready.json")),
    )
    .await;
    let error = client(&not_ready)
        .scheduling_admin(WORKSPACE, &SchedulingAdminRequest::Me)
        .await
        .unwrap_err();
    assert_eq!(error.code(), Some(CODE_SCHEDULING_NOT_READY), "{error:?}");

    let invalid = answering(
        ADMIN,
        ResponseTemplate::new(400)
            .insert_header(ERROR_CODE_HEADER, CODE_INVALID_PARAMS)
            .set_body_json(fixture("district-scheduling-admin-invalid-params.json")),
    )
    .await;
    let error = client(&invalid)
        .scheduling_admin(WORKSPACE, &SchedulingAdminRequest::Me)
        .await
        .unwrap_err();
    assert_eq!(error.code(), Some(CODE_INVALID_PARAMS), "{error:?}");

    let forbidden = answering(
        ADMIN,
        ResponseTemplate::new(403)
            .set_body_json(json!({"error": "forbidden", "code": "forbidden"})),
    )
    .await;
    let error = client(&forbidden)
        .scheduling_admin(WORKSPACE, &cancel())
        .await
        .unwrap_err();
    assert!(matches!(error, ApiError::Forbidden(_)), "{error:?}");
}

#[tokio::test]
async fn an_answer_of_another_shape_is_a_decode_error_that_quotes_nothing() {
    for body in [
        json!({"ok": true, "data": {"items": "not a list"}}),
        json!({"data": {"id": "bk-1"}}),
        json!({"ok": false, "failure": 3}),
    ] {
        let server = answering(
            ADMIN,
            ResponseTemplate::new(200).set_body_json(body.clone()),
        )
        .await;
        let error = client(&server)
            .scheduling_admin(WORKSPACE, &SchedulingAdminRequest::EventTypes)
            .await
            .unwrap_err();
        let ApiError::Decode { endpoint, .. } = &error else {
            panic!("{body}: {error:?}")
        };
        assert_eq!(*endpoint, Endpoint::SchedulingAdmin);
        assert!(!error.to_string().contains("not a list"), "{error}");
    }
}

#[tokio::test]
async fn an_image_goes_to_the_upload_route_with_its_target() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(UPLOAD))
        .and(query_param("workspaceId", WORKSPACE))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(fixture("district-scheduling-upload.json")),
        )
        .mount(&server)
        .await;
    let upload = SchedulingAdminRequest::Upload(SchedulingUpload {
        target: SchedulingUploadTarget::Logo,
        file_name: "logo.png".into(),
        mime_type: "image/png".into(),
        bytes: vec![0x89, b'P', b'N', b'G'],
    });

    let reply = client(&server)
        .scheduling_admin(WORKSPACE, &upload)
        .await
        .unwrap();

    let SchedulingAdminReply::Done(SchedulingAdminData::Uploaded(result)) = reply else {
        panic!("{reply:?}")
    };
    assert_eq!(
        result.published_url(),
        Some("https://book.example.com/media/branding/logo.png")
    );
    let request = &server.received_requests().await.unwrap()[0];
    let body = &request.body;
    let target = b"name=\"target\"\r\n\r\nlogo\r\n";
    assert!(
        body.windows(target.len()).any(|w| w == target),
        "the target field"
    );
    let file = b"filename=\"logo.png\"\r\nContent-Type: image/png\r\n\r\n\x89PNG\r\n";
    assert!(body.windows(file.len()).any(|w| w == file), "the file");
    assert!(
        !body
            .windows(b"workspaceId".len())
            .any(|w| w == b"workspaceId"),
        "the workspace is in the query, not the form"
    );
}

#[tokio::test]
async fn an_upload_answered_with_another_shape_names_the_upload_route() {
    let server = answering(
        UPLOAD,
        ResponseTemplate::new(200).set_body_json(json!({"ok": true, "data": "text"})),
    )
    .await;
    let upload = SchedulingAdminRequest::Upload(SchedulingUpload {
        target: SchedulingUploadTarget::Avatar,
        file_name: "me.webp".into(),
        mime_type: "image/webp".into(),
        bytes: vec![1],
    });
    let error = client(&server)
        .scheduling_admin(WORKSPACE, &upload)
        .await
        .unwrap_err();
    assert!(
        matches!(
            error,
            ApiError::Decode {
                endpoint: Endpoint::SchedulingAdminUpload,
                ..
            }
        ),
        "{error:?}"
    );
}

#[tokio::test]
async fn a_form_field_is_refused_on_a_json_body() {
    let server = MockServer::start().await;
    let result: Result<Value, ApiError> = client(&server)
        .request(Endpoint::SchedulingAdmin)
        .workspace(WORKSPACE)
        .form_field("target", "logo")
        .send()
        .await;
    let Err(ApiError::InvalidRequest(message)) = result else {
        panic!("{result:?}")
    };
    assert!(message.contains("takes no form fields"), "{message}");
}

const SIGN_IN: &str = "/api/district/scheduling/sso";
const NEXT: &str =
    "/v1/calendar/connect?provider=google&return_to=https%3A%2F%2Fwww.example.com%2Fx";

async fn signing_in(response: ResponseTemplate) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(SIGN_IN))
        .and(query_param("workspaceId", WORKSPACE))
        .and(query_param("next", NEXT))
        .respond_with(response)
        .mount(&server)
        .await;
    server
}

#[tokio::test]
async fn the_scheduler_sign_in_is_its_redirects_whole_location_never_followed() {
    let link = "https://book.example.com/v1/auth/sso?token=single-use&next=%2Fv1%2Fcalendar";
    let server = signing_in(ResponseTemplate::new(302).insert_header("Location", link)).await;

    let answer = client(&server)
        .scheduling_sign_in(WORKSPACE, NEXT)
        .await
        .unwrap();

    assert_eq!(answer.expose(), link, "the query, token and all, is kept");
    assert!(!format!("{answer:?}").contains("single-use"));
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

#[tokio::test]
async fn a_sign_in_that_does_not_redirect_somewhere_is_unconfirmed() {
    for response in [
        ResponseTemplate::new(200).set_body_json(json!({"ok": true})),
        ResponseTemplate::new(302),
    ] {
        let server = signing_in(response).await;
        let error = client(&server)
            .scheduling_sign_in(WORKSPACE, NEXT)
            .await
            .unwrap_err();
        assert!(
            matches!(
                error,
                ApiError::Unconfirmed {
                    endpoint: Endpoint::SchedulingSignIn
                }
            ),
            "{error:?}"
        );
    }
    let not_ready = signing_in(
        ResponseTemplate::new(409).set_body_json(json!({"error": "Scheduling is not ready"})),
    )
    .await;
    let error = client(&not_ready)
        .scheduling_sign_in(WORKSPACE, NEXT)
        .await
        .unwrap_err();
    assert!(matches!(error, ApiError::Conflict(_)), "{error:?}");
}
