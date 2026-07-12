import { GraphStudio } from "@zhuangsheng/domain-ui";

import { useGraphStudio } from "./use-graph-studio";

export function GraphStudioRoute() {
  const studio = useGraphStudio();
  return <GraphStudio
    graphs={studio.graphs}
    selectedGraphId={studio.selectedGraphId}
    draft={studio.draft}
    jsonText={studio.jsonText}
    projection={studio.projection}
    diagnostics={studio.diagnostics}
    applied={studio.applied}
    dirty={studio.dirty}
    status={studio.status}
    error={studio.error}
    onSelectGraph={studio.selectGraph}
    onCreateGraph={studio.createGraph}
    onJsonChange={studio.changeJson}
    onSave={() => void studio.save()}
    onApply={() => void studio.apply()}
    onReload={studio.reload}
  />;
}
