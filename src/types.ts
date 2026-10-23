export type SourcePageRef = { path: string; title: string; url?: string | null };
export type SourcePage = SourcePageRef & { kind: string; photoCount: number; itemCount: number };
export type ImportedPageContext = { pageId: string; source: SourcePage; photoIds: string[]; content: ImportedItem[] };
export type Appearance = 'light' | 'dark';
export type IdentityAsset = { id: string; name: string; mime: string; dataUri: string; sha256: string };
export type ProjectImage = Pick<IdentityAsset, 'id' | 'name' | 'dataUri'>;
export type ProjectPhoto = { id: string; name: string; dataUri: string; sha256: string; width: number; height: number; description: string; sourcePages?: SourcePageRef[] };
export type ImportedItem = { kind: 'project' | 'tutorial' | 'article' | 'page' | 'product' | 'other'; title: string; url: string; summary: string; sourcePages?: SourcePageRef[] };
export type ImportedIdentity = { sourceName: string; sourceSha256: string; summary: string; assets: IdentityAsset[]; photos?: ProjectPhoto[]; content?: ImportedItem[]; pages?: ImportedPageContext[] };
export type IdentitySource = { id: string; name: string; sha256: string; files: { id: string; name: string; textBytes: number; sourcePage?: SourcePageRef | null; asset: IdentityAsset | null }[]; skipped: string[]; photos?: { id: string; name: string; width: number; height: number; thumbnail: string; sourcePages?: SourcePageRef[] }[]; pages?: SourcePage[] };
export type IdentitySelection = { fileIds: string[]; fits: boolean; message: string | null; needsSettings: boolean };
export type IdentityCandidate = { jobId: string; requirementsSha256: string; summary: string; brandName: string; tone: string; findings: { topic: string; value: string; evidence: string[]; confidence: 'high' | 'medium' | 'low' }[]; logoAssetIds: string[]; warnings: string[]; photos?: { id: string; description: string }[]; content?: ImportedItem[] };
export type IdentityAnalysis = { jobId: string; sourceId: string; source: IdentitySource; fileIds: string[]; authentication?: AuthenticationMethod | null; chatgptAccountId?: string | null; status: 'analyzing' | 'complete' | 'failed' | 'unknown' | 'cancelled'; message: string; receivedBytes: number; candidate: IdentityCandidate | null; error: string | null };
export type NewProjectIdentity = { identityAnalysisId: string; identityFindingIndices: number[]; identityAssetIds: string[]; identityPhotoIds?: string[]; identityItemIndices?: number[]; identityPagePaths?: string[] };
export type GenerationMode = 'fuzzy' | 'strict';
export type Viewport = { width: number; height: number };
export type GridSettings = { spacing: number; unit: 'pixels' | 'percent'; snap: boolean; rulers: boolean };
export type Rect = { x: number; y: number; width: number; height: number };
export type RegionShape = { kind: 'rect' } | { kind: 'ellipse' } | { kind: 'freehand'; points: [number, number][] };
export type ElementTarget = { id?: string; selector: string; tag: string; label: string; sourceSha256?: string; document?: DocumentTarget };
export type DocumentTarget = { kind: 'page'; pageId: string } | { kind: 'siteFile'; path: string };
export type FrameDocumentView = { iframeId: string; src: string; path?: string | null; pageId?: string; document: DocumentTarget; sourceSha256: string; content: SiteContent };
export type FrameDocumentsArgs = { projectId: string; pageId: string };
export type EnsureElementIdArgs = { projectId: string; pageId: string; revision: number; target: ElementTarget; document?: DocumentTarget | null };
export type FrameTarget = { pageId: string; iframeId: string; document: DocumentTarget; rect: Rect };
export type FrameSource = { kind: 'page'; pageId: string } | { kind: 'siteFile'; path: string } | { kind: 'external'; url: string } | { kind: 'unresolvable'; src: string };
export type PageFrameTarget = { iframeId: string; src: string; source: FrameSource; document?: DocumentTarget | null; sourceSha256?: string | null };
export type FrameTargetsArgs = { projectId: string; pageId: string };
export type InspectElementArgs = { projectId: string; pageId: string; target: ElementTarget; document?: DocumentTarget | null };
export type EditElementArgs = { projectId: string; pageId: string; revision: number; target: ElementTarget; change: ElementEdit; document?: DocumentTarget | null };
export type UndoElementArgs = { projectId: string; pageId: string; revision: number; target: ElementTarget; document?: DocumentTarget | null };
export type CopyLayerArgs = { projectId: string; pageId: string; revision: number; target: LayerTarget; visualStyles: ElementStyleEdit[]; document?: DocumentTarget | null };
export type PasteLayerArgs = { projectId: string; pageId: string; revision: number; after: ElementTarget | null; document?: DocumentTarget | null };
export type LayerDirection = 'forward' | 'backward' | 'front' | 'back';
export type LayerPseudoStyle = { pseudo: 'before' | 'after' | 'marker'; style: string };
export type ElementStyleEdit = { target: ElementTarget; style: string; pseudoStyles?: LayerPseudoStyle[] };
export type LayerTarget = { kind: 'request'; regionId: string } | { kind: 'element'; target: ElementTarget };
export type LayerPasteView = { workspace: WorkspaceView; selection: { element: ElementTarget | null; regionId: string | null } };
export type ElementSelection = { element: ElementTarget; rect: Rect };
export type ElementEdit = { kind: 'update'; texts: { index: number; text: string }[]; attributes: Record<string, string | null>; style: string | null }
  | { kind: 'replace'; html: string; keepStyle: boolean } | { kind: 'reorder'; direction: 'before' | 'after' } | { kind: 'layerStyles'; styles: ElementStyleEdit[] } | { kind: 'remove' };
