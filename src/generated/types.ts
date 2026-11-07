// Generated from Rust by `npm run generate:contract`. Do not edit.

export type AccessResult = { providerError: ProviderError | null, message: string, };

export type AiSettingsView = { imageCredential: string | null, imageKey: CredentialInfo, mainImageAccess: ImageAccess | null, imageKeyAccess: ImageAccess | null, imageAccess: ImageAccess | null, imageAccessChecking: boolean, provider: ProviderId, providers: Array<ProviderDescriptor>, settings: ProviderSettings, credential: CredentialInfo, catalogue: Array<ModelInfo>, modelsError: string | null, modelsProviderError: ProviderError | null, modelsAuthRequired: boolean, configurationError: string | null, authentication: AuthenticationMethod, chatgpt: ChatgptView, };

export type AnalysisStatus = "analyzing"
  | "complete"
  | "failed"
  | "unknown"
  | "cancelled";

export type AppSettingsView = { showStartPage: boolean, };

export type Appearance = "light"
  | "dark";

export type AttachmentKind = "image"
  | "text"
  | "link";

export type AuthenticationMethod = "apiKey"
  | "chatgpt";

export type BoundaryKind = "page"
  | "screen";

export type BranchSummary = { name: string, head: number, current: boolean, primary: boolean, };

export type Brief = { purpose: string, audience: string, brandName: string, tone: string, };

export type CanvasFont = { id: string, bundled: boolean, fellBack: boolean, italic: boolean, };

export type CanvasPlacement = { page_id: string, size_id: string, x: number, y: number, kind: BoundaryKind, };

export type CanvasTemplate = "blank"
  | "website"
  | "appScreen"
  | "slideDeck"
  | "socialImage";

export type ChatgptAccount = { id: string, label: string, email: string | null, connected: boolean, planEnabled: boolean, storage: CredentialLocation, storageRepairRequired: boolean, };

export type ChatgptModel = { id: string, name: string, reasoningEfforts: Array<ReasoningEffort>, contextWindowTokens: number | null, maxOutputTokens: number | null, };

export type ChatgptSignIn = { status: ChatgptSignInStatus, message: string | null, };

export type ChatgptSignInStatus = "idle"
  | "awaitingBrowser"
  | "complete"
  | "cancelled"
  | "failed";

export type ChatgptView = { accounts: Array<ChatgptAccount>, activeAccountId: string | null, signIn: ChatgptSignIn, models: Array<ChatgptModel>, modelsError: string | null, modelsProviderError: ProviderError | null, storageError: string | null, showWelcome: boolean, message: string | null, };

export type ClassifiedAction = { "kind": "goTo", boundary: string, }
  | { "kind": "show", element: string, }
  | { "kind": "hide", element: string, }
  | { "kind": "toggle", element: string, }
  | { "kind": "swap", element: string, withElement: string, }
  | { "kind": "exportOnly" };

export type ColourStyle = { id: string, name: string, value: string, };

