export type Appearance = 'light' | 'dark';
export type IdentityAsset = { id: string; name: string; mime: string; dataUri: string; sha256: string };
export type ImportedIdentity = { sourceName: string; sourceSha256: string; summary: string; assets: IdentityAsset[] };
export type IdentitySource = { id: string; name: string; sha256: string; files: { id: string; name: string; textBytes: number; asset: IdentityAsset | null }[]; skipped: string[] };
export type IdentityCandidate = { jobId: string; requirementsSha256: string; summary: string; brandName: string; tone: string; findings: { topic: string; value: string; evidence: string[]; confidence: 'high' | 'medium' | 'low' }[]; logoAssetIds: string[]; warnings: string[] };
export type IdentityAnalysis = { jobId: string; sourceId: string; source: IdentitySource; fileIds: string[]; authentication?: AuthenticationMethod | null; chatgptAccountId?: string | null; status: 'analyzing' | 'complete' | 'failed' | 'unknown' | 'cancelled'; message: string; receivedBytes: number; candidate: IdentityCandidate | null; error: string | null };
export type NewProjectIdentity = { identityAnalysisId: string; identityFindingIndices: number[]; identityAssetIds: string[] };
export type GenerationMode = 'fuzzy' | 'strict';
export type Viewport = { width: number; height: number };
export type GridSettings = { spacing: number; unit: 'pixels' | 'percent'; snap: boolean; rulers: boolean };
export type Rect = { x: number; y: number; width: number; height: number };
export type ElementTarget = { selector: string; tag: string; label: string; sourceSha256: string };
export type LayerDirection = 'forward' | 'backward' | 'front' | 'back';
export type LayerPseudoStyle = { pseudo: 'before' | 'after' | 'marker'; style: string };
export type ElementStyleEdit = { target: ElementTarget; style: string; pseudoStyles?: LayerPseudoStyle[] };
export type LayerTarget = { kind: 'request'; regionId: string } | { kind: 'element'; target: ElementTarget };
export type LayerPasteView = { workspace: WorkspaceView; selection: { element: ElementTarget | null; regionId: string | null } };
export type ElementSelection = { element: ElementTarget; rect: Rect };
export type ElementEdit = { kind: 'update'; texts: { index: number; text: string }[]; attributes: Record<string, string | null>; style: string | null }
  | { kind: 'replace'; html: string; keepStyle: boolean } | { kind: 'reorder'; direction: 'before' | 'after' } | { kind: 'layerStyles'; styles: ElementStyleEdit[] } | { kind: 'remove' };
export type ElementBounds = { regionId: string; target: ElementTarget; rect: Rect };
export type ElementDetails = { target: ElementTarget; texts: { index: number; text: string }[]; textsTruncated: boolean; attributes: Record<string, string>; inlineStyle: string; html: string; canMove: boolean; canMoveBefore: boolean; canMoveAfter: boolean };
export type ElementEditView = { workspace: WorkspaceView; element: ElementTarget | null };
export type ElementVisual = { translate: string; color: string; backgroundColor: string; fontSize: string; width: string; height: string; borderRadius: string; padding: string };
export type Region = { element?: ElementTarget; completed?: boolean; id: string; rect: Rect; zIndex: number; prompt: string };
export type SiteContent = { html: string; css: string; javascript: string };
export type Page = { id: string; name: string; path: string; included: boolean; viewport: Viewport; appearance: Appearance; regions: Region[]; content: SiteContent | null };
export type Brief = { purpose: string; audience: string; brandName: string; tone: string };
export type Scope = { kind: 'project' } | { kind: 'page' | 'region'; id: string };
export type Decision = { id: string; topic: string; value: string; scope: Scope; protected: boolean; acceptedAtRevision: number };
export type DesignException = { id: string; rule: string; reason: string; scope: Scope; acceptedAtRevision: number };
export type Project = { schemaVersion: number; id: string; name: string; revision: number; viewport: Viewport;
  importedIdentity?: ImportedIdentity;
  appearance: Appearance; generationMode: GenerationMode; showGrid: boolean; gridSettings: GridSettings; latestGeneration: GenerationDossier | null; homePageId: string; pages: Page[]; brief: Brief;
  decisions: Decision[]; exceptions: DesignException[]; appliedJobIds: string[]; extensions: Record<string, { schemaVersion: number; data: unknown }> };