export type ElementBounds = { regionId: string; bounds: { target: ElementTarget; rect: Rect }[] };
export type ElementDetails = { target: ElementTarget; texts: { index: number; text: string }[]; textsTruncated: boolean; attributes: Record<string, string>; inlineStyle: string; html: string; canMove: boolean; canMoveBefore: boolean; canMoveAfter: boolean };
export type ElementEditView = { workspace: WorkspaceView; element: ElementTarget | null };
export type ElementVisual = { translate: string; color: string; backgroundColor: string; fontSize: string; width: string; height: string; borderRadius: string; padding: string; textAlign?: string };
export type RequestAttachment = { id: string; name: string; kind: 'image' | 'text' | 'link'; dataUri?: string; preview?: string; text?: string; url?: string };
export type Region = { shape?: RegionShape; areaBounds?: { target: ElementTarget; rect: Rect }[]; attachments?: RequestAttachment[]; elements?: ElementTarget[]; element?: ElementTarget; frameTarget?: FrameTarget; allowTargetRemoval?: boolean; mobileOnly?: boolean; runtimeErrors?: string[]; wholePage?: boolean; wholePageDisabled?: boolean; completed?: boolean; id: string; rect: Rect; zIndex: number; prompt: string };
export type SiteContent = { html: string; css: string; javascript: string };
export type Page = { id: string; name: string; path: string; included: boolean; viewport: Viewport; appearance: Appearance; regions: Region[]; content: SiteContent | null };
export type Brief = { purpose: string; audience: string; brandName: string; tone: string };
export type Scope = { kind: 'project' } | { kind: 'page' | 'region'; id: string };
export type Decision = { id: string; topic: string; value: string; scope: Scope; protected: boolean; acceptedAtRevision: number };
export type DesignException = { id: string; rule: string; reason: string; scope: Scope; acceptedAtRevision: number };
export type Project = { website?: { framework: string | null; skipped: string[] }; pictures?: Record<string, string>; schemaVersion: number; id: string; name: string; revision: number; viewport: Viewport;
  importedIdentity?: ImportedIdentity;
  appearance: Appearance; generationMode: GenerationMode; showGrid: boolean; gridSettings: GridSettings; latestGeneration: GenerationDossier | null; homePageId: string; pages: Page[]; brief: Brief;
  decisions: Decision[]; exceptions: DesignException[]; appliedJobIds: string[]; extensions: Record<string, { schemaVersion: number; data: unknown }> };
