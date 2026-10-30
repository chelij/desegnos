export type SourcePageRef = { path: string; title: string; url?: string | null };
export type SourcePage = SourcePageRef & { kind: string; photoCount: number; itemCount: number };
export type ImportedPageContext = { pageId: string; source: SourcePage; photoIds: string[]; content: ImportedItem[] };
export type Appearance = 'light' | 'dark';
export type IdentityAsset = { id: string; name: string; mime: string; dataUri: string; sha256: string };
export type ProjectPhoto = { id: string; name: string; dataUri: string; sha256: string; width: number; height: number; description: string; sourcePages?: SourcePageRef[] };
export type ImportedItem = { kind: 'project' | 'tutorial' | 'article' | 'page' | 'product' | 'other'; title: string; url: string; summary: string; sourcePages?: SourcePageRef[] };
export type ImportedIdentity = { sourceName: string; sourceSha256: string; summary: string; assets: IdentityAsset[]; photos?: ProjectPhoto[]; content?: ImportedItem[]; pages?: ImportedPageContext[] };
export type IdentitySource = { id: string; name: string; sha256: string; files: { id: string; name: string; textBytes: number; sourcePage?: SourcePageRef | null; asset: IdentityAsset | null }[]; skipped: string[]; photos?: { id: string; name: string; width: number; height: number; thumbnail: string; sourcePages?: SourcePageRef[] }[]; pages?: SourcePage[] };
export type IdentitySelection = { fileIds: string[]; fits: boolean; message: string | null; needsSettings: boolean };
export type IdentityCandidate = { jobId: string; requirementsSha256: string; summary: string; brandName: string; tone: string; findings: { topic: string; value: string; evidence: string[]; confidence: 'high' | 'medium' | 'low' }[]; logoAssetIds: string[]; warnings: string[]; photos?: { id: string; description: string }[]; content?: ImportedItem[] };
export type IdentityAnalysis = { jobId: string; sourceId: string; source: IdentitySource; fileIds: string[]; authentication?: AuthenticationMethod | null; chatgptAccountId?: string | null; status: 'analyzing' | 'complete' | 'failed' | 'unknown' | 'cancelled'; message: string; receivedBytes: number; candidate: IdentityCandidate | null; error: string | null };
export type GenerationMode = 'fuzzy' | 'strict';
export type Viewport = { width: number; height: number };
export type GridSettings = { spacing: number; unit: 'pixels' | 'percent'; snap: boolean };
export type Rect = { x: number; y: number; width: number; height: number };
export type NamedView = { id: string; name: string; x: number; y: number; zoom: number };
export type RequestAttachment = { id: string; name: string; kind: 'image' | 'text' | 'link'; dataUri?: string; preview?: string; text?: string; url?: string };
// Persisted height modes: page grows with content; screen keeps its fixed height.
export type BoundaryKind = 'page' | 'screen';
// appScreen is retained for saved projects; new web boundaries use webpage.
export type OutputType = 'webpage' | 'appScreen' | 'slide' | 'image';
export type PageSize = { id: string; label: string; width: number; height: number; kind: BoundaryKind; x: number; y: number };
export type SizePreset = 'phone' | 'tablet' | 'desktop' | 'custom';
export type InputType = 'text' | 'email' | 'password' | 'number' | 'search' | 'tel' | 'url' | 'date' | 'time' | 'color' | 'range' | 'file';
export type ElementKind = 'box' | 'group' | 'text' | 'ellipse' | 'line' | 'image' | 'button' | 'link' | 'input' | 'textarea' | 'select' | 'checkbox' | 'radio' | 'video' | 'audio' | 'embed' | 'code';
export type TextAlign = 'left' | 'center' | 'right';
export type StackDirection = 'row' | 'column';
export type StackAlign = 'start' | 'center' | 'end' | 'stretch';
export type StackJustify = 'start' | 'center' | 'end' | 'spaceBetween';
export type ElementStroke = { color: string | null; width: number };
export type ElementShadow = { x: number; y: number; blur: number; color: string | null };
export type ElementTextStyle = { fontFamily: string; size: number; weight: number; italic: boolean; lineHeight: number; letterSpacing: number; color: string; align: TextAlign };
export type ElementOverride = { rect?: Rect | null; style?: ElementStyle | null; layout?: ElementLayout | null; hidden: boolean; dismissed: boolean };
export type SizingMode = 'fixed' | 'fit' | 'fill';
export type ElementSizing = { width: SizingMode; height: SizingMode; minWidth?: number | null; maxWidth?: number | null; minHeight?: number | null; maxHeight?: number | null };
/** Content inset for a stack or grid; the layout's single padding is the all-sides fallback. */
export type ElementPadding = { top: number; right: number; bottom: number; left: number };
export type StyleReferences = { fill?: string | null; stroke?: string | null; textColour?: string | null; text?: string | null };
export type ColourStyle = { id: string; name: string; value: string };
export type TextStyle = { id: string; name: string; fontFamily: string; size: number; weight: number; lineHeight: number; letterSpacing: number; italic: boolean };
export type ProjectStyles = { colours: ColourStyle[]; text: TextStyle[] };
export type ElementStyle = { flipX?: boolean; flipY?: boolean; sizing?: ElementSizing; padding?: ElementPadding | null; references?: StyleReferences; fill: string | null; stroke: ElementStroke; radius: number; opacity: number; shadow: ElementShadow; textStyle: ElementTextStyle };
export type ElementLayout =
  | { kind: 'free' }
  | { kind: 'stack'; direction: StackDirection; gap: number; padding: number; align: StackAlign; justify: StackJustify; wrap: boolean }
  | { kind: 'grid'; columns: number; gap: number; padding: number };