export type RecoveryInfo = { projectId: string; name: string; revision: number };
export type RecentProject = { path: string; name: string };
export type RecentProjectsView = { projects: RecentProject[]; error: string | null };
export type WebsiteExport = { path: string; indexPath: string; pageCount: number; fileCount: number; warnings: string[] };
export type WorkspaceView = { project: Project | null; path: string | null; dirty: boolean; canUndo: boolean; canRedo: boolean;
  recovery: RecoveryInfo | null; recoveries: RecoveryInfo[]; recoveryError: string | null; migrationNotice: string | null };
export type RequirementsSnapshot = { jobId: string; projectId: string; baseRevision: number; markdown: string; sha256: string;
  regionCount: number; pageCount: number; dependencies: Record<string, string> };
export type ProviderSettings = { model: string; maxOutputTokens: number; reasoningEffort: string; spendingLimitUsd: number };
export type CredentialInfo = { configured: boolean; storage: 'system' | 'session' | 'none'; maskedIdentifier: string | null; vaultError: string | null };
export type AuthenticationMethod = 'apiKey' | 'chatgpt';
export type ProviderId = 'openai' | 'anthropic' | 'google' | 'openrouter' | 'local';
export type ProviderDescriptor = { id: ProviderId; name: string; available: boolean; signInSupported: boolean; capabilities: { webSearch: boolean; imageDownloads: number } };
export type ChatgptAccount = { id: string; label: string; email: string | null; connected: boolean; planEnabled: boolean; storage: 'system' | 'session' | 'none'; storageRepairRequired: boolean };
export type ChatgptView = { accounts: ChatgptAccount[]; activeAccountId: string | null;
  signIn: { status: 'idle' | 'awaitingBrowser' | 'complete' | 'cancelled' | 'failed'; message: string | null };
  models: { id: string; name: string }[]; modelsError: string | null; storageError: string | null; showWelcome: boolean; message: string | null };
export type ModelInfo = { id: string; name: string; inputUsdPerMillion: number; outputUsdPerMillion: number; verifiedAt: string; sourceUrl: string; reasoningEfforts: string[] };
export type AiSettingsView = { provider: ProviderId; providers: ProviderDescriptor[]; settings: ProviderSettings; credential: CredentialInfo; catalogue: ModelInfo[]; configurationError: string | null;
  authentication: AuthenticationMethod; chatgpt: ChatgptView };
export type TokenUsage = { inputTokens: number; outputTokens: number; totalTokens: number };
export type GenerationStatus = 'prepared' | 'generating' | 'ready' | 'blocked' | 'failed' | 'unknown' | 'cancelled' | 'discarded' | 'applying' | 'applied';
export type GenerationAttempt = { ordinal: number; status: string; startedAt: string; finishedAt: string | null; responseId: string | null; usage: TokenUsage | null };
export type DecisionProposal = { topic: string; value: string; scope: Scope; rationale: string };
export type DesignCriticism = { regionId: string | null; message: string; suggestion: string };
export type GenerationDossier = { jobId: string; attemptOrdinal: number; baseRevision: number; recordedAtRevision: number; requirementsSha256: string; candidateSha256: string; summary: string;
  targets: { pageId: string; regionId: string }[]; decisionProposals: DecisionProposal[]; criticism: DesignCriticism[];
  applicationStatus: 'draft' | 'applied' | 'undone'; appliedAtRevision: number | null; acceptedDecisionIndices: number[] };