export type RecoveryInfo = { projectId: string; name: string; revision: number; savedPath: string | null; editedAtMs: number };
export type RecentProject = { path: string; name: string };
export type RecentProjectsView = { projects: RecentProject[]; error: string | null };
export type ProjectLocationView = { path: string; systemDefault: string; customized: boolean };
export type WebsiteExport = { path: string; indexPath: string; pageCount: number; fileCount: number; warnings: string[] };
export type HistoryView = { sessionId: string; start: number; position: number; end: number; version: number };
export type OpenProjectSummary = { id: string; name: string; dirty: boolean; active: boolean; pages: { id: string; name: string }[] };
export type WorkspaceView = { documentsKey?: string; openProjects?: OpenProjectSummary[]; anyDirty?: boolean; history: HistoryView | null; project: Project | null; path: string | null; dirty: boolean; canUndo: boolean; canRedo: boolean;
  siteWarnings?: string[]; outsideEdits?: OutsideEdits; projectFolder?: boolean;
  recovery: RecoveryInfo | null; recoveries: RecoveryInfo[]; recoveryError: string | null; migrationNotice: string | null };
export type RequirementsSnapshot = { jobId: string; projectId: string; baseRevision: number; markdown: string; sha256: string;
  regionCount: number; pageCount: number; dependencies: Record<string, string> };
export type ProviderSettings = { model: string; maxOutputTokens: number; reasoningEffort: string; spendingLimitUsd: number; imageGeneration: boolean };
export type CredentialInfo = { configured: boolean; storage: 'system' | 'session' | 'none'; maskedIdentifier: string | null; vaultError: string | null };
export type AuthenticationMethod = 'apiKey' | 'chatgpt';
export type ProviderId = 'openai' | 'anthropic' | 'google' | 'openrouter' | 'local';
export type ProviderDescriptor = { id: ProviderId; name: string; available: boolean; signInSupported: boolean; capabilities: { webSearch: boolean; imageDownloads: number; imageGeneration?: { maxImages: number; sizes: string[] } } };
export type ChatgptAccount = { id: string; label: string; email: string | null; connected: boolean; planEnabled: boolean; storage: 'system' | 'session' | 'none'; storageRepairRequired: boolean };
export type ChatgptView = { accounts: ChatgptAccount[]; activeAccountId: string | null;
  signIn: { status: 'idle' | 'awaitingBrowser' | 'complete' | 'cancelled' | 'failed'; message: string | null };
  models: { id: string; name: string }[]; modelsError: string | null; storageError: string | null; showWelcome: boolean; message: string | null };
