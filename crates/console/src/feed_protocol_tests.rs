//! A GraphQL send is listed in the Console as protocol `graphql` (REQ-GQL-01), though it
//! goes on the wire as HTTP.

use api_model::SecretValues;
use gpui::TestAppContext;
use http_engine::graphql::PROTOCOL;
use http_engine::{
    AppliedAuth, PipelineError, RequestBody, ResolvedRequest, Response, ResponseBody, SendOutcome,
    SendSettings, TransportError,
};

use crate::{ConsoleStore, record_error_as, record_outcome_as};

fn graphql_outcome() -> SendOutcome {
    SendOutcome {
        request: ResolvedRequest::new(
            api_model::Method::Post,
            "https://example.test/graphql",
            Vec::new(),
            RequestBody::Bytes(br#"{"query":"{ me { id } }"}"#.to_vec()),
            SendSettings::default(),
        )
        .unwrap(),
        response: Response::new(200, "OK", ResponseBody::Bytes(br#"{"data":{}}"#.to_vec())),
        warnings: Vec::new(),
        auth: AppliedAuth::None,
        secrets_used: SecretValues::new(),
    }
}

fn init(cx: &mut TestAppContext) {
    cx.update(|cx| {
        assert_eq!(settings::SettingsStore::init(cx, None), Ok(()));
        crate::init(cx);
    });
}

#[gpui::test]
fn console_records_graphql_send_and_failure_as_protocol_graphql(cx: &mut TestAppContext) {
    init(cx);

    let protocols = cx.update(|cx| {
        record_outcome_as(PROTOCOL, &graphql_outcome(), cx);
        record_error_as(
            PROTOCOL,
            None,
            &PipelineError::Transport(TransportError::Cancelled),
            cx,
        );
        cx.global::<ConsoleStore>()
            .entries()
            .iter()
            .map(|entry| entry.protocol.clone())
            .collect::<Vec<_>>()
    });

    assert_eq!(
        protocols,
        vec!["graphql".to_string(), "graphql".to_string()]
    );
}