export type Command = { "type": "add_colour_style", style: ColourStyle, }
  | { "type": "update_colour_style", style: ColourStyle, }
  | { "type": "remove_colour_style", id: string, }
  | { "type": "add_text_style", style: TextStyle, }
  | { "type": "update_text_style", style: TextStyle, }
  | { "type": "remove_text_style", id: string, }
  | { "type": "rename_project", name: string, }
  | { "type": "set_grid", enabled: boolean, }
  | { "type": "set_grid_settings", settings: GridSettings, }
  | { "type": "set_generation_mode", mode: GenerationMode, }
  | { "type": "update_page", page_id: string, name: string, path: string, background: string, included: boolean, }
  | { "type": "set_page_output", page_id: string, output: OutputType, }
  | { "type": "adapt_size", page_id: string, size_id: string, }
  | { "type": "update_page_size", page_id: string, size_id: string, label?: string | null, width?: number | null, height?: number | null, kind?: BoundaryKind | null, 
moves?: Array<CanvasPlacement>, reference?: Array<ReferencePlacement>, }
  | { "type": "create_boundary", page: Page, after_page_id?: string | null, moves?: Array<CanvasPlacement>, reference?: Array<ReferencePlacement>, }
  | { "type": "add_canvas_page", name: string, path: string, size: PageSize, moves?: Array<CanvasPlacement>, 
reference?: Array<ReferencePlacement>, }
  | { "type": "add_page_size", page_id: string, size: PageSize, moves?: Array<CanvasPlacement>, reference?: Array<ReferencePlacement>, }
  | { "type": "remove_page_size", page_id: string, size_id: string, }
  | { "type": "set_canvas_layout", placements: Array<CanvasPlacement>, 
reference?: Array<ReferencePlacement>, }
  | { "type": "add_named_view", name: string, x: number, y: number, zoom: number, }
  | { "type": "update_named_view", view_id: string, name?: string | null, x?: number | null, y?: number | null, zoom?: number | null, }
  | { "type": "remove_named_view", view_id: string, }
  | { "type": "add_reference_item", item: ReferenceItem, 
picture_data_uri?: string | null, }
  | { "type": "update_reference_item", item_id: string, rect?: Rect | null, text?: string | null, url?: string | null, title?: string | null, color?: string | null, }
  | { "type": "remove_reference_items", ids: Array<string>, }
  | { "type": "add_pointer", pointer: Pointer, }
  | { "type": "update_pointer", pointer_id: string, from?: PointerSource | null, to?: PointerTarget | null, comment?: string | null, kind?: PointerKind | null, }
  | { "type": "remove_pointers", ids: Array<string>, }
  | { "type": "set_pointer_wiring", pointer_id: string, wiring?: PointerWiring | null, comment?: string | null, }
  | { "type": "resolve_pointer_clash", keep: string, drop: Array<string>, merged_comment?: string | null, }
  | { "type": "record_export", record: ExportRecord, }
  | { "type": "forget_export_pointer", pointer_id: string, }
  | { "type": "element_to_reference", page_id: string, element_id: string, rect: Rect, size_id?: string | null, }
  | { "type": "reference_to_element", item_id: string, page_id: string, rect: Rect, size_id?: string | null, }
  | { "type": "attach_reference_context", request_ids: Array<string>, item_ids: Array<string>, }
  | { "type": "delete_page", page_id: string, }
  | { "type": "insert_elements", page_id: string, parent_id?: string | null, index: number, elements: Array<Element>, 
picture_data_uri?: string | null, 
size_id?: string | null, }
  | { "type": "import_website", pages: Array<Page>, reference: Array<ReferenceItem>, pointers: Array<Pointer>, pictures: { [key in string]: string }, }
  | { "type": "update_elements", page_id: string, patches: Array<ElementPatch>, }
  | { "type": "link_elements", ids: Array<string>, source_id?: string | null, }
  | { "type": "unlink_elements", ids: Array<string>, }
  | { "type": "remove_linked_definition", id: string, }
  | { "type": "paste_linked_instances", items: Array<LinkedPasteInstance>, }
  | { "type": "nudge_elements", moves: Array<ElementMove>, dx: number, dy: number, }
  | { "type": "stack_elements", page_id: string, ids: Array<string>, }
  | { "type": "move_elements", moves: Array<ElementMove>, }
  | { "type": "remove_elements", page_id: string, ids: Array<string>, }
  | { "type": "delete_items", elements?: Array<[string, string]>, reference_ids?: Array<string>, pointer_ids?: Array<string>, request_ids?: Array<string>, page_ids?: Array<string>, size_ids?: Array<[string, string]>, linked_ids?: Array<string>, }
  | { "type": "group_elements", page_id: string, ids: Array<string>, }
  | { "type": "ungroup_elements", page_id: string, id: string, }
  | { "type": "add_request", request: PendingRequest, }
  | { "type": "update_request", request: PendingRequest, }
  | { "type": "remove_requests", ids: Array<string>, }
  | { "type": "update_element_context", element_id: string, context: Array<RequestAttachment>, }
  | { "type": "set_brief", brief: Brief, }
  | { "type": "set_decision", topic: string, value: string, scope: Scope, protected: boolean, }
  | { "type": "remove_decision", decision_id: string, }
  | { "type": "add_exception", rule: string, reason: string, scope: Scope, }
  | { "type": "remove_exception", exception_id: string, };

export type CompactReport = { removedVersions: number, removedBlobs: number, };

export type Confidence = "high"
  | "medium"
  | "low";

export type CorrelationCause = "invalidIndex"
  | "missingIdentity"
  | "itemAbsent"
  | "itemDuplicate"
  | "identityChanged"
  | "contentPartAbsent"
  | "unfinishedItem"
  | "itemChanged";

export type CredentialInfo = { configured: boolean, storage: CredentialLocation, maskedIdentifier: string | null, vaultError: string | null, };

export type CredentialLocation = "system"
  | "session"
  | "none";

export type Decision = { id: string, topic: string, value: string, scope: Scope, protected: boolean, acceptedAtRevision: number, };

export type DecisionProposal = { topic: string, value: string, scope: Scope, rationale: string, };

export type DesignCriticism = { elementId: string | null, message: string, suggestion: string, };

export type DesignException = { id: string, rule: string, reason: string, scope: Scope, acceptedAtRevision: number, };

export type DossierTarget = { pageId: string, elementId: string, };

