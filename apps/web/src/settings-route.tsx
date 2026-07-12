import { SettingsSetup } from "@zhuangsheng/domain-ui";

import { useInitialSetup } from "./use-initial-setup";

export function SettingsRoute() {
  const setup = useInitialSetup();
  return <SettingsSetup
    status={setup.status}
    secrets={setup.secrets}
    channels={setup.channels}
    presets={setup.presets}
    loading={setup.loading}
    pending={setup.pending}
    error={setup.error}
    onReload={setup.reload}
    onStoreSecret={setup.storeSecret}
    onPublishChannel={setup.publishChannel}
    onPublishPreset={setup.publishRolePreset}
  />;
}
