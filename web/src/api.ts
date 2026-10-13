export type DocumentIdentity = {
  source: string
  resource: string
  derived_sha: string
}

export type Document = DocumentIdentity & {
  blob_sha: string
  title?: string
  observed_at: string
  tool: string
  chars: number
  chunks: number
  excluded: boolean
  exclusion_reason?: string
  classifications: Record<string, number>
}

export type CorpusFilters = {
  search: string
  address: string
  source: string
  usage: string
  classifier: string
  min_score: string
  max_score: string
}

export type CorpusPage = {
  documents: Document[]
  total: number
  /** Characters over every matching document, to price a run before it starts. */
  total_chars: number
  page: number
  page_size: number
  sources: string[]
  pending: number
  /** What each filter would find, counted without its own filter. Absent from servers before facets. */
  facets?: CorpusFacets
}

export type CorpusFacets = {
  sources: Record<string, number>
  usage: Record<string, number>
  /** Per classifier key: matching documents in each tenth of score, low to high. */
  scores: Record<string, number[]>
  /** Matching documents by what they were read as: `pdf`, `web_page`, `spreadsheet`, … `other`. Absent from servers older than this field. */
  kinds?: Record<string, number>
}

/** Who is signed in, as the gateway keeps it. The first account to sign up is the administrator. */
export type Account = {
  id: string
  name: string
  email: string
  image: string | null
  role: 'admin' | 'user'
  /** A new account waits for the administrator. Every request re-reads this, so revoking is immediate. */
  access: 'pending' | 'approved' | 'revoked'
  /** Set when the account signed in with Nostr. */
  publicKey?: string
}

export type QuestionAction = 'exclude' | 'tag' | 'keep'
export type QuestionKind = 'noul' | 'choice'

/** One option of a choice. The id and description are meaning; the action is policy. */
export type ChoiceOption = {
  id: string
  description: string
  action: QuestionAction
}

export type Question = {
  id: string
  instructions: string
  version: number
  kind?: QuestionKind
  options?: ChoiceOption[]
  threshold: number
  /** Scores from here up to the threshold are held for review. Absent is no review band. */
  review?: number | null
  action: QuestionAction
  /** Ask only of documents carrying this tag: `question:option` or a noul's `question`. Absent is a root. */
  when?: string
}

/** A group of shipped default questions, as the server offers them for adding. */
export type Preset = {
  id: string
  label: string
  detail: string
  questions: Question[]
}

/** What a run covers when it is given no documents: the corpus, one source, or one document. */
export type RunScope = {
  source?: string
  document?: DocumentIdentity
}

/** What a scoped run would send, priced the way the server prices every run. */
export type RunPlan = {
  documents: number
  estimated_tokens: number
  estimated_cost_usd?: number | null
}

/** The Corpus filter a run resolves on the server, plus how many top matches to take. */
export type RunSelection = {
  search: string
  address: string
  source: string
  usage: string
  classifier: string
  min_score?: number
  max_score?: number
  count: number
}

/** What the current policy decides for one document from one question. */
export type Outcome = {
  excluded: boolean
  tags?: string[]
  review: boolean
  /** A choice's winning option. */
  top?: string
  /** A choice's summed probability over its exclude options. */
  exclusion?: number
}

export type RunResult = DocumentIdentity & {
  /**
   * A noul's probability under its id; a choice option's under `question:option`. A
   * question with no key here was not asked: a follow-up whose parent landed elsewhere.
   */
  answers: Record<string, number>
  choices?: Record<string, { choice: string; confidence?: number }>
  sampled?: { sent_chars: number; total_chars: number }
  /** Wall time for this document, retries and smaller resends included. */
  duration_ms?: number
  /** Requests sent: one for each level of the chain asked, plus any retry or smaller resend. */
  attempts?: number
  error?: string
  outcomes?: Record<string, Outcome>
}

/** One document out to Jev right now. */
export type InFlight = {
  source: string
  resource: string
  /** Milliseconds since the Unix epoch. */
  started_ms: number
  attempt: number
  sent_chars: number
}

export type OutcomeTotals = {
  excluded: number
  review: number
  tagged: number
  kept: number
  /** Documents no question of the run reached; nothing was sent for them. */
  not_asked?: number
  errors: number
  sampled: number
}