export type Element = { id: string, 
name: string | null, 
link?: string | null, 
rect: Rect, rotation: number, visible: boolean, locked: boolean, style: ElementStyle, text: string, 
image: string | null, 
url: string | null, layout: ElementLayout, 
overrides?: { [key in string]: ElementOverride }, children: Array<Element>, meta: ElementMeta, } & ({ "kind": "box" }
  | { "kind": "group" }
  | { "kind": "text" }
  | { "kind": "ellipse" }
  | { "kind": "line" }
  | { "kind": "image" }
  | { "kind": "button" }
  | { "kind": "link" }
  | { "kind": "input", inputType: InputType, }
  | { "kind": "textarea" }
  | { "kind": "select", options: Array<string>, }
  | { "kind": "checkbox" }
  | { "kind": "radio" }
  | { "kind": "video" }
  | { "kind": "audio" }
  | { "kind": "embed" }
  | { "kind": "code", language: string, source: string, });

export type ElementDraft = { rect: Rect, name: string | null, rotation: number, visible: boolean, locked: boolean, style: ElementStyle, text: string, 
image: string | null, 
imageRequest: string | null, url: string | null, layout: ElementLayout, children: Array<ElementDraft>, } & ({ "kind": "box" }
  | { "kind": "group" }
  | { "kind": "text" }
  | { "kind": "ellipse" }
  | { "kind": "line" }
  | { "kind": "image" }
  | { "kind": "button" }
  | { "kind": "link" }
  | { "kind": "input", inputType: InputType, }
  | { "kind": "textarea" }
  | { "kind": "select", options: Array<string>, }
  | { "kind": "checkbox" }
  | { "kind": "radio" }
  | { "kind": "video" }
  | { "kind": "audio" }
  | { "kind": "embed" }
  | { "kind": "code", language: string, source: string, });

export type ElementLayout = { "kind": "free" }
  | { "kind": "stack", direction: StackDirection, gap: number, padding: number, align: StackAlign, justify: StackJustify, wrap: boolean, }
  | { "kind": "grid", columns: number, gap: number, padding: number, };

export type ElementMeta = { 
requests: Array<RequestRecord>, context: Array<RequestAttachment>, };

export type ElementMove = { id: string, to_page_id: string, to_parent_id: string | null, index: number, rect: Rect, 
size_id?: string | null, 
all_sizes?: boolean, };

export type ElementOperation = { "kind": "setPage", pageId: string, name: string | null, path: string | null, background: string | null, }
  | { "kind": "replaceChildren", children: Array<ElementDraft>, }
  | { "kind": "setBehaviour", elementId: string | null, behaviour: string, }
  | { "kind": "update", patches: Array<ElementUpdate>, }
  | { "kind": "insert", parentId: string, index: number | null, element: ElementDraft, }
  | { "kind": "remove", elementId: string, }
  | { "kind": "insertInBoundary", pageId: string, sizeId?: string | null, element: ElementDraft, }
  | { "kind": "addReference", item: ReferenceDraft, };

export type ElementOverride = { 
automatic?: boolean, rect?: Rect | null, style?: ElementStyle | null, layout?: ElementLayout | null, hidden: boolean, dismissed: boolean, };

export type ElementPadding = { top: number, right: number, bottom: number, left: number, };

export type ElementPatch = { id: string, rect?: Rect | null, rotation?: number | null, style?: ElementStyle | null, text?: string | null, image?: string | null | null, url?: string | null | null, layout?: ElementLayout | null, visible?: boolean | null, locked?: boolean | null, name?: string | null | null, 
linked_name?: string | null | null, input_type?: InputType | null, options?: Array<string> | null, language?: string | null, source?: string | null, 
picture_data_uri?: string | null, 
size_id?: string | null, 
all_sizes?: boolean, 
dismissed?: boolean | null, };

export type ElementShadow = { x: number, y: number, blur: number, 
color: string | null, };

export type ElementSizing = { width: SizingMode, height: SizingMode, minWidth?: number | null, maxWidth?: number | null, minHeight?: number | null, maxHeight?: number | null, };

export type ElementStroke = { 
color: string | null, width: number, };

export type ElementStyle = { flipX?: boolean, flipY?: boolean, sizing?: ElementSizing, 
padding?: ElementPadding | null, references?: StyleReferences, 
fill: string | null, stroke: ElementStroke, radius: number, opacity: number, shadow: ElementShadow, textStyle: ElementTextStyle, };

export type ElementTextStyle = { fontFamily: string, size: number, weight: number, italic: boolean, lineHeight: number, letterSpacing: number, color: string, align: TextAlign, decoration?: TextDecoration | null, };

