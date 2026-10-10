import { keepPreviousData, queryOptions, skipToken } from '@tanstack/react-query'
import { api, corpusParams, type CorpusFilters, type DocumentIdentity, type JobState, type RunDetailQuery } from './api'

/**
 * Every read the workspace makes, one builder each. A route's loader warms the cache
 * with the same options its component reads, so a page arrives with its data. Keys keep
 * one prefix per kind (`corpus`, `runs`, `run`, …) for invalidation to aim at.
 */
export const queries = {
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

  reviewQueue: (source: string, includeReviewed: boolean, pageSize: number) => queryOptions({
    queryKey: ['review-queue', source, includeReviewed, pageSize],
    queryFn: () => api.reviewQueue({ source, page_size: pageSize, include_reviewed: includeReviewed }),
    staleTime: Infinity,
  }),
}

const noFilters: CorpusFilters = { search: '', address: '', source: '', usage: 'all', classifier: '', min_score: '', max_score: '' }
