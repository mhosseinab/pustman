use crate::{
    Auth, DescriptorSource, GraphQlRequest, GrpcRequest, ItemId, KeyValue, MessageKind, Request,
    RequestSettings, SavedEmit, SavedMessage, SocketIoRequest, WebSocketRequest,
};
use std::collections::BTreeMap;

fn header(key: &str, value: &str) -> KeyValue {
    KeyValue {
        key: key.to_string(),
        value: Some(value.to_string()),
        description: None,
        enabled: true,
        extra: BTreeMap::new(),
    }
}

/// Serialize a request, check its `type` tag, decode it back and return the decoded value.
fn round_trip(request: &Request, tag: &str) -> Request {
    let json = serde_json::to_value(request).unwrap();
    assert_eq!(json["type"], tag, "the request carries its kind tag");
    let bytes = serde_json::to_vec(&json).unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

#[test]
fn graphql_request_round_trips_json() {
    let request = Request::GraphQl(GraphQlRequest {
        id: ItemId::new(),
        name: "Search users".to_string(),
        url: "https://api.example.test/graphql".to_string(),
        headers: vec![header("X-Trace", "on")],
        auth: Auth::Bearer {
            token: Some("{{token}}".to_string()),
            extra: BTreeMap::new(),
        },
        query: "query Search($q: String) { users(q: $q) { id } }".to_string(),
        // Invalid JSON on purpose: a half-typed variables editor must survive a save.
        variables: "{\"q\": ".to_string(),
        operation_name: Some("Search".to_string()),
        settings: Some(RequestSettings {
            timeout_ms: Some(5_000),
            ..RequestSettings::default()
        }),
        extra: BTreeMap::new(),
    });

    let decoded = round_trip(&request, "graphql");
    assert_eq!(decoded, request);
    let Request::GraphQl(graphql) = &decoded else {
        panic!("a graphql request must decode typed");
    };
    assert_eq!(graphql.variables, "{\"q\": ", "raw variables text survives");
}

#[test]
fn websocket_request_round_trips_with_saved_messages() {
    let request = Request::WebSocket(WebSocketRequest {
        id: ItemId::new(),
        name: "Chat".to_string(),
        url: "wss://echo.example.test/ws".to_string(),
        headers: vec![header("Origin", "https://example.test")],
        params: vec![header("room", "1")],
        subprotocols: vec!["graphql-transport-ws".to_string(), "chat".to_string()],
        auth: Auth::None,
        settings: None,
        saved_messages: vec![
            SavedMessage {
                name: "Hello".to_string(),
                kind: MessageKind::Text,
                payload: "hello".to_string(),
                extra: BTreeMap::new(),
            },
            SavedMessage {
                name: "Join".to_string(),
                kind: MessageKind::Json,
                payload: "{\"join\":\"room-1\"}".to_string(),
                extra: BTreeMap::new(),
            },
            SavedMessage {
                name: "Ping bytes".to_string(),
                kind: MessageKind::Binary,
                payload: "AAEC".to_string(),
                extra: BTreeMap::new(),
            },
        ],
        extra: BTreeMap::new(),
    });

    let decoded = round_trip(&request, "websocket");
    assert_eq!(decoded, request);
    let Request::WebSocket(websocket) = &decoded else {
        panic!("a websocket request must decode typed");
    };
    let kinds: Vec<MessageKind> = websocket
        .saved_messages
        .iter()
        .map(|message| message.kind)
        .collect();
    assert_eq!(
        kinds,
        vec![MessageKind::Text, MessageKind::Json, MessageKind::Binary]
    );
}

#[test]
fn socketio_request_defaults_path_and_namespace() {
    let input = r#"{
        "type": "socketio",
        "id": "01BX5ZZKBKACTAV9WEVGEMMVRZ",
        "name": "Live scores",
        "url": "https://scores.example.test"
    }"#;

    let request: Request = serde_json::from_slice(input.as_bytes()).unwrap();
    let Request::SocketIo(socketio) = &request else {
        panic!("a socketio request must decode typed");
    };
    assert_eq!(socketio.path, "/socket.io");
    assert_eq!(socketio.namespace, "/");
    assert!(socketio.all_events, "all-events mode is on by default");
    assert!(socketio.listen.is_empty());
    assert!(socketio.saved_emits.is_empty());

    let full = Request::SocketIo(SocketIoRequest {
        path: "/ws".to_string(),
        namespace: "/admin".to_string(),
        headers: vec![header("X-Client", "pustman")],
        auth_payload: "{\"token\":\"{{token}}\"}".to_string(),
        handshake_timeout_ms: Some(3_000),
        listen: vec!["score".to_string()],
        all_events: false,
        saved_emits: vec![SavedEmit {
            name: "Subscribe".to_string(),
            event: "subscribe".to_string(),
            arguments: vec![SavedMessage {
                name: "match".to_string(),
                kind: MessageKind::Json,
                payload: "{\"match\":7}".to_string(),
                extra: BTreeMap::new(),
            }],
            acknowledge: true,
            extra: BTreeMap::new(),
        }],
        ..socketio.clone()
    });
    assert_eq!(round_trip(&full, "socketio"), full);
}