export type ElementUpdate = { elementId: string, rect: Rect | null, rotation: number | null, style: ElementStyle | null, text: string | null, url: string | null, layout: ElementLayout | null, visible: boolean | null, locked: boolean | null, name: string | null, inputType: InputType | null, options: Array<string> | null, language: string | null, source: string | null, 
sizeId: string | null, };

export type ErrorKind = "invalidSettings"
  | "invalidRequirements"
  | "missingCredential"
  | "invalidCredential"
  | "vaultUnavailable"
  | "contextLimit"
  | "spendingLimit"
  | "inputLimit"
  | "outputLimit"
  | "repetition"
  | "authentication"
  | "quota"
  | "rateLimited"
  | "modelUnavailable"
  | "requestRejected"
  | "redirect"
  | "refused"
  | "incomplete"
  | "truncated"
  | "malformed"
  | "unexpectedResponseFormat"
  | "cancelled"
  | "deadline"
  | "transport"
  | "server"
  | "chatgptNotEligible"
  | "chatgptUsageLimit"
  | "chatgptUsageUnavailable"
  | "chatgptUnsupportedCapability"
  | "chatgptRouteNotSupported"
  | "chatgptInvalidUser"
  | "chatgptPermission"
  | "chatgptUserUnavailable";

export type ExportAttemptView = { ordinal: number, status: string, usage: TokenUsage | null, };

export type ExportBehaviour = { 
acceptedRevision: number, fingerprint: string, js: string, };

export type ExportCheck = { states: Array<PointerStateView>, lastExport: ExportRecord | null, readable: boolean, };

export type ExportJobView = { jobId: string, projectId: string, baseRevision: number, kind: 'code' | 'comment', status: ExportStatus, message: string, summary: string, estimateUsd: number, usage: TokenUsage | null, attempts: Array<ExportAttemptView>, pages: Array<ExportPageView>, sizeCount: number, pointerCount: number, reusedBehaviourCount: number, warnings: Array<string>, result: ExportResultView | null, error: string | null, providerError: ProviderError | null, provider: ProviderId | null, };

export type ExportPageView = { pageId: string, name: string, path: string, output: OutputType, file: string, };

export type ExportRecord = { id: string, projectId: string, 
version: number, 
destination: string, exportedAt: string, pages: Array<ExportedPage>, pointers: { [key in string]: ExportedPointer }, 
behaviours?: { [key in string]: ExportBehaviour }, };

export type ExportResultView = { label: string, path: string, fileCount: number, pageCount: number, previewUrl: string | null, };

export type ExportStatus = "prepared"
  | "sending"
  | "ready"
  | "failed"
  | "cancelled"
  | "stale";

export type ExportedPage = { pageId: string, path: string, output: OutputType, };

export type ExportedPointer = { fingerprint: string, comment: string, };

export type FileStamp = { projectId: string, version: number, };

export type FrozenLayerRequest = { pageId: string, elementId: string, rect: Rect, request: string, };

export type GeneratedImage = { key: string, prompt: string, width: number, height: number, };

export type GenerationApplicationStatus = "draft"
  | "applied"
  | "undone";

export type GenerationAttempt = { ordinal: number, status: GenerationStatus, startedAt: string, finishedAt: string | null, responseId: string | null, usage: TokenUsage | null, };

export type GenerationBatch = { part: number, requirements: RequirementsSnapshot, estimateUsd: number, candidate: GenerationCandidate | null, status: GenerationStatus, message: string, validationEvents: Array<GenerationValidationEvent>, repairRounds: number, };

export type GenerationCandidate = { jobId: string, requirementsSha256: string, summary: string, requests: Array<RequestOutcome>, images: Array<GeneratedImage>, pointerClassifications: Array<PointerClassification>, decisionProposals: Array<DecisionProposal>, criticism: Array<DesignCriticism>, };

export type GenerationCapabilities = { webSearch: boolean, imageDownloads: number, imageGeneration?: ImageGenerationCapability | null, };

export type GenerationDossier = { jobId: string, attemptOrdinal: number, baseRevision: number, recordedAtRevision: number, requirementsSha256: string, candidateSha256: string, summary: string, targets: Array<DossierTarget>, decisionProposals: Array<DecisionProposal>, criticism: Array<DesignCriticism>, applicationStatus: GenerationApplicationStatus, appliedAtRevision: number | null, acceptedDecisionIndices: Array<number>, };

export type GenerationDossierContext = { jobId: string, attemptOrdinal: number, baseRevision: number, recordedAtRevision: number, summary: string | null, targets: Array<DossierTarget>, decisionProposals: Array<DecisionProposal>, criticism: Array<DesignCriticism>, applicationStatus: GenerationApplicationStatus, appliedAtRevision: number | null, acceptedDecisionIndices: Array<number>, };

export type GenerationHistory = { jobs: Array<GenerationJobSummary>, skipped: number, truncated: boolean, };