export type QuestionTotals = {
  excluded: number
  review: number
  tagged: number
  /** Documents this question did not reach. Not a no: it was never asked. */
  not_asked?: number
  top?: Record<string, number>
  tags?: Record<string, number>
}

/** The page of results a run detail holds, and the counts over every result. */
export type RunView = {
  page: number
  page_size: number
  result_total: number
  scored: number
  input_total: number
  documents: OutcomeTotals
  questions: Record<string, QuestionTotals>
  /** While scoring: the documents out to Jev right now. */
  in_flight?: InFlight[]
  /** While scoring: the latest answers, newest first. */
  recent?: RunResult[]
}

export type ResultOutcome = '' | 'exclude' | 'review' | 'tag' | 'keep' | 'not_asked' | 'error'

export type RunDetailQuery = {
  page: number
  page_size: number
  outcome: ResultOutcome
  sort: string
  direction: '' | 'asc' | 'desc'
}

export type Run = {
  id: string
  created_at: string
  completed_at?: string
  model: string
  evaluation_date: string
  questions: Question[]
  effective_questions?: Question[]
  inputs: DocumentIdentity[]
  settings: Record<string, unknown>
  /** The scope the run covered, when it was given one rather than documents. */
  scope?: RunScope
  /** The root of the chain this run continues. */
  continue_from?: string
  document_count: number
  status: string
  input_tokens?: number | null
  output_tokens?: number | null
  cost_usd?: number | null
  cost_estimated?: boolean
  usage_documents?: number
  duration_ms?: number | null
  throughput_docs_sec?: number | null
  errors: number
  /** Documents answered so far, failures included. */
  scored?: number
  /** Why the run stopped short: Jev's own words, or the failure that ended it. */
  error?: string
  /** In a run detail: one page of results, filtered and sorted as asked. */
  results: RunResult[]
  view?: RunView
}

export type RunSummary = Pick<Run,
  'id' | 'created_at' | 'completed_at' | 'model' | 'evaluation_date' | 'document_count' |
  'status' | 'error' | 'input_tokens' | 'output_tokens' | 'cost_usd' | 'cost_estimated' | 'duration_ms' |
  'throughput_docs_sec' | 'errors'
>

export type RunPage = {
  runs: RunSummary[]
  total: number
  page: number
  page_size: number
}

export type ReadReport = {
  url: string
  source: string
  kind: string
  blob_sha: string
  derived_sha: string
  observed_at: string
  tool: string
  text: string
  chars: number
  total_chars: number
  offset: number
  truncated: boolean
}

/** A document as the review tool shows it: the Corpus row plus what the policy decided. */
export type ReviewCandidate = Document & {
  outcomes: Record<string, Outcome>
  /** Some answer sits in a question's review band. */
  review_band: boolean
  reviewed: boolean
}

export type ReviewQueue = {
  documents: ReviewCandidate[]
  in_review_band: number
  reviewed: number
  scored: number
}

/** What the model said beside what the person said, for one question. */
export type Verdict = {
  /** A yes-or-no question's probability, or a choice's winning option. */
  model?: number | string | null
  /** `true`/`false` for a yes-or-no question, an option id for a choice. */
  human: boolean | string
}

/** One line of `workspace/reviews.jsonl`. The server stamps `at`. */
export type Review = DocumentIdentity & {
  verdicts: Record<string, Verdict>
  proposed?: string[]
  note?: string
  reviewer?: string
}

export type ReviewReport = {
  excluded: boolean
  usage_changed: boolean
  tags: string[]
}

export type QuestionEvaluation = {
  id: string
  version: number
  kind: string
  compared: number
  unscored: number
  agreement?: number | null
  threshold: number
  suggested_threshold?: number
  precision?: number
  recall?: number
  true_positive?: number
  false_positive?: number
  false_negative?: number
  true_negative?: number
  confusion?: Record<string, Record<string, number>>
}

export type Evaluation = {
  reviews: number
  documents: number
  questions: QuestionEvaluation[]
  proposed: Record<string, number>
}