export type RequestPoint = { x: number; y: number };
export type RequestTarget =
  | { kind: 'element'; page: string; size?: string | null; id: string }
  | { kind: 'boundary'; page: string; size?: string | null }
  | { kind: 'reference'; id: string }
  | { kind: 'pointer'; id: string };
export type RequestShape =
  | { kind: 'rect'; rect: Rect }
  | { kind: 'ellipse'; rect: Rect }
  | { kind: 'polygon'; points: RequestPoint[] }
  | { kind: 'freehand'; points: RequestPoint[] };
export type RequestArea = { page?: string | null; size?: string | null; shape: RequestShape };
export type PendingRequest = { id: string; text: string; attachments: RequestAttachment[]; targets: RequestTarget[]; area: RequestArea | null; requestedAt: string };
export type RequestRecord = { id: string; prompt: string; attachments: RequestAttachment[]; createdAt: string; outcome: string | null; jobId: string | null };
export type ElementMeta = { requests: RequestRecord[]; context: RequestAttachment[] };
export type Element = {
  id: string; kind: ElementKind; name: string | null; link?: string | null; rect: Rect; rotation: number; visible: boolean; locked: boolean;
  style: ElementStyle; text: string; image: string | null; url: string | null; layout: ElementLayout;
  children: Element[]; meta: ElementMeta; overrides?: Record<string, ElementOverride>; inputType?: InputType; options?: string[]; language?: string; source?: string;
};
export type LinkedDefinition = { id: string; name: string; master: Element };
export type LinkedPasteInstance = { id: string; sourceId: string; pageId: string; parentId: string | null; index: number; rect: Rect };
export type ElementPatch = { id: string; rect?: Rect; rotation?: number; style?: ElementStyle; text?: string; image?: string | null; url?: string | null; layout?: ElementLayout;
  visible?: boolean; locked?: boolean; name?: string | null; linked_name?: string | null; input_type?: InputType; options?: string[]; language?: string; source?: string; picture_data_uri?: string;
  size_id?: string | null; all_sizes?: boolean; dismissed?: boolean };