#[test]
fn grpc_request_round_trips_each_descriptor_source() {
    let sources = [
        DescriptorSource::Reflection {
            extra: BTreeMap::new(),
        },
        DescriptorSource::ProtoFiles {
            files: vec!["protos/helloworld.proto".to_string()],
            import_paths: vec!["protos".to_string()],
            extra: BTreeMap::new(),
        },
        DescriptorSource::DescriptorSet {
            path: "protos/greeter.pb".to_string(),
            extra: BTreeMap::new(),
        },
    ];

    for source in sources {
        let request = Request::Grpc(GrpcRequest {
            id: ItemId::new(),
            name: "Say hello".to_string(),
            target: "localhost:50051".to_string(),
            service: "helloworld.Greeter".to_string(),
            method: "SayHello".to_string(),
            message: "{\"name\":\"world\"}".to_string(),
            metadata: vec![header("x-request-id", "1")],
            auth: Auth::Inherit,
            deadline_ms: Some(2_000),
            tls: false,
            descriptor_source: source.clone(),
            extra: BTreeMap::new(),
        });

        let decoded = round_trip(&request, "grpc");
        assert_eq!(decoded, request, "{source:?} round-trips");
    }
}

#[test]
fn mqtt_kind_still_preserved_as_unsupported() {
    let input = r#"{
        "type": "mqtt",
        "id": "01BX5ZZKBKACTAV9WEVGEMMVRZ",
        "name": "Sensors",
        "url": "mqtt://broker.example.test:1883",
        "topic": "sensors/+/temperature"
    }"#;

    let request: Request = serde_json::from_slice(input.as_bytes()).unwrap();
    let Request::Unsupported(raw) = &request else {
        panic!("an MQTT request is not modelled yet and must be kept raw");
    };
    assert_eq!(raw["topic"], "sensors/+/temperature");

    let output = serde_json::to_value(&request).unwrap();
    let original: serde_json::Value = serde_json::from_slice(input.as_bytes()).unwrap();
    assert_eq!(output, original, "an MQTT request round-trips verbatim");
}