export type GenerationJobSummary = { jobId: string, createdAt: string | null, status: GenerationStatus, usage: TokenUsage | null, summary: string, firstRequest: string, };

export type GenerationJobView = { jobId: string, projectId: string, baseRevision: number, requirements: RequirementsSnapshot, 
batches: Array<GenerationBatch>, 
mergedRequirements?: RequirementsSnapshot | null, sourceFingerprints: { [key in string]: { [key in string]: string } }, appliedSourceFingerprints: { [key in string]: { [key in string]: string } }, context: RequirementsSnapshot | null, draftFeedback: Array<GenerationValidationEvent>, earlierContexts: Array<RequirementsSnapshot>, rawReply: string | null, validationEvents: Array<GenerationValidationEvent>, validationCountsRecorded: boolean, repairRounds: number, firstExchange: ValidationExchange | null, repairAreaCount: number, createdAt: string | null, areas: Array<GenerationTarget>, generationMode: GenerationMode, provider: ProviderId, settings: ProviderSettings, authentication: AuthenticationMethod, chatgptAccountId: string | null, failureKind: ErrorKind | null, providerError: ProviderError | null, providerRetry: ProviderRetry | null, status: GenerationStatus, estimateUsd: number, receivedBytes: number, 
pagesStarted: Array<string>, message: string, candidate: GenerationCandidate | null, usage: TokenUsage | null, attempts: Array<GenerationAttempt>, acceptedDecisionIndices: Array<number>, recommendedDecisionIndices: Array<number>, dossierError: string | null, };

export type GenerationMode = "fuzzy"
  | "strict";

export type GenerationStatus = "prepared"
  | "generating"
  | "ready"
  | "blocked"
  | "failed"
  | "unknown"
  | "cancelled"
  | "discarded"
  | "applying"
  | "applied";

export type GenerationTarget = { pageId: string, elementId: string, };

export type GenerationValidationEvent = { kind: string, disposition: string, elementId: string | null, message: string, };

export type GridSettings = { spacing: number, unit: GridUnit, snap: boolean, };

export type GridUnit = "pixels"
  | "percent";

export type HistoryEntry = { label: string, at: number, position: number, };

export type HistoryView = { sessionId: string, start: number, position: number, end: number, version: number, entries: Array<HistoryEntry>, };

export type IdentityAnalysis = { jobId: string, sourceId: string, source: IdentitySource, fileIds: Array<string>, status: AnalysisStatus, message: string, receivedBytes: number, authentication: AuthenticationMethod | null, chatgptAccountId: string | null, candidate: IdentityCandidate | null, error: string | null, providerError: ProviderError | null, provider: ProviderId | null, };

export type IdentityAsset = { id: string, name: string, mime: string, dataUri: string, sha256: string, };

export type IdentityCandidate = { jobId: string, requirementsSha256: string, summary: string, brandName: string, tone: string, findings: Array<IdentityFinding>, logoAssetIds: Array<string>, warnings: Array<string>, 
photos: Array<PhotoDescription>, 
content: Array<ImportedItem>, };

export type IdentityFileManifest = { id: string, name: string, textBytes: number, sourcePage: SourcePageRef | null, asset: IdentitySourceAsset | null, };

export type IdentityFinding = { topic: string, value: string, evidence: Array<string>, confidence: Confidence, };

export type IdentityPhotoManifest = { id: string, name: string, width: number, height: number, thumbnail: string, sourcePages: Array<SourcePageRef>, };

export type IdentitySelection = { fileIds: Array<string>, fits: boolean, message: string | null, needsSettings: boolean, };

export type IdentitySource = { id: string, name: string, sha256: string, files: Array<IdentityFileManifest>, skipped: Array<string>, photos: Array<IdentityPhotoManifest>, pages: Array<SourcePage>, };

export type IdentitySourceAsset = { id: string, name: string, mime: string, dataUri: string, sha256: string, };

export type ImageAccess = { available: ImageAvailability, checkedAt: number, reason: string, };

export type ImageAvailability = "yes"
  | "no"
  | "unknown";

export type ImageGenerationCapability = { maxImages: number, sizes: Array<string>, };

export type ImportedIdentity = { sourceName: string, sourceSha256: string, summary: string, assets: Array<IdentityAsset>, photos: Array<ProjectPhoto>, content: Array<ImportedItem>, pages: Array<ImportedPageContext>, };

export type ImportedItem = { kind: string, title: string, 
url: string, summary: string, sourcePages: Array<SourcePageRef>, };

export type ImportedPageContext = { pageId: string, source: SourcePage, photoIds: Array<string>, content: Array<ImportedItem>, };