/** One unit of work a stage finished: a page fetched, a document read or scored. */
export type JobItem = {
  address: string
  /** An HTTP status for a fetch, a content kind for a read, `scored` or `error` for a run. */
  tag: string
  verdict: 'ok' | 'warn' | 'missing' | 'fail'
  noun: string
  bytes: number
  /** Characters of text, where the stage produces some. */
  produced?: number
  millis: number
  detail?: string
  /** Found inside a page rather than declared by the source. */
  nested?: boolean
}

export type JobOutcome = 'ok' | 'failed' | 'cancelled'

/** What happened to a job, as the server stamped it. `seq` only grows, across every job. */
export type JobEvent = { seq: number; at: number; job: string } & (
  | { type: 'started'; kind: string; label: string; stoppable?: boolean }
  | { type: 'step'; step: string }
  | { type: 'progress'; message: string; done: number; total: number; current?: string }
  | { type: 'item'; item: JobItem }
  | { type: 'note'; message: string }
  | { type: 'finished'; outcome: JobOutcome; error?: string }
)

/** A job as it stands: every event folded in, and the latest readable ones. */
export type JobState = {
  id: string
  /** The op it runs, or `classify` for a workspace run (whose id is the run's). A `run` from the Sources page carries its journal run id. */
  kind: string
  label: string
  /** Whether `api.stopJob` can end it. */
  stoppable?: boolean
  started_at: number
  finished_at?: number
  /** The last event folded in. */
  seq: number
  /** The stage it is in: `tampa.gov · collect`, `embed`. */
  step?: string
  message?: string
  done?: number
  total?: number
  /** The item in hand. */
  current?: string
  ok: number
  failed: number
  outcome?: JobOutcome
  error?: string
  /** Oldest first. Counter ticks are not kept here. */
  log: JobEvent[]
}

/**
 * Development only: hold every request for `VITE_DEV_DELAY_MS`, or for
 * `localStorage['centinel.delayMs']`, to see skeletons and transitions at a slow
 * server's pace. Production builds drop it.
 */
const devDelay = () => {
  if (!import.meta.env.DEV) return 0
  return Number(import.meta.env.VITE_DEV_DELAY_MS || globalThis.localStorage?.getItem('centinel.delayMs') || 0)
}

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const delay = devDelay()
  if (delay) await new Promise(done => setTimeout(done, delay))
  let res: Response
  try {
    res = await fetch(path, { ...init, headers: { 'content-type': 'application/json', ...init?.headers } })
  } catch {
    throw new Error('The Centinel API is unavailable. Start `centinel web`, then reload this page.')
  }
  const text = await res.text()
  let body: unknown
  try { body = JSON.parse(text) as unknown }
  catch { throw new Error('The Centinel API is unavailable. Start `centinel web`, then reload this page.') }
  if (typeof body !== 'object' || body === null || Array.isArray(body)) {
    throw new Error(`${path} returned an invalid response`)
  }
  if (!res.ok) throw new Error(typeof (body as Record<string, unknown>).error === 'string' ? (body as Record<string, unknown>).error as string : `${res.status} ${res.statusText}`)
  return body as T
}

function withArray<T>(value: T, key: keyof T, path: string) {
  if (!Array.isArray(value[key])) throw new Error(`${path} returned an invalid response`)
  return value
}

export function corpusParams(filters: CorpusFilters, page: number, pageSize: number) {
  const params = new URLSearchParams({ page: String(page), page_size: String(pageSize) })
  Object.entries(filters).forEach(([key, value]) => value && params.set(key, value))
  return params
}

export type SystemInfo = {
  /** `CENTINEL_PUBLIC_URL` on the server, when it sits behind another address. */
  public_url?: string | null
  product: string
  api_version: number
  version: string
  web_version: string
  build_id: string
  store_root: string
}

/** One op the registry offers a remote caller. `mcp` says whether agents see it as a tool. */
export type RemoteOp = { name: string; about: string; mcp: boolean; long_running: boolean }

/** Where the reader fetches a document's bytes as collected. */
export const originalUrl = (blob: string, source: string, download = false) =>
  `/workspace/original?${new URLSearchParams({ blob, source, ...(download ? { download: 'true' } : {}) })}`

