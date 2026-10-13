import { keepPreviousData, queryOptions, skipToken } from '@tanstack/react-query'
import { api, corpusParams, type CorpusFilters, type DocumentIdentity, type JobState, type RunDetailQuery } from './api'

/**
 * Every read the workspace makes, one builder each. A route's loader warms the cache
 * with the same options its component reads, so a page arrives with its data. Keys keep
 * one prefix per kind (`corpus`, `runs`, `run`, …) for invalidation to aim at.
 */
export const queries = {
  /** Who is signed in. Re-read every 15 s while someone is, so a revocation lands within that. */
  account: () => queryOptions({ queryKey: ['account'], queryFn: api.account, retry: false, refetchInterval: query => query.state.data ? 15_000 : false }),
  users: () => queryOptions({ queryKey: ['users'], queryFn: api.users, refetchInterval: 15_000 }),
  system: () => queryOptions({ queryKey: ['system'], queryFn: api.system, staleTime: 60_000 }),
  ops: () => queryOptions({ queryKey: ['ops'], queryFn: api.ops, staleTime: Infinity }),
  questions: () => queryOptions({ queryKey: ['questions'], queryFn: api.questions }),
  presets: () => queryOptions({ queryKey: ['presets'], queryFn: api.presets, staleTime: Infinity }),
  evaluation: () => queryOptions({ queryKey: ['evaluation'], queryFn: api.evaluation }),
  /**
   * Every job. Written only by `useJobEvents`, from the stream's own snapshot and then its
   * events: a separate read could land after newer events and put the jobs back in time.
   */
  jobs: () => queryOptions<JobState[]>({ queryKey: ['jobs'], queryFn: skipToken, staleTime: Infinity }),

  /** A page of the corpus, as the filters and page ask for it. */
  corpus: (filters: Partial<CorpusFilters>, page = 1, pageSize = 25) => {
    const params = corpusParams({ ...noFilters, ...filters }, page, pageSize)
    return queryOptions({ queryKey: ['corpus', params.toString()], queryFn: () => api.corpus(params), placeholderData: keepPreviousData })
  },

  /** A page of the run ledger. Polls while one of its runs is scoring. */
  runs: (page: number, pageSize = 25) => queryOptions({
    queryKey: ['runs', page, pageSize],
    queryFn: () => api.runs(page, pageSize),
    staleTime: 5_000,
    refetchInterval: query => query.state.data?.runs.some(run => run.status === 'running') ? 2000 : false,
  }),

  /** One run as a view asks for it. Polls every `every` ms while it is scoring. */
  run: (id: string, view: Partial<RunDetailQuery>, every = 1000) => queryOptions({
    queryKey: ['run', id, view],
    queryFn: () => api.runDetail(id, view),
    enabled: Boolean(id),
    staleTime: 5_000,
    placeholderData: keepPreviousData,
    refetchInterval: query => query.state.data?.status === 'running' ? every : false,
  }),

  /** A document's extracted text. One key per identity, whoever asks. */
  read: (doc: DocumentIdentity) => queryOptions({
    queryKey: ['read', doc.source, doc.resource, doc.derived_sha],
    queryFn: () => api.read(doc),
    staleTime: Infinity,
  }),

  /** The price list. It changes with a release, not while the page is open. */
  prices: () => queryOptions({ queryKey: ['prices'], queryFn: api.prices, staleTime: Infinity }),

  /** Spend since the start of a window. The window moves with the clock, so it refreshes. */
  spend: (since: string) => queryOptions({
    queryKey: ['spend', since],
    queryFn: () => api.spend(since),
    placeholderData: keepPreviousData,
    refetchInterval: 60_000,
  }),

  /** What a classify run over a scope would send. The corpus changes under it, so it is short-lived. */
  plan: (plan: Parameters<typeof api.classifyPlan>[0]) => queryOptions({
    queryKey: ['classify-plan', plan],
    queryFn: () => api.classifyPlan(plan),
    staleTime: 10_000,
    placeholderData: keepPreviousData,
  }),

  /** Which host each source collects from, for its icon. A source's host does not change; a new source is picked up within the stale time. */
  sourceHosts: () => queryOptions({ queryKey: ['source-hosts'], queryFn: api.sourceHosts, staleTime: 10 * 60_000 }),

  /** Every source, and the run holding the lane. Re-read while a run holds it, so a row's state follows. */
  sources: () => queryOptions({
    queryKey: ['sources'],
    queryFn: api.sources,
    refetchInterval: query => query.state.data?.running ? 3000 : 30_000,
  }),

  /** One source: its definition, what its log says, its runs. Faster while a run holds the lane. */
  source: (id: string) => queryOptions({
    queryKey: ['source', id],
    queryFn: () => api.source(id),
    refetchInterval: query => query.state.data?.running ? 3000 : 30_000,
  }),

  /** A source's off-host links. Reading them can take seconds on a large source, so they are asked once. */
  sourceCrumbs: (id: string) => queryOptions({ queryKey: ['source-crumbs', id], queryFn: () => api.sourceCrumbs(id), staleTime: 5 * 60_000 }),

  /** What a try found. A try never changes once filed. */
  tried: (id: string) => queryOptions({ queryKey: ['tried', id], queryFn: () => api.tried(id), staleTime: Infinity, retry: false }),

  reviewQueue: (pageSize: number) => queryOptions({
    queryKey: ['review-queue', pageSize],
    queryFn: () => api.reviewQueue({ page_size: pageSize }),
    staleTime: Infinity,
  }),
}

const noFilters: CorpusFilters = { search: '', address: '', source: '', usage: 'all', classifier: '', min_score: '', max_score: '' }