export type ElementMove = { id: string; to_page_id: string; to_parent_id: string | null; index: number; rect: Rect; size_id?: string | null; all_sizes?: boolean };
export type Placements = { page_id: string; size_id: string; x: number; y: number; kind: BoundaryKind }[];
export type ReferenceKind = { kind: 'element'; element: Element; size?: string } | { kind: 'note'; text: string } | { kind: 'link'; url: string; title: string } | { kind: 'image'; picture: string; name?: string } | { kind: 'swatch'; color: string };
export type ReferenceItem = { id: string; rect: Rect; kind: ReferenceKind };
export type ReferencePlacement = { item_id: string; x: number; y: number };
export type PointerSource = { kind: 'element'; pageId: string; elementId: string; sizeId?: string | null } | { kind: 'boundary'; pageId: string; sizeId?: string | null };
export type PointerTarget = PointerSource | { kind: 'reference'; itemId: string } | { kind: 'url'; url: string };
export type PointerWiring = { fingerprint: string; generatedComment: string };
export type PlayAction = { kind: 'goTo'; boundary: string } | { kind: 'show'; element: string } | { kind: 'hide'; element: string } | { kind: 'toggle'; element: string } | { kind: 'swap'; element: string; withElement: string };
export type PointerKind = 'reference' | 'change';
export type Pointer = { id: string; from: PointerSource; to: PointerTarget; comment: string; kind: PointerKind; wiring: PointerWiring | null; play?: PlayAction | null; playComment?: string };
export type PointerStateKind = 'unwired' | 'needsGenerating' | 'needsExporting' | 'codeChanged' | 'wired' | 'contradicts' | 'broken';
export type PointerStateView = { id: string; state: PointerStateKind; clashWith: string[] };
export type ExportedPage = { pageId: string; path: string; output: OutputType };
export type ExportedPointer = { fingerprint: string; comment: string };
export type ExportRecord = { id: string; version: number; destination: string; exportedAt: string; pages: ExportedPage[]; pointers: Record<string, ExportedPointer> };
export type Page = { id: string; name: string; path: string; included: boolean; output: OutputType; sizes: PageSize[]; background: string; elements: Element[] };
export type Brief = { purpose: string; audience: string; brandName: string; tone: string };
export type Scope = { kind: 'project' } | { kind: 'page' | 'element'; id: string };
export type Decision = { id: string; topic: string; value: string; scope: Scope; protected: boolean; acceptedAtRevision: number };
export type DesignException = { id: string; rule: string; reason: string; scope: Scope; acceptedAtRevision: number };
export type Project = { template?: CanvasTemplate; styles?: ProjectStyles; pictures?: Record<string, string>; schemaVersion: number; id: string; name: string; revision: number; viewport: Viewport;
  importedIdentity?: ImportedIdentity;
  appearance: Appearance; generationMode: GenerationMode; showGrid: boolean; gridSettings: GridSettings; latestGeneration: GenerationDossier | null; homePageId: string; pages: Page[]; reference: ReferenceItem[]; pointers?: Pointer[]; linked?: Record<string, LinkedDefinition>; requests?: PendingRequest[]; views: NamedView[]; brief: Brief;
  lastExport?: ExportRecord | null;
  decisions: Decision[]; exceptions: DesignException[]; appliedJobIds: string[]; extensions: Record<string, { schemaVersion: number; data: unknown }> };