/** The Jev model a run asks unless the person picks another. */
export const DEFAULT_MODEL = 'jev-1.13.0'

// ── Sources ─────────────────────────────────────────────────────────────────

export type SourceKind = 'site' | 'channel'

/**
 * One source the store defines, as `source list` reports it. `resources` is every address its
 * log knows, discovered or collected; `liveness` counts the collected ones by state.
 */
export type ListedSource = {
  id: string
  kind: SourceKind
  target: string
  enabled: boolean
  resources: number
  liveness: Record<string, number>
  last_run?: LastRun
}

/** The newest run in the journal that covered a source. */
export type LastRun = { started_at: string; outcome: RunAttempt['outcome']; trigger: RunAttempt['trigger']; schedule?: string; documents: number }

/** The run holding the store's one lane. `sources` is empty for a run over every enabled source. */
export type LaneHolder = { pid: number; started_at: string; trigger: 'schedule' | 'manual' | 'catch-up'; schedule?: string; sources?: string[]; args: string }

/** `log/<id>/source.toml`. */
export type SourceDefinition = {
  id: string
  site?: string
  channel?: string
  enabled?: boolean
  rps?: number
  matches?: string[]
  yt_dlp_args?: string[]
  audio_if_no_captions?: boolean
  lang?: string
  strategy?: string
}

export type SourceProblem = { natural_key: string; state: string; since: string; consecutive_failures: number; detail?: string }

/** What the log says of a source: `centinel list`, one row. */
export type SourceSummary = {
  source: string
  defined: boolean
  resources: number
  observations: number
  liveness: Record<string, number>
  problems: SourceProblem[]
  problems_truncated?: number
  last_discovery?: { at: string; method: string; addresses: number }
}

/** One run in the journal. */
export type RunAttempt = {
  run_id: string
  schedule?: string
  trigger: 'schedule' | 'manual' | 'catch-up'
  started_at: string
  finished_at: string
  outcome: 'ok' | 'partial' | 'failed' | 'skipped' | 'interrupted'
  detail?: string
  added: { documents: number; derived: number; chunks: number }
  subtracted: { vanished: number; gone: number; blocked: number; errored: number }
  /** The sources it was started for; empty for every enabled one. */
  sources?: string[]
}

export type SourceDetail = { definition: SourceDefinition; summary?: SourceSummary; runs: RunAttempt[]; running: LaneHolder | null }

export type CrumbStanding = 'open' | 'ignored' | 'promoted'
export type Crumb = { host: string; links: number; pages: number; example: string; standing?: CrumbStanding }
/** The ledger, read. `unrecorded` pages have no row yet and are not counted; a rescan writes them down. */
export type CrumbsList = { action: 'list'; sources: string[]; documents: number; crumbs: Crumb[]; hidden: number; unrecorded: number }

export type Note = { label: string; detail: string; mark?: 'ok' | 'warn' | 'bad' }
export type ReadQuality = { chars: number; bytes: number; chars_per_kb: number; links: number; link_share: number; findings?: string[] }

/** Which part of an op made a request. */
export type RequestStep = { kind: 'robots' | 'seed' | 'page' | 'enclosure' } | { kind: 'probe'; strategy: string }

/** One request an investigation made, and what came back. `status` is absent when no answer arrived at all. */
export type RequestRecord = {
  step: RequestStep
  method: string
  url: string
  status?: number
  content_type?: string
  bytes: number
  elapsed_ms: number
  /** When it was sent, in Unix milliseconds. */
  at_ms: number
  redirected_to?: string
  error?: string
}

export type InvestigateReport = {
  address: string
  seed: { http_status?: string; bytes: number; kind: string; final_url?: string; robots_declared: boolean }
  recognised?: Array<{ strategy: string; keyed_on: string; kind: string; best: boolean; evidence: Note[]; warnings?: Note[] }>
  /** Strategies that were asked and did not recognise it. The registry records no reason. */
  declined?: string[]
  probe?: { addresses: number; complete: boolean; strategy: string; by_fallback: boolean; requests_allowed: number; sample?: string[]; declared_total?: number }
  lead?: { read: ReadQuality; anchors: number; script_bytes: number; sitemap_declared: boolean; findings?: string[] }
  crumbs?: Crumb[]
  promote?: { id: string; site: string; strategy?: string }
  warnings?: string[]
  requests?: RequestRecord[]
  elapsed_secs: number
}

