use zhuangsheng_core::application::tool::ToolDescriptorView;

use crate::{CommandResult, TauriAdapter};

impl TauriAdapter {
    pub async fn list_tool_descriptors(&self) -> CommandResult<Vec<ToolDescriptorView>> {
        Ok(self.tool_registry.list_tool_descriptors().await?)
    }
}
