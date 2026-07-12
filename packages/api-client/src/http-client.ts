import { HttpGraphClient } from "./http-graph-client";
import { HttpConfigClient } from "./http-config-client";
import { HttpRuntimeClient } from "./http-runtime-client";
import { HttpMemoryClient } from "./http-memory-client";
import { HttpArtifactClient } from "./http-artifact-client";
import { HttpSecretClient } from "./http-secret-client";
import { HttpContextClient } from "./http-context-client";
import { HttpToolClient } from "./http-tool-client";
import { HttpConversationClient } from "./http-conversation-client";

export class HttpApiClient extends HttpConversationClient {
  readonly runtime: HttpRuntimeClient;
  readonly secrets: HttpSecretClient;
  readonly graphs: HttpGraphClient;
  readonly config: HttpConfigClient;
  readonly memory: HttpMemoryClient;
  readonly artifacts: HttpArtifactClient;
  readonly contexts: HttpContextClient;
  readonly tools: HttpToolClient;

  constructor(baseUrl = "") {
    super(baseUrl);
    this.runtime = new HttpRuntimeClient(baseUrl);
    this.secrets = new HttpSecretClient(baseUrl);
    this.graphs = new HttpGraphClient(baseUrl);
    this.config = new HttpConfigClient(baseUrl);
    this.memory = new HttpMemoryClient(baseUrl);
    this.artifacts = new HttpArtifactClient(baseUrl);
    this.contexts = new HttpContextClient(baseUrl);
    this.tools = new HttpToolClient(baseUrl);
  }
}