export type CheckedDocument = {
  address: string
  enclosed?: boolean
  bytes: number
  declared_type?: string
  kind: string
  http_status?: string
  final_url?: string
  tool?: string
  title?: string
  chars: number
  verdict?: ReadQuality
  recovered_by_fallback?: boolean
  pages_needing_ocr?: number
  notes?: string[]
  unextractable?: string
  preview?: string
}

export type CheckReport = {
  target: string
  documents: CheckedDocument[]
  not_fetched?: string[]
  enumeration?: { strategy: string; recognised: boolean; truncated: boolean; addresses: number; sample?: string[]; notes?: Note[]; warnings?: string[] }
  requests?: RequestRecord[]
  elapsed_secs: number
}

/** Each half of an investigation, kept whatever became of the other. */
export type Half<T> = { report: T; error?: undefined } | { report?: undefined; error: string }
export type InvestigationReport = { address: string; started_at: string; finished_at: string; investigate: Half<InvestigateReport>; check: Half<CheckReport> }

export type NewSource = { id: string; site?: string; channel?: string; strategy?: string; disabled?: boolean }

// ── Graph ───────────────────────────────────────────────────────────────────

/**
 * A node of the graph: a source the store defines, or a host its pages link to that no
 * source claims. `id` is `source:<id>` or `host:<host>`, which is what an edge names. A
 * host some source claims is not a node: the edge lands on that source.
 */
export type GraphNode =
  | { kind: 'source'; id: string; source: string; acquisition: SourceKind; target: string; host?: string; enabled: boolean; addresses: number }
  | { kind: 'host'; id: string; host: string; pages: number; links: number; sources: number; standing: CrumbStanding }

/** One source's pages pointing at one node. `pages` is the weight; `links` is how many times. */
export type GraphEdge = { from: string; to: string; pages: number; links: number }

/** The whole graph's numbers, before the filter, so a view can say what it leaves out. */
export type GraphTotals = { sources: number; hosts: number; edges: number; links: number; open: number; refused: number }

export type GraphView = { nodes: GraphNode[]; edges: GraphEdge[]; totals: GraphTotals; truncated: boolean }

/** Edges carried by fewer pages are left out, and so is a host no edge reaches. */
export type GraphFilter = { min_pages: number; hide_refused: boolean }

/** One host's card: who links there, where on the host, and from which pages. */
export type HostDetail = {
  host: string
  standing: CrumbStanding
  note?: string
  pages: number
  links: number
  example?: string
  sources: Array<{ source: string; pages: number; links: number; example: string }>
  paths: Array<{ path: string; pages: number }>
  paths_more: number
  /** `blob` is the page as collected, which `originalUrl` opens. */
  referrers: Array<{ source: string; address: string; blob?: string; at?: string }>
  referrers_more: number
}

/** What a refusal or an allow answers with: the ruling as `crumbs ignore` reports it. */
export type Ruled = { action: 'ruled'; host: string; ruling: 'ignore' | 'allow'; at: string; note?: string; previous?: string }