export type RecoveryInfo = { projectId: string; name: string; revision: number; savedPath: string | null; editedAtMs: number };
export type RecentProject = { path: string; name: string; pinned: boolean; openedAtMs: number };
export type RecentProjectsView = { projects: RecentProject[]; error: string | null };
export type AppSettingsView = { showStartPage: boolean };
export type CanvasTemplate = 'blank' | 'website' | 'appScreen' | 'slideDeck' | 'socialImage';
export type ProjectLocationView = { path: string; systemDefault: string; customized: boolean };
export type ExportPageView = { pageId: string; name: string; path: string; output: OutputType; file: string };
export type ExportResultView = { label: string; path: string; fileCount: number; pageCount: number; previewUrl: string | null };
export type ExportAttemptView = { ordinal: number; status: string; usage: TokenUsage | null };
export type ExportStatus = 'prepared' | 'sending' | 'ready' | 'failed' | 'cancelled' | 'stale';
export type ExportJobView = { jobId: string; projectId: string; baseRevision: number; kind: 'code' | 'comment'; status: ExportStatus;
  message: string; summary: string; estimateUsd: number; usage: TokenUsage | null; attempts: ExportAttemptView[];
  pages: ExportPageView[]; sizeCount: number; pointerCount: number; warnings: string[]; result: ExportResultView | null; error: string | null };
export type ExportCheck = { states: PointerStateView[]; lastExport: ExportRecord | null; readable: boolean };
export type HistoryEntry = { label: string; at: number; position: number };
export type HistoryView = { entries: HistoryEntry[]; sessionId: string; start: number; position: number; end: number; version: number };
export type VersionSummary = { id: number; parent: number | null; label: string; createdAt: number; head: boolean };
export type BranchSummary = { name: string; head: number; current: boolean; primary: boolean };
export type ProjectHistoryView = { branch: string; primaryBranch: string; branches: BranchSummary[]; versions: VersionSummary[] };
export type CompactReport = { removedVersions: number; removedBlobs: number };
export type OpenProjectSummary = { id: string; name: string; dirty: boolean; active: boolean; pages: { id: string; name: string }[] };
export type WorkspaceView = { untitled?: boolean; openProjects?: OpenProjectSummary[]; anyDirty?: boolean; history: HistoryView | null; project: Project | null; path: string | null; dirty: boolean; canUndo: boolean; canRedo: boolean;
  pointerStates?: PointerStateView[];
  recovery: RecoveryInfo | null; recoveries: RecoveryInfo[]; recoveryError: string | null };
export type RequirementsSnapshot = { jobId: string; projectId: string; baseRevision: number; markdown: string; sha256: string;
  requestCount: number; pageCount: number; dependencies: Record<string, string>; referenceIds?: string[] };
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
export type DesignCriticism = { elementId: string | null; message: string; suggestion: string };
export type GenerationDossier = { jobId: string; attemptOrdinal: number; baseRevision: number; recordedAtRevision: number; requirementsSha256: string; candidateSha256: string; summary: string;
  targets: { pageId: string; elementId: string }[]; decisionProposals: DecisionProposal[]; criticism: DesignCriticism[];
  applicationStatus: 'draft' | 'applied' | 'undone'; appliedAtRevision: number | null; acceptedDecisionIndices: number[] };
export type ElementDraft = Omit<Element, 'id' | 'meta' | 'image'> & { image?: string | null; imageRequest?: string | null; children: ElementDraft[] };
export type ElementUpdate = { elementId: string; rect?: Rect; rotation?: number; style?: ElementStyle; text?: string; url?: string; layout?: ElementLayout; visible?: boolean; locked?: boolean; name?: string; inputType?: InputType; options?: string[]; language?: string; source?: string; sizeId?: string | null };
export type ElementOperation =
  | { kind: 'replaceChildren'; children: ElementDraft[] }
  | { kind: 'setBehaviour'; elementId: string | null; behaviour: string }
  | { kind: 'update'; patches: ElementUpdate[] }
  | { kind: 'insert'; parentId: string; index?: number; element: ElementDraft }
  | { kind: 'remove'; elementId: string }
  | { kind: 'insertInBoundary'; pageId: string; sizeId?: string | null; element: ElementDraft }
  | { kind: 'addReference'; item: { rect: Rect; kind: ReferenceKind } };
