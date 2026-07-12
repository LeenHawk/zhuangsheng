import { useState } from "react";

import { ApplicationSettings, SettingsSetup } from "@zhuangsheng/domain-ui";

import { useInitialSetup } from "./use-initial-setup";
import { loadUiPreferences, saveUiPreferences } from "./ui-preferences";

export function SettingsRoute() {
  const setup = useInitialSetup();
  const [preferences, setPreferences] = useState(loadUiPreferences);
  const savePreferences = (value: typeof preferences) => { saveUiPreferences(value); setPreferences(value); };
  return <div className="mx-auto max-w-5xl space-y-6 pb-24"><ApplicationSettings value={preferences} onSave={savePreferences} /><SettingsSetup
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
  /></div>;
}
