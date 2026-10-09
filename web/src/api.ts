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
}

/** A group of shipped default questions, as the server offers them for adding. */
export type Preset = {
  id: string
  label: string
  detail: string
  questions: Question[]
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
  /** A noul's probability under its id; a choice option's under `question:option`. */
  answers: Record<string, number>
  choices?: Record<string, { choice: string; confidence?: number }>
  sampled?: { sent_chars: number; total_chars: number }
  /** Wall time for this document, retries and smaller resends included. */
  duration_ms?: number
  /** Requests sent. More than one is a retry or a smaller resend. */
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
  errors: number
  sampled: number
}

export type QuestionTotals = {
  excluded: number
  review: number
  tagged: number
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

export type ResultOutcome = '' | 'exclude' | 'review' | 'tag' | 'keep' | 'error'

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
  preview: {
    affected_documents: number
    affected_chunks: number
    affected_chars: number
  }
  /** In a run detail: one page of results, filtered and sorted as asked. */
  results: RunResult[]
  view?: RunView
}

export type RunSummary = Pick<Run,
  'id' | 'created_at' | 'completed_at' | 'model' | 'evaluation_date' | 'document_count' |
  'status' | 'input_tokens' | 'output_tokens' | 'cost_usd' | 'cost_estimated' | 'duration_ms' |
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

async function request<T>(path: string, init?: RequestInit): Promise<T> {
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

export const api = {
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
    questions: Question[]
    model: string
    evaluation_date: string
    settings: Record<string, unknown>
    /** `false` returns the scores without writing a run record. */
    record?: boolean
  }) => request<Run>('/workspace/runs', { method: 'POST', body: JSON.stringify(body) }),
  commit: (id: string) => request<Run>(`/workspace/runs/${encodeURIComponent(id)}/commit`, { method: 'POST' }),
  restore: (identity: DocumentIdentity) => request<unknown>('/workspace/restore', {
    method: 'POST',
    body: JSON.stringify(identity),
  }),
  /** The review band first, then a random sample of decided documents. */
  reviewQueue: (query: { source?: string; page_size?: number; include_reviewed?: boolean }) => {
    const params = new URLSearchParams()
    if (query.source) params.set('source', query.source)
    if (query.page_size) params.set('page_size', String(query.page_size))
    if (query.include_reviewed) params.set('include_reviewed', 'true')
    const path = `/workspace/review/queue?${params}`
    return request<ReviewQueue>(path).then(value => withArray(value, 'documents', path))
  },
  /** Records a person's verdicts on one document, and acts on them at once. */
  review: (review: Review) => request<ReviewReport>('/workspace/review', { method: 'POST', body: JSON.stringify(review) }),
  evaluation: () => request<Evaluation>('/workspace/evaluation').then(value => withArray(value, 'questions', '/workspace/evaluation')),
}
