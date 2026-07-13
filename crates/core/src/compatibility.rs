pub const OPERATION_TAXONOMY_VERSION: u32 = 1;
pub const ADAPTER_DECODER_VERSION: u32 = 1;

pub mod sillytavern;

pub fn supports_operation_versions(taxonomy: u32, decoder: u32) -> bool {
    taxonomy == OPERATION_TAXONOMY_VERSION && decoder == ADAPTER_DECODER_VERSION
}
