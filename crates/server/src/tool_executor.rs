use std::{collections::BTreeMap, sync::Arc};

use async_trait::async_trait;
use zhuangsheng_core::{
    application::tool::{
        ToolCallOutput, ToolExecutionContext, ToolExecutionError, ToolExecutor, ToolOutputPart,
    },
    llm::ir::LlmContentPartIr,
};

pub(crate) const BUILTIN_ECHO_EXECUTOR_KEY: &str = "builtin.echo";
pub(crate) const BUILTIN_ECHO_IMPLEMENTATION_DIGEST: &str =
    "sha256:97ffa07077c35fc664217667cb97da252ad68d3d20263de984744c8442a5648a";

type ExecutorIdentity = (String, String);

#[derive(Clone, Default)]
pub(crate) struct ToolExecutorRegistry {
    executors: BTreeMap<ExecutorIdentity, Arc<dyn ToolExecutor>>,
}

impl ToolExecutorRegistry {
    pub(crate) fn with_builtins() -> Self {
        let mut registry = Self::default();
        registry.register(
            BUILTIN_ECHO_EXECUTOR_KEY,
            BUILTIN_ECHO_IMPLEMENTATION_DIGEST,
            Arc::new(EchoToolExecutor),
        );
        registry
    }

    pub(crate) fn register(
        &mut self,
        executor_key: impl Into<String>,
        implementation_digest: impl Into<String>,
        executor: Arc<dyn ToolExecutor>,
    ) {
        self.executors.insert(
            (executor_key.into(), implementation_digest.into()),
            executor,
        );
    }

    pub(crate) fn resolve(
        &self,
        executor_key: &str,
        implementation_digest: &str,
    ) -> Option<Arc<dyn ToolExecutor>> {
        self.executors
            .get(&(executor_key.to_owned(), implementation_digest.to_owned()))
            .cloned()
    }
}

struct EchoToolExecutor;

#[async_trait]
impl ToolExecutor for EchoToolExecutor {
    async fn execute(
        &self,
        context: ToolExecutionContext,
    ) -> Result<ToolCallOutput, ToolExecutionError> {
        let text = context
            .invocation
            .arguments
            .get("text")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| ToolExecutionError {
                code: "echo_text_missing".into(),
                safe_message: "echo input is missing text".into(),
                retryable: false,
                outcome_unknown: false,
            })?;
        Ok(ToolCallOutput {
            parts: vec![ToolOutputPart::LlmResult {
                content: vec![LlmContentPartIr::Text { text: text.into() }],
            }],
        })
    }
}