export type InputType = "text"
  | "email"
  | "password"
  | "number"
  | "search"
  | "tel"
  | "url"
  | "date"
  | "time"
  | "color"
  | "range"
  | "file";

export type LinkedDefinition = { id: string, name: string, master: Element, };

export type LinkedPasteInstance = { id: string, sourceId: string, pageId: string, parentId: string | null, index: number, rect: Rect, };

export type ModelInfo = { id: string, name: string, inputUsdPerMillion: number, outputUsdPerMillion: number, verifiedAt: string, sourceUrl: string, reasoningEfforts: Array<ReasoningEffort>, contextWindowTokens: number, maxOutputTokens: number, longContextInputUsdPerMillion: number, longContextOutputUsdPerMillion: number, pricingRequired: boolean, defaultPricing?: ModelPricing | null, };

export type ModelPricing = { model: string, inputUsdPerMillion: number, outputUsdPerMillion: number, requestUsd: number, imageUsd: number, };

export type NamedView = { id: string, name: string, x: number, y: number, zoom: number, };

export type NativeDrag = { phase: string, id: string, position: Position | null, picture: NativeFile | null, files: Array<NativeFile>, fileErrors: Array<string>, message: string | null, };

export type NativeFile = { name: string, mime: string, data: string, };

export type OpenFailure = { message: string, url: string | null, };

export type OpenPageSummary = { id: string, name: string, };

export type OpenProjectSummary = { id: string, name: string, dirty: boolean, active: boolean, pages: Array<OpenPageSummary>, };

export type OutputType = "webpage"
  | "appScreen"
  | "slide"
  | "image";

export type Page = { id: string, name: string, path: string, included: boolean, 
output: OutputType, 
sizes: Array<PageSize>, background: string, 
elements: Array<Element>, };

export type PageSize = { id: string, label: string, width: number, height: number, kind: BoundaryKind, x: number, y: number, };

export type PendingCrash = { kind: string, summary: string, body: string, };

export type PendingRequest = { id: string, text: string, attachments: Array<RequestAttachment>, targets: Array<RequestTarget>, area?: RequestArea | null, requestedAt: string, 
boundaryRetryOrder?: Array<string>, };

export type PhotoDescription = { id: string, description: string, };

export type PickDestination = { path: string, };

export type PlayAction = { "kind": "goTo", boundary: string, }
  | { "kind": "show", element: string, }
  | { "kind": "hide", element: string, }
  | { "kind": "toggle", element: string, }
  | { "kind": "swap", element: string, withElement: string, };

export type Pointer = { id: string, from: PointerSource, to: PointerTarget, comment: string, kind: PointerKind, wiring: PointerWiring | null, 
play?: PlayAction | null, playComment?: string, };

export type PointerClassification = { pointerId: string, comment: string, action: ClassifiedAction, };

export type PointerKind = "reference"
  | "change";

export type PointerSource = { "kind": "element", pageId: string, elementId: string, sizeId?: string | null, }
  | { "kind": "boundary", pageId: string, sizeId?: string | null, };

export type PointerStateKind = "unwired"
  | "needsGenerating"
  | "needsExporting"
  | "codeChanged"
  | "wired"
  | "contradicts"
  | "broken";

export type PointerStateView = { id: string, state: PointerStateKind, 
clashWith: Array<string>, };

export type PointerTarget = { "kind": "element", pageId: string, elementId: string, sizeId?: string | null, }
  | { "kind": "boundary", pageId: string, sizeId?: string | null, }
  | { "kind": "reference", itemId: string, }
  | { "kind": "url", url: string, };

export type PointerWiring = { fingerprint: string, generatedComment: string, };

export type Position = { x: number, y: number, };

export type Project = { styles: ProjectStyles, schemaVersion: number, template: CanvasTemplate, id: string, name: string, revision: number, appearance: Appearance, showGrid: boolean, gridSettings: GridSettings, generationMode: GenerationMode, homePageId: string, pages: Array<Page>, brief: Brief, importedIdentity?: ImportedIdentity | null, pictures: { [key in string]: string }, 
reference: Array<ReferenceItem>, 
pointers: Array<Pointer>, 
linked: { [key in string]: LinkedDefinition }, 
requests: Array<PendingRequest>, 
views: Array<NamedView>, decisions: Array<Decision>, exceptions: Array<DesignException>, appliedJobIds: Array<string>, latestGeneration: GenerationDossier | null, 
lastExport?: ExportRecord | null, };

export type ProjectHistoryView = { branch: string, primaryBranch: string, branches: Array<BranchSummary>, versions: Array<VersionSummary>, };

export type ProjectLocationView = { path: string, systemDefault: string, customized: boolean, };

export type ProjectPhoto = { id: string, name: string, dataUri: string, sha256: string, width: number, height: number, description: string, sourcePages: Array<SourcePageRef>, };