export type GenerationCandidate = {
  jobId: string; requirementsSha256: string; summary: string;
  regions: { regionId: string; status: 'implemented' | 'conflict'; explanation: string }[];
  pages: { pageId: string; content: SiteContent }[];
  componentRequests: { regionId: string; capability: string; resolution: 'native' | 'unavailable'; reason: string }[];
  decisionProposals: DecisionProposal[];
  criticism: DesignCriticism[];
};
export type GenerationJobView = { jobId: string; projectId: string; baseRevision: number; requirements: RequirementsSnapshot; areas: { pageId: string; regionId: string; rect: Rect }[]; generationMode: GenerationMode;
  settings: ProviderSettings; status: GenerationStatus; estimateUsd: number; receivedBytes: number; message: string; candidate: GenerationCandidate | null;
  usage: TokenUsage | null; attempts: GenerationAttempt[]; acceptedDecisionIndices: number[]; recommendedDecisionIndices: number[];
  provider?: ProviderId; authentication: AuthenticationMethod; chatgptAccountId: string | null; failureKind: string | null; dossierError: string | null; providerError: ProviderError | null };
export type ResponseRejection = 'eventJson' | 'eventType' | 'eventName' | 'responseIdentity' | 'textLocation' | 'textIdentity' | 'textOrder' | 'textMismatch' | 'outputShape' | 'messageRole' | 'messageState' | 'messagePhase' | 'contentShape' | 'answerCount' | 'credentialEcho' | 'candidateJson' | 'candidateShape' | 'jobMismatch' | 'requirementsMismatch' | 'usage';
export type ResponseCorrelation = {
  terminalOutput: 'notReceived' | 'absent' | 'null' | 'array' | 'invalid';
  terminalItems: number; terminalTextParts: number; streamedParts: number; finalizedItems: number; emptyAddedItems: number;
  cause: 'invalidIndex' | 'missingIdentity' | 'itemAbsent' | 'itemDuplicate' | 'identityChanged' | 'contentPartAbsent' | 'unfinishedItem' | 'itemChanged';
  outputIndex?: number | null; contentIndex?: number | null;
};
export type ProviderError = { kind: string; outcome: 'notSubmitted' | 'rejected' | 'unknown' | 'received';
  httpStatus: number | null; code: string | null; param: string | null; requestId: string | null;
  responseFormat?: { contentType: string | null; bodyKind: 'unavailable' | 'eventStream' | 'empty' | 'jsonError' | 'jsonDetail' | 'jsonResponse' | 'jsonOther' | 'html' | 'text' | 'binary'; bodyBytes: number; providerMessage?: string | null; bodyTruncated?: boolean; limitBytes?: number | null; rejection?: ResponseRejection | null; correlation?: ResponseCorrelation | null } | null };
export type PreviewDescriptor = { url: string; sessionId: string; sourceSha256: string | null };
export type Command =
  | { type: 'rename_project'; name: string }
  | { type: 'set_grid'; enabled: boolean }
  | { type: 'set_grid_settings'; settings: GridSettings }
  | { type: 'set_generation_mode'; mode: GenerationMode }
  | { type: 'add_page'; name: string; path: string }
  | { type: 'update_page'; page_id: string; name: string; path: string; viewport: Viewport; appearance: Appearance; included: boolean }
  | { type: 'delete_page'; page_id: string }
  | { type: 'add_region'; page_id: string; rect: Rect }
  | { type: 'add_element_region'; page_id: string; rect: Rect; element: ElementTarget }
  | { type: 'update_region'; page_id: string; region_id: string; rect: Rect; prompt: string }
  | { type: 'dismiss_empty_regions'; keep_region_id: string | null }
  | { type: 'arrange_region'; page_id: string; region_id: string; direction: LayerDirection }
  | { type: 'remove_region'; page_id: string; region_id: string }
  | { type: 'set_brief'; brief: Brief }
  | { type: 'set_decision'; topic: string; value: string; scope: Scope; protected: boolean }
  | { type: 'remove_decision'; decision_id: string }
  | { type: 'add_exception'; rule: string; reason: string; scope: Scope }
  | { type: 'remove_exception'; exception_id: string };
