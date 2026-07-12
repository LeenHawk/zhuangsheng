use serde_json::json;

use crate::{
    graph::{AppliedGraphDefinition, GraphOutputContractEntry, OutputCollection, RunLimits},
    schema,
};

use super::{
    ASSISTANT_REPLY_PAYLOAD_V1_DOCUMENT_HASH, CONVERSATION_RUN_INPUT_V1_DOCUMENT_HASH,
    ConversationInputShape, ConversationRunSpec, assistant_reply_payload_v1_schema,
    conversation_run_input_v1_schema, validate_conversation_run_contract,
};

#[test]
fn canonical_conversation_contracts_compile_and_match_exactly() {
    let input = conversation_run_input_v1_schema();
    let output = assistant_reply_payload_v1_schema();
    validate(&definition()).unwrap();
    assert_eq!(
        schema::compile(&input).unwrap().canonical_document_hash,
        CONVERSATION_RUN_INPUT_V1_DOCUMENT_HASH
    );
    assert_eq!(
        schema::compile(&output).unwrap().canonical_document_hash,
        ASSISTANT_REPLY_PAYLOAD_V1_DOCUMENT_HASH
    );
}

#[test]
fn compatibility_rejects_shape_cardinality_and_wider_limits() {
    let mut definition = definition();
    definition.output_contract[0].required = false;
    assert!(validate(&definition).is_err());
    definition.output_contract[0].required = true;
    definition.output_contract[0].collection = OutputCollection::Append;
    assert!(validate(&definition).is_err());
    definition.output_contract[0].collection = OutputCollection::Single;
    definition.output_contract[0]
        .schema
        .as_mut()
        .unwrap()
        .limits
        .validation_fuel += 1;
    assert!(validate(&definition).is_err());
    definition.output_contract[0].schema = Some(assistant_reply_payload_v1_schema());
    definition.run_input_schema.as_mut().unwrap().document["properties"]["schemaVersion"] =
        json!({"const":2});
    assert!(validate(&definition).is_err());
}

fn definition() -> AppliedGraphDefinition {
    AppliedGraphDefinition {
        schema_version: 1,
        graph_id: "graph_1".into(),
        operation_taxonomy_version: 1,
        adapter_decoder_version: 1,
        nodes: vec![],
        edges: vec![],
        run_input_schema: Some(conversation_run_input_v1_schema()),
        output_contract: vec![GraphOutputContractEntry {
            key: "reply".into(),
            schema: Some(assistant_reply_payload_v1_schema()),
            collection: OutputCollection::Single,
            required: true,
        }],
        limits: RunLimits::default(),
        schema_semantics: vec![],
    }
}

fn validate(definition: &AppliedGraphDefinition) -> Result<(), &'static str> {
    validate_conversation_run_contract(
        definition,
        &ConversationRunSpec {
            graph_revision_id: "graphrev_1".into(),
            reply_output_key: "reply".into(),
            input_shape: ConversationInputShape::ConversationMessageV1,
        },
    )
}