export type ProjectStyles = { colours: Array<ColourStyle>, text: Array<TextStyle>, };

export type ProviderDescriptor = { id: ProviderId, name: string, available: boolean, signInSupported: boolean, capabilities: GenerationCapabilities, };

export type ProviderError = { kind: ErrorKind, outcome: SubmissionOutcome, responseId: string | null, usage: TokenUsage | null, authentication: AuthenticationMethod, httpStatus: number | null, code: string | null, param: string | null, requestId: string | null, responseFormat?: ResponseFormat | null, };

export type ProviderId = "openai"
  | "anthropic"
  | "google"
  | "openrouter"
  | "deepseek"
  | "local";

export type ProviderRetry = { attempt: number, kind: ErrorKind, };

export type ProviderSettings = { model: string, maxOutputTokens: number, reasoningEffort: ReasoningEffort, spendingLimitUsd: number, imageGeneration: boolean, pricing?: ModelPricing | null, endpoint?: string | null, localAuthentication?: boolean, };

export type ReasoningEffort = string;

export type RecentProject = { path: string, name: string, 
pinned: boolean, 
openedAtMs: number, };

export type RecentProjectsView = { projects: Array<RecentProject>, error: string | null, };

export type RecoveryInfo = { projectId: string, name: string, revision: number, savedPath: string | null, editedAtMs: number, };

export type Rect = { x: number, y: number, width: number, height: number, };

export type ReferenceDraft = { rect: Rect, kind: ReferenceKind, };

export type ReferenceItem = { id: string, rect: Rect, kind: ReferenceKind, };

export type ReferenceKind = { "kind": "element", element: Element, size?: string | null, }
  | { "kind": "note", text: string, }
  | { "kind": "link", url: string, title: string, }
  | { "kind": "image", picture: string, 
name?: string, }
  | { "kind": "swatch", color: string, };

export type ReferencePlacement = { item_id: string, x: number, y: number, };

export type RequestArea = { 
page?: string | null, size?: string | null, shape: RequestShape, };

export type RequestAttachment = { id: string, name: string, kind: AttachmentKind, dataUri?: string | null, preview?: string | null, text?: string | null, url?: string | null, };

export type RequestImage = { attachmentId: string, dataUri: string, storedFormat?: string | null, transparent?: boolean | null, transparentPercent?: number | null, };

export type RequestOutcome = { 
requestId?: string | null, 
elementId?: string | null, status: RequestStatus, explanation: string, operations: Array<ElementOperation>, };

export type RequestPoint = { x: number, y: number, };

export type RequestRecord = { id: string, prompt: string, attachments: Array<RequestAttachment>, createdAt: string, outcome: string | null, jobId: string | null, };

export type RequestShape = { "kind": "rect", rect: Rect, }
  | { "kind": "ellipse", rect: Rect, }
  | { "kind": "polygon", points: Array<RequestPoint>, }
  | { "kind": "freehand", points: Array<RequestPoint>, };

export type RequestStatus = "implemented"
  | "conflict";

export type RequestTarget = { "kind": "element", page: string, size?: string | null, id: string, }
  | { "kind": "boundary", page: string, size?: string | null, }
  | { "kind": "reference", id: string, }
  | { "kind": "pointer", id: string, };

export type RequirementsSnapshot = { jobId: string, projectId: string, baseRevision: number, markdown: string, sha256: string, requestCount: number, pageCount: number, 
dependencies: { [key in string]: string }, dossierContext?: GenerationDossierContext | null, 
requestHistory?: string, capabilities?: GenerationCapabilities | null, 
referenceIds: Array<string>, 
images: Array<RequestImage>, layerRequests: Array<FrozenLayerRequest>, };

export type ResponseBodyKind = "unavailable"
  | "eventStream"
  | "empty"
  | "jsonError"
  | "jsonDetail"
  | "jsonResponse"
  | "jsonOther"
  | "html"
  | "text"
  | "binary";

export type ResponseCorrelation = { terminalOutput: TerminalOutput, terminalItems: number, terminalTextParts: number, streamedParts: number, finalizedItems: number, emptyAddedItems: number, cause: CorrelationCause, outputIndex: number | null, contentIndex: number | null, };

export type ResponseFormat = { diagnosticReply?: string | null, contentType: string | null, bodyKind: ResponseBodyKind, bodyBytes: number, providerMessage?: string | null, bodyTruncated: boolean, limitBytes?: number | null, rejection?: ResponseRejection | null, correlation?: ResponseCorrelation | null, };

