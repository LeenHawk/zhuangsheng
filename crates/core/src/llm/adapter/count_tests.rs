use serde_json::{Value, json};

use crate::{
    canonical,
    llm::{LlmOperationExecutionPin, Operation, OperationKey, Provider},
};

use super::{ShapeAdapterKey, WireGenerationRequest, decode_count_terminal, encode_count_request};

#[test]
fn count_wire_filters_generation_only_fields_and_decodes() {
    let generation = wire(
        ShapeAdapterKey::OpenAiResponsesV1,
        json!({"model":"model","input":"hello","stream":true,"max_output_tokens":32}),
    );
    let count = encode_count_request(
        &generation,
        OperationKey::provider(Operation::CountTokens, Provider::OpenAi),
    )
    .unwrap();
    assert_eq!(count.relative_path, "/v1/responses/input_tokens");
    assert_eq!(
        serde_json::from_slice::<Value>(count.body()).unwrap(),
        json!({"input":"hello","model":"model"})
    );
    assert_eq!(
        decode_count_terminal(
            &count,
            br#"{"input_tokens":17,"object":"response.input_tokens"}"#,
        )
        .unwrap(),
        17
    );
}

fn wire(key: ShapeAdapterKey, body: Value) -> WireGenerationRequest {
    WireGenerationRequest::from_parts(
        key,
        LlmOperationExecutionPin {
            channel_revision_id: "channel-revision".into(),
            model_id: "model".into(),
            operation_key: OperationKey::content_generation(
                Operation::GenerateContent,
                key.generation_kind(),
            ),
            operation_taxonomy_version: 1,
            adapter_decoder_version: 1,
        },
        gproxy_protocol::HttpMethod::Post,
        "/generation".into(),
        None,
        canonical::to_vec(&body).unwrap(),
    )
}