export type ModelInfo = { id: string; name: string; inputUsdPerMillion: number; outputUsdPerMillion: number; verifiedAt: string; sourceUrl: string; reasoningEfforts: string[] };
export type ImageAccess = { available: 'yes' | 'no' | 'unknown'; checkedAt: number; reason: string };
export type AiSettingsView = { imageCredential: 'chatgpt' | 'connection' | 'imageKey' | null; imageKey: CredentialInfo; mainImageAccess: ImageAccess | null; imageKeyAccess: ImageAccess | null; imageAccess: ImageAccess | null; imageAccessChecking: boolean; provider: ProviderId; providers: ProviderDescriptor[]; settings: ProviderSettings; credential: CredentialInfo; catalogue: ModelInfo[]; configurationError: string | null;
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
  documents?: { document: DocumentTarget; content: SiteContent }[];
  componentRequests: { regionId: string; capability: string; resolution: 'native' | 'unavailable'; reason: string }[];
  decisionProposals: DecisionProposal[];
  criticism: DesignCriticism[];
};
export type GenerationDraft = { baseContent?: SiteContent | null; id: string; pageId: string | null; regionIds: string[]; code: string; content: SiteContent | null; reason: string; dismissed: boolean; applied: boolean };
export type DraftReview = { job: GenerationJobView; workspace: WorkspaceView | null; revision: number; blocked: string | null; previewable: boolean; confirmation: string };
export type GenerationJobView = { drafts?: GenerationDraft[]; jobId: string; projectId: string; baseRevision: number; requirements: RequirementsSnapshot; context?: RequirementsSnapshot | null; earlierContexts?: RequirementsSnapshot[]; firstExchange?: { summary: string; regions: GenerationCandidate['regions']; usage: TokenUsage | null } | null; repairRounds?: number; repairAreaCount?: number; validationEvents?: { kind: string; disposition: string; regionId: string | null; message: string }[]; rawReply?: string | null; createdAt?: string | null; areas: { pageId: string; regionId: string; rect: Rect; shape?: RegionShape; document?: DocumentTarget | null; frameRect?: Rect | null }[]; generationMode: GenerationMode;
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
export type PreviewDescriptor = { website?: boolean; content?: SiteContent | null; url: string; sessionId: string; sourceSha256: string | null };
export type Command =
  | { type: 'rename_project'; name: string }
  | { type: 'set_grid'; enabled: boolean }
  | { type: 'format_page_code'; page_id: string }
  | { type: 'set_grid_settings'; settings: GridSettings }
  | { type: 'set_generation_mode'; mode: GenerationMode }
  | { type: 'add_page'; name: string; path: string }
  | { type: 'update_page'; page_id: string; name: string; path: string; viewport: Viewport; appearance: Appearance; included: boolean }
  | { type: 'delete_page'; page_id: string }
  | { type: 'add_region'; page_id: string; rect: Rect }
  | { type: 'add_shaped_region'; page_id: string; rect: Rect; shape: RegionShape }
  | { type: 'add_element_region'; page_id: string; rect: Rect; element: ElementTarget }
  | { type: 'add_element_regions'; page_id: string; elements: ElementTarget[]; bounds: { target: ElementTarget; rect: Rect }[] }
  | { type: 'retarget_region'; page_id: string; region_id: string; elements: ElementTarget[]; bounds: { target: ElementTarget; rect: Rect }[] }
  | { type: 'set_region_target_removal'; page_id: string; region_id: string; allow_target_removal: boolean }
  | { type: 'set_region_runtime_errors'; page_id: string; region_id: string; errors: string[] }
  | { type: 'set_region_frame_target'; page_id: string; region_id: string; frame_target: FrameTarget | null }
  | { type: 'set_region_whole_page'; page_id: string; region_id: string; whole_page: boolean }
  | { type: 'set_region_mobile_only'; page_id: string; region_id: string; mobile_only: boolean }
  | { type: 'add_region_attachment'; page_id: string; region_id: string; attachment: RequestAttachment }
  | { type: 'remove_region_attachment'; page_id: string; region_id: string; attachment_id: string }
  | { type: 'delete_elements'; page_id: string; elements: ElementTarget[]; document?: DocumentTarget | null }
  | { type: 'update_region'; page_id: string; region_id: string; rect: Rect; prompt: string }
  | { type: 'dismiss_empty_regions'; keep_region_id: string | null }
  | { type: 'arrange_region'; page_id: string; region_id: string; direction: LayerDirection }
  | { type: 'remove_region'; page_id: string; region_id: string }
  | { type: 'set_brief'; brief: Brief }
  | { type: 'set_decision'; topic: string; value: string; scope: Scope; protected: boolean }
  | { type: 'remove_decision'; decision_id: string }
  | { type: 'add_exception'; rule: string; reason: string; scope: Scope }
  | { type: 'remove_exception'; exception_id: string };

export type GenerationHistory = { jobs: { jobId: string; createdAt: string | null; status: GenerationStatus; usage: TokenUsage | null; summary: string; firstRequest: string }[]; skipped: number; truncated: boolean };

export type WebsiteFolderSelection = { id: string; name: string; website: { path: string; framework: string | null; output: string | null; fresh: boolean; pages: string[]; buildCommand: string[] | null; installCommand: string | null; buildUnavailable: string | null } | null };

export type ArchiveReport = { name: string; added: number; skipped: { name: string; reason: string }[] };

export type OutsideEdits = { changedPaths: string[]; keptStalePaths: string[]; backedUp: boolean };