export type ClassifiedAction = { kind: 'goTo'; boundary: string } | { kind: 'show' | 'hide' | 'toggle'; element: string } | { kind: 'swap'; element: string; withElement: string } | { kind: 'exportOnly' };
export type GenerationCandidate = {
  jobId: string; requirementsSha256: string; summary: string;
  requests: { requestId?: string | null; elementId?: string | null; status: 'implemented' | 'conflict'; explanation: string; operations: ElementOperation[] }[];
  images?: { key: string; prompt: string; width: number; height: number }[];
  pointerClassifications?: { pointerId: string; comment: string; action: ClassifiedAction }[];
  decisionProposals: DecisionProposal[];
  criticism: DesignCriticism[];
};
export type GenerationBatch = { part: number; requirements: RequirementsSnapshot; estimateUsd: number; candidate?: GenerationCandidate | null; status: GenerationStatus;
  message: string; validationEvents?: { kind: string; disposition: string; elementId: string | null; message: string }[]; repairRounds: number };
export type GenerationJobView = { batches?: GenerationBatch[]; mergedRequirements?: RequirementsSnapshot | null; jobId: string; projectId: string; baseRevision: number; requirements: RequirementsSnapshot; context?: RequirementsSnapshot | null; earlierContexts?: RequirementsSnapshot[]; firstExchange?: { summary: string; requests: GenerationCandidate['requests']; usage: TokenUsage | null } | null; repairRounds?: number; repairAreaCount?: number; validationEvents?: { kind: string; disposition: string; elementId: string | null; message: string }[]; rawReply?: string | null; createdAt?: string | null; areas: { pageId: string; elementId: string }[]; generationMode: GenerationMode;
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
export type Command =
  | { type: 'add_colour_style' | 'update_colour_style'; style: ColourStyle }
  | { type: 'add_text_style' | 'update_text_style'; style: TextStyle }
  | { type: 'remove_colour_style' | 'remove_text_style'; id: string }
  | { type: 'rename_project'; name: string }
  | { type: 'set_grid'; enabled: boolean }
  | { type: 'set_grid_settings'; settings: GridSettings }
  | { type: 'set_generation_mode'; mode: GenerationMode }
  | { type: 'update_page'; page_id: string; name: string; path: string; background: string; included: boolean }
  | { type: 'set_page_output'; page_id: string; output: OutputType }
  | { type: 'forget_export_pointer'; pointer_id: string }
  | { type: 'update_page_size'; page_id: string; size_id: string; label?: string; width?: number; height?: number; kind?: BoundaryKind; moves?: Placements; reference?: ReferencePlacement[] }
  | { type: 'create_boundary'; page: Page; after_page_id?: string; moves: Placements; reference: ReferencePlacement[] }
  | { type: 'add_canvas_page'; name: string; path: string; size: PageSize; moves: Placements; reference: ReferencePlacement[] }
  | { type: 'add_page_size'; page_id: string; size: PageSize; moves: Placements; reference: ReferencePlacement[] }
  | { type: 'remove_page_size'; page_id: string; size_id: string }
  | { type: 'set_canvas_layout'; placements: Placements; reference: ReferencePlacement[] }
  | { type: 'add_named_view'; name: string; x: number; y: number; zoom: number }
  | { type: 'update_named_view'; view_id: string; name?: string; x?: number; y?: number; zoom?: number }
  | { type: 'remove_named_view'; view_id: string }
  | { type: 'add_reference_item'; item: ReferenceItem; picture_data_uri?: string }
  | { type: 'update_reference_item'; item_id: string; rect?: Rect; text?: string; url?: string; title?: string; color?: string }
  | { type: 'remove_reference_items'; ids: string[] }
  | { type: 'add_pointer'; pointer: Pointer }
  | { type: 'update_pointer'; pointer_id: string; from?: PointerSource; to?: PointerTarget; comment?: string; kind?: PointerKind }
  | { type: 'remove_pointers'; ids: string[] }
  | { type: 'set_pointer_wiring'; pointer_id: string; wiring: PointerWiring | null; comment?: string }
  | { type: 'resolve_pointer_clash'; keep: string; drop: string[]; merged_comment?: string }
  | { type: 'element_to_reference'; page_id: string; element_id: string; rect: Rect; size_id?: string | null }
  | { type: 'reference_to_element'; item_id: string; page_id: string; rect: Rect; size_id?: string | null }
  | { type: 'attach_reference_context'; request_ids: string[]; item_ids: string[] }
  | { type: 'delete_page'; page_id: string }
  | { type: 'insert_elements'; page_id: string; parent_id: string | null; index: number; elements: Element[]; picture_data_uri?: string; size_id?: string | null }
  | { type: 'update_elements'; page_id: string; patches: ElementPatch[] }
  | { type: 'link_elements'; ids: string[]; source_id?: string | null }
  | { type: 'unlink_elements'; ids: string[] }
  | { type: 'remove_linked_definition'; id: string }
  | { type: 'paste_linked_instances'; items: LinkedPasteInstance[] }
  | { type: 'move_elements'; moves: ElementMove[] }
  | { type: 'nudge_elements'; moves: ElementMove[]; dx: number; dy: number }
  | { type: 'stack_elements'; page_id: string; ids: string[] }
  | { type: 'remove_elements'; page_id: string; ids: string[] }
  | { type: 'delete_items'; elements?: [string, string][]; reference_ids?: string[]; pointer_ids?: string[]; request_ids?: string[]; page_ids?: string[]; size_ids?: [string, string][]; linked_ids?: string[] }
  | { type: 'group_elements'; page_id: string; ids: string[] }
  | { type: 'ungroup_elements'; page_id: string; id: string }
  | { type: 'add_request'; request: PendingRequest }
  | { type: 'update_request'; request: PendingRequest }
  | { type: 'remove_requests'; ids: string[] }
  | { type: 'update_element_context'; element_id: string; context: RequestAttachment[] }
  | { type: 'set_brief'; brief: Brief }
  | { type: 'set_decision'; topic: string; value: string; scope: Scope; protected: boolean }
  | { type: 'remove_decision'; decision_id: string }
  | { type: 'add_exception'; rule: string; reason: string; scope: Scope }
  | { type: 'remove_exception'; exception_id: string };

export type GenerationHistory = { jobs: { jobId: string; createdAt: string | null; status: GenerationStatus; usage: TokenUsage | null; summary: string; firstRequest: string }[]; skipped: number; truncated: boolean };

export type WebScript = { src?: string | null; text?: string | null };
export type WebNodeAttributes = { id: string; className: string; role: string; ariaExpanded?: string | null; ariaControls?: string | null; dataToggle?: string | null; name?: string | null };
export type WebNodeStyles = { background: string; color: string; borderColor: string; borderWidth: number; radius: number; opacity: number;
  shadowX: number; shadowY: number; shadowBlur: number; shadowColor: string;
  fontFamily: string; fontSize: number; fontWeight: number; italic: boolean; lineHeight: number; letterSpacing: number; align: string;
  display: string; direction: string; rowGap: number; columnGap: number; padding: number; columns: number; alignItems: string; justifyContent: string; wrap: boolean };
export type WebNode = { tag: string; rect: Rect; text?: string | null; src?: string | null; poster?: string | null; href?: string | null;
  inputType?: string | null; placeholder?: string | null; value?: string | null; alt?: string | null; options: string[];
  attributes: WebNodeAttributes; styles: WebNodeStyles; children: WebNode[] };
export type WebCapture = { url: string; site?: string; route?: string; title: string; width: number; height: number; nodes: WebNode[]; scripts: WebScript[] };
export type WebsiteImportPageView = { route: string; title: string; url: string };
export type WebsiteImportSourceView = { id: string; name: string; baseUrl: string; extractorUrl: string; totalPages: number; identitySource: IdentitySource | null; identityError: string | null; skipped: string[]; pages: WebsiteImportPageView[] };
export type WebsiteImportResultView = { workspace: WorkspaceView; pages: number; layers: number; pictures: number; warnings: string[] };