export type ResponseRejection = "eventJson"
  | "eventType"
  | "eventName"
  | "responseIdentity"
  | "textLocation"
  | "textIdentity"
  | "textOrder"
  | "textMismatch"
  | "outputShape"
  | "messageRole"
  | "messageState"
  | "messagePhase"
  | "contentShape"
  | "answerCount"
  | "credentialEcho"
  | "candidateJson"
  | "candidateShape"
  | "jobMismatch"
  | "requirementsMismatch"
  | "usage";

export type SaveConflict = { token: string, kind: SaveFailureKind, path: string, fileName: string, message: string, defaultChoice: string, canReplace: boolean, details: SaveConflictDetails, };

export type SaveConflictDetails = { expected: FileStamp | null, actual: FileStamp | null, };

export type SaveFailureKind = "changed"
  | "differentProject"
  | "unreadable"
  | "invalidProject"
  | "io";

export type SaveOutcome = { "status": "saved", workspace: WorkspaceView, }
  | { "status": "cancelled" }
  | { "status": "conflict", conflict: SaveConflict, };

export type Scope = { "kind": "project" }
  | { "kind": "page", "id": string }
  | { "kind": "element", "id": string };

export type SizePreset = "phone"
  | "tablet"
  | "desktop"
  | "custom";

export type SizingMode = "fixed"
  | "fit"
  | "fill";

export type SourcePage = { path: string, title: string, url?: string | null, kind: string, photoCount: number, itemCount: number, };

export type SourcePageRef = { path: string, title: string, url?: string | null, };

export type StackAlign = "start"
  | "center"
  | "end"
  | "stretch";

export type StackDirection = "row"
  | "column";

export type StackJustify = "start"
  | "center"
  | "end"
  | "spaceBetween";

export type StyleReferences = { fill?: string | null, stroke?: string | null, textColour?: string | null, text?: string | null, };

export type SubmissionOutcome = "notSubmitted"
  | "rejected"
  | "unknown"
  | "received";

export type TerminalOutput = "notReceived"
  | "absent"
  | "null"
  | "array"
  | "invalid";

export type TextAlign = "left"
  | "center"
  | "right";

export type TextDecoration = "none"
  | "underline"
  | "line-through";

export type TextStyle = { id: string, name: string, fontFamily: string, size: number, weight: number, lineHeight: number, letterSpacing: number, italic: boolean, };

export type TokenUsage = { inputTokens: number, outputTokens: number, totalTokens: number, };

export type UpdateCheck = { current: string, 
latest: UpdateRelease | null, newer: boolean, };

export type UpdateRelease = { 
version: string, name: string, 
url: string, publishedAt: string, };

export type ValidationExchange = { summary: string, requests: Array<RequestOutcome>, usage: TokenUsage | null, };

export type VersionSummary = { id: number, parent: number | null, label: string, createdAt: number, 
head: boolean, };

export type WebCapture = { url: string, 
site: string, 
route: string, title: string, width: number, height: number, nodes: Array<WebNode>, scripts: Array<WebScript>, };

export type WebNode = { tag: string, rect: Rect, text: string | null, src: string | null, poster: string | null, href: string | null, inputType: string | null, placeholder: string | null, value: string | null, alt: string | null, options: Array<string>, attributes: WebNodeAttributes, styles: WebNodeStyles, children: Array<WebNode>, };

export type WebNodeAttributes = { id: string, className: string, role: string, ariaExpanded: string | null, ariaControls: string | null, dataToggle: string | null, name: string | null, };

export type WebNodeStyles = { background: string, color: string, borderColor: string, borderWidth: number, radius: number, opacity: number, shadowX: number, shadowY: number, shadowBlur: number, shadowColor: string, fontFamily: string, fontSize: number, fontWeight: number, italic: boolean, lineHeight: number, letterSpacing: number, align: string, display: string, direction: string, rowGap: number, columnGap: number, padding: number, columns: number, alignItems: string, justifyContent: string, wrap: boolean, };

export type WebScript = { src: string | null, text: string | null, };

export type WebsiteImportPageView = { route: string, title: string, url: string, };

export type WebsiteImportResultView = { workspace: WorkspaceView, pages: number, layers: number, pictures: number, warnings: Array<string>, };

export type WebsiteImportSourceView = { id: string, name: string, baseUrl: string, extractorUrl: string, pages: Array<WebsiteImportPageView>, totalPages: number, identitySource: IdentitySource | null, identityError: string | null, skipped: Array<string>, };

export type WorkspaceView = { project: Project | null, path: string | null, dirty: boolean, 
untitled: boolean, canUndo: boolean, canRedo: boolean, history: HistoryView | null, recovery: RecoveryInfo | null, recoveries: Array<RecoveryInfo>, recoveryError: string | null, openProjects: Array<OpenProjectSummary>, anyDirty: boolean, 
pointerStates: Array<PointerStateView>, };
