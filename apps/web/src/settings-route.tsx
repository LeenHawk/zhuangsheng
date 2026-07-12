import { SettingsSetup } from "@zhuangsheng/domain-ui";

import { useInitialSetup } from "./use-initial-setup";

export function SettingsRoute() {
  const setup = useInitialSetup();
  return <SettingsSetup
    status={setup.status}
    secrets={setup.secrets}
    channels={setup.channels}
    presets={setup.presets}
    templates={setup.templates}
    preview={setup.preview}
    discovery={setup.discovery}
    rolePlaySettings={setup.rolePlaySettings}
    loading={setup.loading}
    pending={setup.pending}
    error={setup.error}
    onReload={setup.reload}
    onStoreSecret={setup.storeSecret}
    onPublishChannel={setup.publishChannel}
    onPublishPreset={setup.publishRolePreset}
    onPreviewPreset={setup.previewPreset}
    onCreateTemplate={setup.createTemplate}
    onDiscoverModels={setup.discoverModels}
    onPublishDiscoveredModel={setup.publishDiscoveredModel}
    onInspectTemplate={setup.inspectTemplate}
  />;
}