export const api = {
  /** The signed-in account, or null when nobody is. */
  account: () => request<{ account: Account | null }>('/api/account').then(value => value.account),
  /** Every account, newest first. The administrator only. */
  users: () => request<{ users: Account[] }>('/api/admin/users').then(value => withArray(value, 'users', '/api/admin/users').users),
  setAccess: (id: string, access: Account['access']) => request<Account>('/api/admin/users', { method: 'POST', body: JSON.stringify({ id, access }) }),
  system: () => request<SystemInfo>('/workspace/system'),
  ops: () => request<{ ops: RemoteOp[] }>('/ops').then(value => withArray(value, 'ops', '/ops')),
  corpus: (params: URLSearchParams) => {
    const path = `/workspace/documents?${params}`
    return request<CorpusPage>(path).then(value => withArray(withArray(value, 'documents', path), 'sources', path))
  },
  read: (identity: DocumentIdentity) => {
    const params = new URLSearchParams(identity)
    return request<ReadReport>(`/workspace/document?${params}`)
  },
  questions: () => request<{ questions: Question[] }>('/workspace/questions').then(value => withArray(value, 'questions', '/workspace/questions')),
  saveQuestions: (questions: Question[]) => request<{ questions: Question[] }>('/workspace/questions', {
    method: 'PUT',
    body: JSON.stringify({ questions }),
  }),
  /** The shipped defaults, grouped. The server seeds a new store's saved set from these. */
  presets: () => request<{ presets: Preset[] }>('/workspace/presets').then(value => withArray(value, 'presets', '/workspace/presets')),
  runs: (page = 1, pageSize = 25) => {
    const params = new URLSearchParams({ page: String(page), page_size: String(pageSize) })
    const path = `/workspace/runs?${params}`
    return request<RunPage>(path).then(value => withArray(value, 'runs', path))
  },
  runDetail: (id: string, query?: Partial<RunDetailQuery>) => {
    const params = new URLSearchParams()
    Object.entries(query || {}).forEach(([key, value]) => value !== '' && value != null && params.set(key, String(value)))
    const suffix = params.toString() ? `?${params}` : ''
    return request<Run>(`/workspace/runs/${encodeURIComponent(id)}${suffix}`)
  },
  /** Starts a run. The answer is the run as started; poll `runDetail` until its status leaves `running`. */
  run: (body: {
    documents?: DocumentIdentity[]
    selection?: RunSelection
    /** Scores the exact inputs of this stored run again; the server copies them. */
    repeat?: string
    /** Every indexed document in scope, resolved on the server as the run goes. No cap. */
    scope?: RunScope
    questions: Question[]
    model: string
    evaluation_date: string
    settings: Record<string, unknown>
    /** `false` returns the scores without writing a run record. */
    record?: boolean
  }) => request<Run>('/workspace/runs', { method: 'POST', body: JSON.stringify(body) }),
  /** Carries a stopped run on: its scope, questions and settings, over what no run in its chain answered. */
  continueRun: (id: string) => request<Run>('/workspace/runs', { method: 'POST', body: JSON.stringify({ continue_from: id }) }),
  /** What a classify run over a scope would send, and its cost, from the server's one estimate. */
  classifyPlan: (plan: { source?: string; document?: DocumentIdentity; questions: string[]; model: string }) => {
    const params = new URLSearchParams({ questions: plan.questions.join(','), model: plan.model })
    if (plan.document) Object.entries(plan.document).forEach(([key, value]) => params.set(key, value))
    else if (plan.source) params.set('source', plan.source)
    return request<RunPlan>(`/workspace/classify/plan?${params}`)
  },
  restore: (identity: DocumentIdentity) => request<unknown>('/workspace/restore', {
    method: 'POST',
    body: JSON.stringify(identity),
  }),
  /** The review band first, then a random sample of decided documents. */
  reviewQueue: (query: { page_size: number }) => {
    const path = `/workspace/review/queue?${new URLSearchParams({ page_size: String(query.page_size) })}`
    return request<ReviewQueue>(path).then(value => withArray(value, 'documents', path))
  },
  /** Records a person's verdicts on one document, and acts on them at once. */
  review: (review: Review) => request<ReviewReport>('/workspace/review', { method: 'POST', body: JSON.stringify(review) }),
  evaluation: () => request<Evaluation>('/workspace/evaluation').then(value => withArray(value, 'questions', '/workspace/evaluation')),
  /** Every model call since `since`, by hour, model and stage, with every price. */
  spend: (since: string) => {
    const path = `/workspace/spend?${new URLSearchParams({ since })}`
    return request<SpendSummary>(path).then(value => withArray(value, 'buckets', path))
  },
  /** What each model is priced at. Local models carry the cloud model they are priced as. */
  prices: () => request<{ prices: SpendPrice[] }>('/workspace/prices').then(value => withArray(value, 'prices', '/workspace/prices')),
  /** The host each source collects from, by source id. Readable by any approved user, unlike `sources`. */
  sourceHosts: () => request<Record<string, string>>('/workspace/source-hosts'),
  sources: () => request<{ sources: ListedSource[]; running: LaneHolder | null }>('/workspace/sources').then(value => withArray(value, 'sources', '/workspace/sources')),
  source: (id: string) => request<SourceDetail>(`/workspace/sources/${encodeURIComponent(id)}`),
  sourceCrumbs: (id: string) => request<CrumbsList>(`/workspace/sources/${encodeURIComponent(id)}/crumbs`).then(value => withArray(value, 'crumbs', 'crumbs')),
  /** Reads every blob of the source once and writes its crumb rows, as a job. The answer is the job's id. */
  rescanCrumbs: (id: string) => request<{ id: string }>(`/workspace/sources/${encodeURIComponent(id)}/crumbs/rescan`, { method: 'POST' }),
  /** Asks a stoppable job to stop at its next item. A run so stopped is journaled `interrupted`. */
  stopJob: (id: string) => request<{ stopped: string }>(`/workspace/jobs/${encodeURIComponent(id)}/stop`, { method: 'POST' }),
  addSource: (source: NewSource) => request<unknown>('/workspace/sources', { method: 'POST', body: JSON.stringify(source) }),
  setSourceEnabled: (id: string, enabled: boolean) => request<unknown>(`/workspace/sources/${encodeURIComponent(id)}/enabled`, { method: 'POST', body: JSON.stringify({ enabled }) }),
  /** Starts a run in the store's one lane. `run_id` is the journal's handle and the job's id, so the page opens the drawer on it. */
  runSource: (id: string, body: { skip?: string[]; limit?: number; refresh?: boolean } = {}) => request<{ run_id: string }>(`/workspace/sources/${encodeURIComponent(id)}/run`, { method: 'POST', body: JSON.stringify(body) }),
  /** Starts an investigation of an address, as a job. The answer is the job's id, which is also the report's. */
  investigate: (url: string) => request<{ id: string }>('/workspace/investigations', { method: 'POST', body: JSON.stringify({ url }) }),
  investigation: (id: string) => request<InvestigationReport>(`/workspace/investigations/${encodeURIComponent(id)}`),
  /** The graph as a filter asks for it. Sources always come back; a host does while an edge reaches it. */
  graph: (filter: GraphFilter) => {
    const path = `/workspace/graph?${new URLSearchParams({ min_pages: String(filter.min_pages), hide_refused: String(filter.hide_refused) })}`
    return request<GraphView>(path).then(value => withArray(withArray(value, 'nodes', path), 'edges', path))
  },
  graphHost: (host: string) => request<HostDetail>(`/workspace/graph/host/${encodeURIComponent(host)}`),
  /** The same corpus-wide ruling `centinel crumbs ignore` writes. */
  refuseHost: (host: string, note?: string) => request<Ruled>(`/workspace/graph/host/${encodeURIComponent(host)}/refuse`, { method: 'POST', body: JSON.stringify({ note }) }),
  allowHost: (host: string) => request<Ruled>(`/workspace/graph/host/${encodeURIComponent(host)}/allow`, { method: 'POST', body: JSON.stringify({}) }),
}

/** Who ran a model: TypeSafe's Jev, OpenRouter, or weights on this machine. */
export type SpendProvider = 'jev' | 'open_router' | 'local'
/** What a call was for. */
export type SpendStage = 'classify' | 'embed' | 'query'

/** One model's calls for one stage in one UTC hour. */
export type SpendBucket = {
  hour: string
  model: string
  provider: SpendProvider
  stage: SpendStage
  requests: number
  input_tokens: number
  output_tokens: number
  /** Summed over the priced requests. */
  cost_usd: number
  /** Requests whose model has no price. */
  unpriced: number
}

/** USD per million tokens. `model` ending in `*` covers a family. */
export type SpendPrice = {
  model: string
  provider: SpendProvider
  input: number
  output: number
  source: string
  /** For a local model: the cloud model whose price it takes. */
  priced_as?: string
}

export type SpendSummary = { since: string; read_at: string; buckets: SpendBucket[] }

/**
 * The live job stream: a `snapshot` event with every job as it stands, then a `job` event
 * per change; `?job=` for one. `GET /workspace/jobs` answers the snapshot alone.
 */
export const jobEventsUrl = '/workspace/jobs/events'