#[test]
fn nested_unknown_keys_survive_a_save() {
    let websocket = r#"{
        "type": "websocket",
        "id": "01BX5ZZKBKACTAV9WEVGEMMVRZ",
        "name": "Chat",
        "url": "wss://echo.example.test/ws",
        "saved_messages": [
            {"name": "Ping", "kind": "binary", "payload": "AAEC", "encoding": "base64"}
        ]
    }"#;
    let socketio = r#"{
        "type": "socketio",
        "id": "01BX5ZZKBKACTAV9WEVGEMMVRZ",
        "name": "Live",
        "url": "https://scores.example.test",
        "saved_emits": [
            {
                "name": "Join",
                "event": "join",
                "volatile": true,
                "arguments": [{"name": "room", "kind": "text", "payload": "1", "encoding": "utf8"}]
            }
        ]
    }"#;
    let grpc_sources = [
        r#"{"kind": "reflection", "include_well_known": true}"#,
        r#"{"kind": "proto_files", "files": ["a.proto"], "include_well_known": true}"#,
        r#"{"kind": "descriptor_set", "path": "a.pb", "include_well_known": true}"#,
    ];

    let mut inputs = vec![websocket.to_string(), socketio.to_string()];
    for source in grpc_sources {
        inputs.push(format!(
            r#"{{"type": "grpc", "id": "01BX5ZZKBKACTAV9WEVGEMMVRZ", "name": "Hello",
                "target": "localhost:50051", "descriptor_source": {source}}}"#
        ));
    }

    for input in inputs {
        let request: Request = serde_json::from_slice(input.as_bytes()).unwrap();
        assert!(
            !matches!(request, Request::Unsupported(_)),
            "{input} must decode typed"
        );
        let saved = serde_json::to_vec(&request).unwrap();
        let reopened: Request = serde_json::from_slice(&saved).unwrap();
        assert_eq!(reopened, request, "{input} round-trips");

        let written: serde_json::Value = serde_json::from_slice(&saved).unwrap();
        let nested = match reopened.type_tag() {
            "websocket" => &written["saved_messages"][0]["encoding"],
            "socketio" => {
                assert_eq!(written["saved_emits"][0]["volatile"], true);
                &written["saved_emits"][0]["arguments"][0]["encoding"]
            }
            _ => &written["descriptor_source"]["include_well_known"],
        };
        assert!(
            !nested.is_null(),
            "{input}: a nested unknown key must survive a save"
        );
    }
}

#[test]
fn minimal_grpc_request_defaults_to_tls_and_reflection() {
    let input = r#"{
        "type": "grpc",
        "id": "01BX5ZZKBKACTAV9WEVGEMMVRZ",
        "name": "Say hello",
        "target": "greeter.example.test:443"
    }"#;

    let request: Request = serde_json::from_slice(input.as_bytes()).unwrap();
    let Request::Grpc(grpc) = &request else {
        panic!("a minimal grpc request must decode typed");
    };
    assert!(grpc.tls, "TLS is on unless a request opts out");
    assert_eq!(
        grpc.descriptor_source,
        DescriptorSource::Reflection {
            extra: BTreeMap::new()
        }
    );
}

#[test]
fn unknown_top_level_keys_round_trip_for_each_kind() {
    let inputs = [
        r#"{"type": "graphql", "id": "01BX5ZZKBKACTAV9WEVGEMMVRZ", "name": "Q",
            "url": "https://api.example.test/graphql", "query": "{ me }", "x_future": {"a": 1}}"#,
        r#"{"type": "websocket", "id": "01BX5ZZKBKACTAV9WEVGEMMVRZ", "name": "W",
            "url": "wss://example.test", "x_future": {"a": 1}}"#,
        r#"{"type": "socketio", "id": "01BX5ZZKBKACTAV9WEVGEMMVRZ", "name": "S",
            "url": "https://example.test", "settings": {"timeout_ms": 5}, "x_future": {"a": 1}}"#,
        r#"{"type": "grpc", "id": "01BX5ZZKBKACTAV9WEVGEMMVRZ", "name": "G",
            "target": "localhost:50051", "settings": {"timeout_ms": 5}, "x_future": {"a": 1}}"#,
    ];

    for input in inputs {
        let request: Request = serde_json::from_slice(input.as_bytes()).unwrap();
        assert!(
            !matches!(request, Request::Unsupported(_)),
            "{input} must decode typed"
        );
        let written = serde_json::to_value(&request).unwrap();
        assert_eq!(written["x_future"], serde_json::json!({"a": 1}), "{input}");
        let original: serde_json::Value = serde_json::from_slice(input.as_bytes()).unwrap();
        if original.get("settings").is_some() {
            assert_eq!(
                written["settings"], original["settings"],
                "{input}: a settings block the kind does not model is kept as an unknown key"
            );
        }
    }
}

#[test]
fn a_known_kind_that_fails_to_decode_is_kept_verbatim() {
    let input = r#"{"type":"grpc","id":"01BX5ZZKBKACTAV9WEVGEMMVRZ","name":"G","target":"localhost:50051","tls":"no"}"#;

    let request: Request = serde_json::from_slice(input.as_bytes()).unwrap();
    assert!(
        matches!(request, Request::Unsupported(_)),
        "an undecodable grpc request must be kept raw"
    );
    let output = serde_json::to_string(&request).unwrap();
    assert_eq!(output, input, "it round-trips byte for byte");
}
