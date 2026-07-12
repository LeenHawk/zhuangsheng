use serde_json::{Value, json};

use crate::{
    graph::{AppliedGraphDefinition, OutputCollection},
    schema::{DIALECT_2020_12, JsonSchemaLimits, JsonSchemaSpec, compile},
};

use super::ConversationRunSpec;

pub const CONVERSATION_RUN_INPUT_V1_DOCUMENT_HASH: &str =
    "sha256:83957b98515709421dd4b3c2b388032cc6f14546635512b83601ed995ff13bfc";
pub const ASSISTANT_REPLY_PAYLOAD_V1_DOCUMENT_HASH: &str =
    "sha256:64e3f242a84362910d87ae9ba004a971ab3f59c33886980faec80f4a79c4695d";

pub fn assistant_reply_payload_v1_schema() -> JsonSchemaSpec {
    contract_schema(json!({
        "$defs": content_defs(),
        "type":"object",
        "properties":{
            "schemaVersion":{"const":1},
            "type":{"const":"assistant_reply"},
            "content":{"type":"array","items":{"$ref":"#/$defs/contentPart"},"minItems":1,"maxItems":256}
        },
        "required":["schemaVersion","type","content"],
        "additionalProperties":false
    }))
}

pub fn conversation_run_input_v1_schema() -> JsonSchemaSpec {
    contract_schema(json!({
        "$defs": content_defs(),
        "type":"object",
        "properties":{
            "schemaVersion":{"const":1},
            "conversationId":{"type":"string","minLength":1,"maxLength":128},
            "turnId":{"type":"string","minLength":1,"maxLength":128},
            "userMessageId":{"type":"string","minLength":1,"maxLength":128},
            "userCommitId":{"type":"string","minLength":1,"maxLength":128},
            "content":{"type":"array","items":{"$ref":"#/$defs/contentPart"},"minItems":1,"maxItems":256}
        },
        "required":["schemaVersion","conversationId","turnId","userMessageId","userCommitId","content"],
        "additionalProperties":false
    }))
}

pub fn validate_conversation_run_contract(
    definition: &AppliedGraphDefinition,
    run: &ConversationRunSpec,
) -> Result<(), &'static str> {
    run.validate()?;
    let expected_input = conversation_run_input_v1_schema();
    let input = definition
        .run_input_schema
        .as_ref()
        .ok_or("conversation graph requires an input schema")?;
    exact_contract(
        input,
        &expected_input,
        CONVERSATION_RUN_INPUT_V1_DOCUMENT_HASH,
    )
    .map_err(|_| "graph input schema is not conversation_message_v1 compatible")?;
    let output = definition
        .output_contract
        .iter()
        .find(|entry| entry.key == run.reply_output_key)
        .ok_or("conversation reply output does not exist")?;
    if !output.required || output.collection != OutputCollection::Single {
        return Err("conversation reply output must be required and single");
    }
    let output_schema = output
        .schema
        .as_ref()
        .ok_or("conversation reply output requires a schema")?;
    exact_contract(
        output_schema,
        &assistant_reply_payload_v1_schema(),
        ASSISTANT_REPLY_PAYLOAD_V1_DOCUMENT_HASH,
    )
    .map_err(|_| "graph reply schema is not AssistantReplyPayloadV1 compatible")
}

fn exact_contract(
    owner: &JsonSchemaSpec,
    contract: &JsonSchemaSpec,
    expected_document_hash: &str,
) -> Result<(), ()> {
    if !owner.limits.fits_within(&contract.limits) {
        return Err(());
    }
    let owner = compile(owner).map_err(|_| ())?;
    let contract = compile(contract).map_err(|_| ())?;
    (contract.canonical_document_hash == expected_document_hash
        && owner.canonical_document_hash == expected_document_hash)
        .then_some(())
        .ok_or(())
}

fn contract_schema(document: Value) -> JsonSchemaSpec {
    JsonSchemaSpec {
        schema_version: 1,
        dialect: DIALECT_2020_12.into(),
        validation_profile_version: 1,
        format_policy_version: 1,
        document,
        limits: contract_limits(),
    }
}

fn contract_limits() -> JsonSchemaLimits {
    JsonSchemaLimits {
        max_schema_bytes: 256 * 1024,
        max_schema_nodes: 4096,
        max_schema_depth: 128,
        max_local_refs: 1024,
        max_ref_depth: 64,
        max_regex_bytes: 4096,
        max_instance_bytes: 16 * 1024 * 1024,
        max_instance_depth: 128,
        max_collection_items: 100_000,
        max_string_bytes: 8 * 1024 * 1024,
        max_number_digits: 128,
        max_number_exponent_magnitude: 1024,
        max_validation_errors: 32,
        validation_fuel: 1_000_000,
    }
}

fn content_defs() -> Value {
    json!({
        "artifactRef":{
            "type":"object",
            "properties":{
                "artifactId":{"type":"string","minLength":1,"maxLength":128},
                "contentHash":{"type":"string","pattern":"^sha256:[0-9a-f]{64}$"},
                "byteSize":{"type":"integer","minimum":1,"maximum":1073741824},
                "mediaType":{"type":"string","minLength":3,"maxLength":128,"pattern":"^[\\x20-\\x7e]+/[\\x20-\\x7e]+$"}
            },
            "required":["artifactId","contentHash","byteSize","mediaType"],
            "additionalProperties":false
        },
        "contentPart":{
            "oneOf":[
                {"type":"object","properties":{"type":{"const":"text"},"text":{"type":"string","minLength":1,"maxLength":1048576}},"required":["type","text"],"additionalProperties":false},
                {"type":"object","properties":{"type":{"const":"image"},"artifactRef":{"$ref":"#/$defs/artifactRef"}},"required":["type","artifactRef"],"additionalProperties":false},
                {"type":"object","properties":{"type":{"const":"file"},"artifactRef":{"$ref":"#/$defs/artifactRef"}},"required":["type","artifactRef"],"additionalProperties":false}
            ]
        }
    })
}
